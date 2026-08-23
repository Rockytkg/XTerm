use std::{
    convert::Infallible,
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::{
    body::Incoming,
    header::{
        CONNECTION, HOST, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING,
        UPGRADE,
    },
    http::{header::HeaderName, uri::Authority, HeaderMap, HeaderValue},
    service::service_fn,
    upgrade, Method, Request, Response, StatusCode, Uri,
};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::{timeout, Instant},
};

use crate::{
    elevated::{self, BindSpec, BoundSocket, ServiceRule},
    firewall::{self, FirewallProtocol},
    logging,
    network_interface::validate_bind_ip,
    proxy::models::{emit_proxy_stats, ProxyConfig, ProxySharedState},
    state::AppState,
};

type ProxyBody = BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;
type ProxyTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
const PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");
const KEEP_ALIVE_HEADER: HeaderName = HeaderName::from_static("keep-alive");
const VIA_HEADER: HeaderName = HeaderName::from_static("via");
const PROXY_VIA_VALUE: &str = "1.1 xterm";
const STATS_IDLE_INTERVAL: Duration = Duration::from_secs(1);
const PROXY_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const TUNNEL_BUFFER_SIZE: usize = 16 * 1024;
const MAX_ACTIVE_CLIENT_CONNECTIONS: usize = 2048;
const PROXY_TASK_QUEUE_CAPACITY: usize = 1024;
const CONNECT_DENIED_PORTS: &[u16] = &[25];

pub(crate) struct ProxyRuntimeHandle {
    pub(crate) shared: Arc<ProxySharedState>,
    shutdown_tx: watch::Sender<bool>,
    accept_task: tauri::async_runtime::JoinHandle<()>,
    stats_task: tauri::async_runtime::JoinHandle<()>,
}

impl ProxyRuntimeHandle {
    pub(crate) fn is_running(&self) -> bool {
        !self.accept_task.inner().is_finished()
    }
}

struct ProxyRequestHandler {
    peer_addr: SocketAddr,
    request: Request<Incoming>,
    client: Client<HttpConnector, ProxyBody>,
    shared: Arc<ProxySharedState>,
    shutdown_rx: watch::Receiver<bool>,
    task_tx: mpsc::Sender<ProxyTask>,
    connection_slot: Arc<OwnedSemaphorePermit>,
}

impl ProxyRequestHandler {
    async fn handle(self) -> Response<ProxyBody> {
        if self.request.method() == Method::CONNECT {
            return handle_connect_request(
                self.peer_addr,
                self.request,
                self.shared,
                self.shutdown_rx,
                self.task_tx,
                self.connection_slot,
            )
            .await;
        }

        forward_http_request(
            self.peer_addr,
            self.request,
            self.client,
            self.shared,
            self.shutdown_rx,
        )
        .await
    }
}

pub(crate) async fn start_runtime(
    app: AppHandle,
    port: u16,
    bind_ip: String,
) -> Result<ProxyRuntimeHandle, String> {
    validate_bind_ip(&bind_ip)?;
    let bind_addr = parse_bind_address(&bind_ip, port)?;
    let listener = match elevated::bind_service_sockets(
        ServiceRule {
            prefix: "XTerm Proxy",
            action: "proxy.firewall.allow",
            protocol: FirewallProtocol::Tcp,
            ports: vec![port],
            all_udp: false,
        },
        vec![BindSpec::tcp(bind_addr)],
        None,
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| "The proxy listener was not created.".to_string())?
    {
        BoundSocket::Tcp(listener) => {
            TcpListener::from_std(listener).map_err(|error| format_bind_error(bind_addr, error))?
        }
        BoundSocket::Udp(_) => return Err("The proxy listener protocol was invalid.".to_string()),
    };

    let shared = ProxySharedState::new();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (task_tx, task_rx) = mpsc::channel(PROXY_TASK_QUEUE_CAPACITY);
    let client = build_client();
    let connection_slots = Arc::new(Semaphore::new(MAX_ACTIVE_CLIENT_CONNECTIONS));
    let accept_shared = shared.clone();
    let accept_task = tauri::async_runtime::spawn(async move {
        accept_loop(
            listener,
            client,
            accept_shared,
            connection_slots,
            shutdown_rx,
            task_tx,
            task_rx,
        )
        .await;
    });

    let stats_config = ProxyConfig {
        bind_ip,
        port,
        running: true,
    };
    emit_proxy_stats(&app, &shared.snapshot(&stats_config));
    let stats_shared = shared.clone();
    let stats_shutdown = shutdown_tx.subscribe();
    let stats_task = tauri::async_runtime::spawn(async move {
        stats_loop(app, stats_shared, stats_config, stats_shutdown).await;
    });

    logging::event("proxy.runtime", "proxy.start.success")
        .field("bind_addr", bind_addr)
        .info();
    Ok(ProxyRuntimeHandle {
        shared,
        shutdown_tx,
        accept_task,
        stats_task,
    })
}

pub(crate) async fn stop_runtime(runtime: ProxyRuntimeHandle, port: u16) -> Result<(), String> {
    let _ = runtime.shutdown_tx.send(true);
    await_runtime_task("proxy.accept", runtime.accept_task).await;
    await_runtime_task("proxy.stats", runtime.stats_task).await;
    firewall::remove_service_port_rule(
        "XTerm Proxy",
        "proxy.firewall.remove",
        port,
        FirewallProtocol::Tcp,
    )
    .await
    .map_err(|error| error.user_message.clone())?;
    logging::event("proxy.runtime", "proxy.stop.success")
        .field("port", port)
        .info();
    Ok(())
}

pub(crate) async fn shutdown_proxy<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (runtime, port) = {
        let mut manager = state.proxy();
        (manager.runtime.take(), manager.config.port)
    };
    let Some(runtime) = runtime else {
        return;
    };
    // Best effort on exit: stop_runtime also removes the firewall rule, so a
    // stale inbound allow rule never outlives the application.
    if let Err(error) = stop_runtime(runtime, port).await {
        logging::event("proxy.runtime", "proxy.shutdown.stop_failed")
            .field("error", error)
            .warn();
    }
}

fn build_client() -> Client<HttpConnector, ProxyBody> {
    let mut connector = HttpConnector::new();
    connector.enforce_http(true);
    connector.set_connect_timeout(Some(UPSTREAM_CONNECT_TIMEOUT));
    connector.set_nodelay(true);
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(16)
        .build(connector)
}

async fn accept_loop(
    listener: TcpListener,
    client: Client<HttpConnector, ProxyBody>,
    shared: Arc<ProxySharedState>,
    connection_slots: Arc<Semaphore>,
    mut shutdown_rx: watch::Receiver<bool>,
    task_tx: mpsc::Sender<ProxyTask>,
    mut task_rx: mpsc::Receiver<ProxyTask>,
) {
    let mut tasks = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            Some(task) = task_rx.recv() => {
                tasks.spawn(task);
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                log_proxy_task_result("proxy.client", result);
            }
            accept_result = listener.accept() => {
                let (stream, peer_addr) = match accept_result {
                    Ok(connection) => connection,
                    Err(error) => {
                        logging::event("proxy.runtime", "proxy.accept.failed")
                            .field("error", error.to_string())
                            .warn();
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let Ok(connection_slot) = connection_slots.clone().try_acquire_owned() else {
                    logging::event("proxy.runtime", "proxy.accept.busy")
                        .field("peer_addr", peer_addr)
                        .warn();
                    continue;
                };
                let connection_slot = Arc::new(connection_slot);
                let _ = stream.set_nodelay(true);
                let service_shared = shared.clone();
                let service_client = client.clone();
                let service_shutdown = shutdown_rx.clone();
                let service_task_tx = task_tx.clone();
                tasks.spawn(async move {
                    if let Err(error) = serve_client(
                        peer_addr,
                        stream,
                        service_client,
                        service_shared.clone(),
                        service_shutdown,
                        service_task_tx,
                        connection_slot,
                    )
                    .await
                    {
                        logging::event("proxy.runtime", "proxy.connection.failed")
                            .field("peer_addr", peer_addr)
                            .field("error", error)
                            .warn();
                    }
                });
            }
        }
    }

    drop(listener);
    drop(task_tx);
    drain_proxy_tasks(tasks, task_rx).await;
}

async fn stats_loop(
    app: AppHandle,
    shared: Arc<ProxySharedState>,
    config: ProxyConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(STATS_IDLE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_emit = Instant::now();

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                let elapsed = last_emit.elapsed();
                emit_proxy_stats(&app, &shared.snapshot_with_window(&config, elapsed));
                last_emit = Instant::now();
            }
        }
    }

    let elapsed = last_emit.elapsed();
    emit_proxy_stats(&app, &shared.snapshot_with_window(&config, elapsed));
}

async fn serve_client(
    peer_addr: SocketAddr,
    stream: TcpStream,
    client: Client<HttpConnector, ProxyBody>,
    shared: Arc<ProxySharedState>,
    shutdown_rx: watch::Receiver<bool>,
    task_tx: mpsc::Sender<ProxyTask>,
    connection_slot: Arc<OwnedSemaphorePermit>,
) -> Result<(), String> {
    let mut connection_shutdown = shutdown_rx.clone();
    let service = service_fn(move |request| {
        handle_proxy_request(
            peer_addr,
            request,
            client.clone(),
            shared.clone(),
            shutdown_rx.clone(),
            task_tx.clone(),
            connection_slot.clone(),
        )
    });

    let connection = hyper::server::conn::http1::Builder::new()
        .keep_alive(true)
        .header_read_timeout(HTTP_HEADER_READ_TIMEOUT)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();

    tokio::pin!(connection);
    tokio::select! {
        result = &mut connection => result.map_err(|error| error.to_string()),
        changed = connection_shutdown.changed() => {
            if changed.is_err() || *connection_shutdown.borrow() {
                Ok(())
            } else {
                (&mut connection).await.map_err(|error| error.to_string())
            }
        }
    }
}

async fn handle_proxy_request(
    peer_addr: SocketAddr,
    request: Request<Incoming>,
    client: Client<HttpConnector, ProxyBody>,
    shared: Arc<ProxySharedState>,
    shutdown_rx: watch::Receiver<bool>,
    task_tx: mpsc::Sender<ProxyTask>,
    connection_slot: Arc<OwnedSemaphorePermit>,
) -> Result<Response<ProxyBody>, Infallible> {
    Ok(ProxyRequestHandler {
        peer_addr,
        request,
        client,
        shared,
        shutdown_rx,
        task_tx,
        connection_slot,
    }
    .handle()
    .await)
}

async fn handle_connect_request(
    peer_addr: SocketAddr,
    request: Request<Incoming>,
    shared: Arc<ProxySharedState>,
    shutdown_rx: watch::Receiver<bool>,
    task_tx: mpsc::Sender<ProxyTask>,
    connection_slot: Arc<OwnedSemaphorePermit>,
) -> Response<ProxyBody> {
    let Some(authority) = request.uri().authority().cloned() else {
        return text_response(StatusCode::BAD_REQUEST, "CONNECT target is missing.");
    };
    let target = match validate_connect_authority(&authority) {
        Ok(target) => target,
        Err(error) => return text_response(StatusCode::BAD_REQUEST, &error),
    };
    let server = match connect_upstream(target.as_str()).await {
        Ok(server) => server,
        Err(error) => {
            logging::event("proxy.runtime", "proxy.connect.upstream_failed")
                .field("peer_addr", peer_addr)
                .field("target", target.as_str())
                .field("error", &error)
                .warn();
            return text_response(StatusCode::BAD_GATEWAY, "CONNECT target is unavailable.");
        }
    };

    logging::event("proxy.runtime", "proxy.connect.accepted")
        .field("peer_addr", peer_addr)
        .field("target", target.as_str())
        .debug();

    let task = Box::pin(async move {
        let _connection_slot = connection_slot;
        match upgrade::on(request).await {
            Ok(upgraded) => {
                if let Err(error) =
                    tunnel_connect(target, upgraded, server, shared, shutdown_rx).await
                {
                    logging::event("proxy.runtime", "proxy.connect.failed")
                        .field("peer_addr", peer_addr)
                        .field("error", error)
                        .warn();
                }
            }
            Err(error) => {
                logging::event("proxy.runtime", "proxy.connect.upgrade_failed")
                    .field("peer_addr", peer_addr)
                    .field("error", error)
                    .warn();
            }
        }
    });
    if let Err(error) = task_tx.try_send(task) {
        logging::event("proxy.runtime", "proxy.connect.task_queue_failed")
            .field("peer_addr", peer_addr)
            .field("error", error.to_string())
            .warn();
        return text_response(StatusCode::SERVICE_UNAVAILABLE, "Proxy is busy.");
    }

    Response::builder()
        .status(StatusCode::OK)
        .body(empty_body())
        .unwrap_or_else(|_| text_response(StatusCode::INTERNAL_SERVER_ERROR, "Proxy error."))
}

async fn tunnel_connect(
    target: String,
    upgraded: hyper::upgrade::Upgraded,
    mut server: TcpStream,
    shared: Arc<ProxySharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    let mut upgraded = TokioIo::new(upgraded);
    tokio::select! {
        result = copy_tunnel(&mut upgraded, &mut server, shared) => {
            result.map_err(|error| format!("failed to proxy CONNECT tunnel for {target}: {error}"))?;
        }
        changed = shutdown_rx.changed() => {
            if changed.is_err() || *shutdown_rx.borrow() {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn forward_http_request(
    peer_addr: SocketAddr,
    request: Request<Incoming>,
    client: Client<HttpConnector, ProxyBody>,
    shared: Arc<ProxySharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Response<ProxyBody> {
    let forward_uri = match resolve_forward_uri(&request) {
        Ok(uri) => uri,
        Err(error) => return text_response(StatusCode::BAD_REQUEST, &error),
    };

    logging::event("proxy.runtime", "proxy.http.forward")
        .field("peer_addr", peer_addr)
        .field("method", request.method())
        .field("uri", forward_uri.to_string())
        .trace();

    let (mut parts, body) = request.into_parts();
    let host = match host_header_value(&forward_uri) {
        Ok(value) => value,
        Err(error) => return text_response(StatusCode::BAD_REQUEST, &error),
    };
    parts.headers.insert(HOST, host);
    parts.uri = forward_uri;
    strip_hop_by_hop_headers(&mut parts.headers);
    append_via_header(&mut parts.headers);
    let body = body.map_frame({
        let shared = shared.clone();
        move |frame| {
            if let Some(data) = frame.data_ref() {
                shared.record_upload(data.len() as u64);
            }
            frame
        }
    });
    let outbound = Request::from_parts(parts, body.map_err(box_error).boxed());

    let response = tokio::select! {
        response = client.request(outbound) => response,
        _ = shutdown_rx.changed() => {
            return text_response(StatusCode::SERVICE_UNAVAILABLE, "Proxy is shutting down.");
        }
    };

    match response {
        Ok(mut response) => {
            append_via_header(response.headers_mut());
            response.map(|body| {
                body.map_frame({
                    let shared = shared.clone();
                    move |frame| {
                        if let Some(data) = frame.data_ref() {
                            shared.record_download(data.len() as u64);
                        }
                        frame
                    }
                })
                .map_err(box_error)
                .boxed()
            })
        }
        Err(error) => {
            logging::event("proxy.runtime", "proxy.http.forward.failed")
                .field("peer_addr", peer_addr)
                .field("error", &error)
                .warn();
            text_response(StatusCode::BAD_GATEWAY, "Upstream request failed.")
        }
    }
}

fn resolve_forward_uri<B>(request: &Request<B>) -> Result<Uri, String> {
    if let (Some(scheme), Some(authority)) = (request.uri().scheme_str(), request.uri().authority())
    {
        if !scheme.eq_ignore_ascii_case("http") {
            return Err(
                "Only plain HTTP requests can be forwarded directly; use CONNECT for TLS targets."
                    .to_string(),
            );
        }
        validate_authority(authority)?;
        return Ok(request.uri().clone());
    }

    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Proxy request is missing a Host header.".to_string())?;
    validate_host_header(host)?;

    let path = request
        .uri()
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or("/");
    let mut uri = String::with_capacity("http://".len() + host.len() + path.len());
    uri.push_str("http://");
    uri.push_str(host);
    uri.push_str(path);

    uri.parse::<Uri>()
        .map_err(|_| "Proxy request URI is invalid.".to_string())
}

fn validate_connect_authority(authority: &Authority) -> Result<String, String> {
    validate_authority(authority)?;
    let port = authority
        .port_u16()
        .ok_or_else(|| "CONNECT target must include an explicit port.".to_string())?;
    if CONNECT_DENIED_PORTS.contains(&port) {
        return Err(format!("CONNECT target port {port} is not allowed."));
    }
    Ok(authority.to_string())
}

fn validate_authority(authority: &Authority) -> Result<(), String> {
    if authority.as_str().trim().is_empty() {
        return Err("CONNECT target is empty.".to_string());
    }
    if authority.host().trim().is_empty() {
        return Err("Proxy target host is empty.".to_string());
    }
    validate_target_host(authority.host())?;
    Ok(())
}

fn validate_host_header(host: &str) -> Result<(), String> {
    let authority = host
        .parse::<Authority>()
        .map_err(|_| "Proxy Host header is invalid.".to_string())?;
    validate_authority(&authority)?;
    Ok(())
}

async fn connect_upstream(target: &str) -> Result<TcpStream, String> {
    let stream = timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(target))
        .await
        .map_err(|_| format!("timed out connecting to {target}"))?
        .map_err(|error| format!("failed to connect to {target}: {error}"))?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn host_header_value(uri: &Uri) -> Result<HeaderValue, String> {
    let authority = uri
        .authority()
        .ok_or_else(|| "Proxy request URI is missing a host.".to_string())?;
    let host = authority.host();
    let host = match authority.port_u16() {
        Some(port) if host.contains(':') && !host.starts_with('[') => format!("[{host}]:{port}"),
        Some(port) => format!("{host}:{port}"),
        None if host.contains(':') && !host.starts_with('[') => format!("[{host}]"),
        None => host.to_string(),
    };
    host.parse::<HeaderValue>()
        .map_err(|_| "Proxy request Host header is invalid.".to_string())
}

fn validate_target_host(host: &str) -> Result<(), String> {
    let normalized = host.trim().trim_start_matches('[').trim_end_matches(']');
    if normalized.eq_ignore_ascii_case("localhost") {
        return Err("Proxy target host is not allowed.".to_string());
    }

    let Ok(ip) = normalized.parse::<IpAddr>() else {
        return Ok(());
    };
    if is_denied_target(ip) {
        return Err("Proxy target host is not allowed.".to_string());
    }
    // IPv4-compatible/mapped IPv6 literals are not loopback/link-local until
    // viewed as IPv4: ::ffff:127.0.0.1 would slip through without this check.
    if let IpAddr::V6(v6) = ip {
        if let Some(v4) = v6.to_ipv4() {
            if is_denied_target(IpAddr::V4(v4)) {
                return Err("Proxy target host is not allowed.".to_string());
            }
        }
    }
    Ok(())
}

fn is_denied_target(ip: IpAddr) -> bool {
    let link_local = match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unicast_link_local(),
    };
    ip.is_loopback() || ip.is_unspecified() || link_local
}

fn append_via_header(headers: &mut HeaderMap) {
    headers.append(VIA_HEADER, HeaderValue::from_static(PROXY_VIA_VALUE));
}

fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    if headers.contains_key(CONNECTION) {
        let connection_tokens = headers
            .get_all(CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
            .collect::<Vec<_>>();

        for header in connection_tokens {
            headers.remove(header);
        }
    }

    for header in [
        &CONNECTION,
        &KEEP_ALIVE_HEADER,
        &PROXY_AUTHENTICATE,
        &PROXY_AUTHORIZATION,
        &PROXY_CONNECTION,
        &TE,
        &TRAILER,
        &TRANSFER_ENCODING,
        &UPGRADE,
    ] {
        headers.remove(header);
    }
}

async fn drain_proxy_tasks(mut tasks: JoinSet<()>, mut task_rx: mpsc::Receiver<ProxyTask>) {
    let remaining = tasks.len();
    let drained = timeout(PROXY_TASK_DRAIN_TIMEOUT, async {
        loop {
            if tasks.is_empty() {
                match task_rx.recv().await {
                    Some(task) => {
                        tasks.spawn(task);
                    }
                    None => break,
                }
                continue;
            }

            tokio::select! {
                Some(task) = task_rx.recv() => {
                    tasks.spawn(task);
                }
                Some(result) = tasks.join_next() => {
                    log_proxy_task_result("proxy.client", result);
                }
            }
        }
    })
    .await;

    if drained.is_err() {
        logging::event("proxy.runtime", "proxy.tasks.force_shutdown")
            .field("remaining", remaining)
            .warn();
        tasks.shutdown().await;
    }
}

async fn await_runtime_task(name: &'static str, mut task: tauri::async_runtime::JoinHandle<()>) {
    tokio::select! {
        result = &mut task => {
            if let Err(error) = result {
                logging::event("proxy.runtime", "proxy.task.join_failed")
                    .field("task", name)
                    .field("error", error.to_string())
                    .warn();
            }
        }
        _ = tokio::time::sleep(PROXY_TASK_DRAIN_TIMEOUT) => {
            logging::event("proxy.runtime", "proxy.task.join_failed")
                .field("task", name)
                .field("error", "timed out waiting for runtime task")
                .warn();
            task.abort();
            let _ = task.await;
        }
    }
}

fn log_proxy_task_result(task: &'static str, result: Result<(), tokio::task::JoinError>) {
    if let Err(error) = result {
        logging::event("proxy.runtime", "proxy.task.join_failed")
            .field("task", task)
            .field("error", error.to_string())
            .warn();
    }
}

async fn copy_tunnel(
    client: &mut (impl AsyncRead + AsyncWrite + Unpin),
    server: &mut TcpStream,
    shared: Arc<ProxySharedState>,
) -> Result<(), std::io::Error> {
    let (client_read, client_write) = tokio::io::split(client);
    let (server_read, server_write) = tokio::io::split(server);
    let upload_shared = shared.clone();
    let download_shared = shared;

    tokio::try_join!(
        copy_tracked(client_read, server_write, upload_shared, |shared, bytes| {
            shared.record_upload(bytes)
        }),
        copy_tracked(
            server_read,
            client_write,
            download_shared,
            |shared, bytes| { shared.record_download(bytes) }
        ),
    )?;
    Ok(())
}

async fn copy_tracked<R, W, F>(
    mut reader: R,
    mut writer: W,
    shared: Arc<ProxySharedState>,
    record_bytes: F,
) -> Result<(), std::io::Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: Fn(&ProxySharedState, u64),
{
    // Keep the tunnel buffer on the task stack; every CONNECT tunnel runs this
    // loop, so avoiding a heap allocation here matters under connection churn.
    let mut buffer = [0_u8; TUNNEL_BUFFER_SIZE];
    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            writer.shutdown().await?;
            return Ok(());
        }
        writer.write_all(&buffer[..bytes_read]).await?;
        record_bytes(&shared, bytes_read as u64);
    }
}

fn parse_bind_address(bind_ip: &str, port: u16) -> Result<SocketAddr, String> {
    let ip = bind_ip
        .parse()
        .map_err(|_| format!("invalid bind IP address '{bind_ip}'"))?;
    Ok(SocketAddr::new(ip, port))
}

fn format_bind_error(bind_addr: SocketAddr, error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!(
                "Port {} is already in use on {}.",
                bind_addr.port(),
                bind_addr.ip()
            )
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("The proxy could not bind to {bind_addr} because access was denied.")
        }
        _ => format!("Failed to bind the proxy to {bind_addr}: {error}"),
    }
}

fn box_error(
    error: impl std::error::Error + Send + Sync + 'static,
) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(error)
}

fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn text_body(text: &str) -> ProxyBody {
    Full::new(Bytes::copy_from_slice(text.as_bytes()))
        .map_err(|never| match never {})
        .boxed()
}

fn text_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .body(text_body(message))
        .unwrap_or_else(|_| Response::new(text_body(message)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str, host: Option<&str>) -> Request<Empty<Bytes>> {
        let mut builder = Request::builder().uri(uri);
        if let Some(host) = host {
            builder = builder.header(HOST, host);
        }
        builder.body(Empty::new()).expect("request")
    }

    #[test]
    fn resolves_absolute_http_uri() {
        let request = request("http://example.com/path?q=1", None);
        assert_eq!(
            resolve_forward_uri(&request).expect("uri").to_string(),
            "http://example.com/path?q=1"
        );
    }

    #[test]
    fn resolves_origin_form_from_host() {
        let request = request("/path?q=1", Some("example.com"));
        assert_eq!(
            resolve_forward_uri(&request).expect("uri").to_string(),
            "http://example.com/path?q=1"
        );
    }

    #[test]
    fn rejects_direct_https_forwarding() {
        let request = request("https://example.com/path", None);
        assert!(resolve_forward_uri(&request).is_err());
    }

    #[test]
    fn strips_static_and_connection_named_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, "x-debug, keep-alive".parse().unwrap());
        headers.insert("x-debug", "1".parse().unwrap());
        headers.insert(PROXY_CONNECTION, "keep-alive".parse().unwrap());
        headers.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        headers.insert(HOST, "example.com".parse().unwrap());

        strip_hop_by_hop_headers(&mut headers);

        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key("x-debug"));
        assert!(!headers.contains_key(PROXY_CONNECTION));
        assert!(!headers.contains_key(TRANSFER_ENCODING));
        assert!(headers.contains_key(HOST));
    }

    #[test]
    fn connect_authority_requires_port() {
        let authority = "example.com".parse::<Authority>().unwrap();
        assert!(validate_connect_authority(&authority).is_err());
        let authority = "example.com:443".parse::<Authority>().unwrap();
        assert_eq!(
            validate_connect_authority(&authority).expect("authority"),
            "example.com:443"
        );
    }

    #[test]
    fn connect_authority_rejects_denied_ports() {
        let authority = "example.com:25".parse::<Authority>().unwrap();
        assert!(validate_connect_authority(&authority).is_err());
    }

    #[test]
    fn rejects_invalid_host_header() {
        let request = request("/path", Some("bad host"));
        assert!(resolve_forward_uri(&request).is_err());
    }

    #[test]
    fn rejects_local_proxy_targets() {
        for uri in [
            "http://localhost/path",
            "http://127.0.0.1/path",
            "http://[::1]/path",
            "http://169.254.10.20/path",
            "http://[fe80::1]/path",
        ] {
            let request = request(uri, None);
            assert!(resolve_forward_uri(&request).is_err(), "{uri}");
        }
    }

    #[test]
    fn rejects_ipv4_mapped_ipv6_targets() {
        for uri in [
            "http://[::ffff:127.0.0.1]/path",
            "http://[::ffff:169.254.10.20]/path",
            "http://[::ffff:0.0.0.0]/path",
        ] {
            let request = request(uri, None);
            assert!(resolve_forward_uri(&request).is_err(), "{uri}");
        }
    }

    #[test]
    fn connect_authority_rejects_ipv4_mapped_loopback() {
        let authority = "[::ffff:127.0.0.1]:443".parse::<Authority>().unwrap();
        assert!(validate_connect_authority(&authority).is_err());
    }

    #[test]
    fn host_header_preserves_explicit_port() {
        let uri = "http://example.com:80/path".parse::<Uri>().unwrap();
        assert_eq!(host_header_value(&uri).unwrap(), "example.com:80");
    }

    #[test]
    fn host_header_formats_ipv6_authority() {
        let uri = "http://[2001:db8::1]:8080/path".parse::<Uri>().unwrap();
        assert_eq!(host_header_value(&uri).unwrap(), "[2001:db8::1]:8080");
    }

    #[test]
    fn appends_via_header() {
        let mut headers = HeaderMap::new();
        append_via_header(&mut headers);
        assert_eq!(headers.get(VIA_HEADER).unwrap(), PROXY_VIA_VALUE);
    }
}
