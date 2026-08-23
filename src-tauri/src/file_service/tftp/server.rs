//! TFTP server runtime lifecycle: binding, the accept loop that hands each
//! RRQ/WRQ to its own session task, and graceful shutdown.

use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tauri::AppHandle;
use tokio::{
    net::UdpSocket,
    sync::{watch, Semaphore},
    task::JoinSet,
    time::timeout,
};

use crate::{
    elevated::{self, BindSpec, BoundSocket, ServiceRule},
    file_service::{
        firewall,
        models::{
            canonical_shared_dir, emit_file_service_config, emit_file_transfer, parse_bind_address,
            FileServiceConfig, TransferRegistry,
        },
    },
    logging,
};

use super::{
    packet::{parse_request, Opcode, MAX_PACKET_SIZE},
    session::{send_error, serve_session, ActiveRequests, SessionContext},
};

const TFTP_MAX_SESSIONS: usize = 64;
const TFTP_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Identifies an in-flight request so retransmitted RRQ/WRQ packets (clients
/// resend the request until the first response arrives, RFC 1350) do not
/// spawn competing sessions for the same transfer.
type ActiveRequestKey = (SocketAddr, Opcode, String);

pub(crate) struct TftpRuntimeHandle {
    shutdown_tx: watch::Sender<bool>,
    pub(crate) accept_tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
}

pub(crate) async fn start_runtime(
    app: AppHandle,
    config: FileServiceConfig,
) -> Result<TftpRuntimeHandle, String> {
    let bind_addr = parse_bind_address("TFTP", &config.bind_ip, config.port)?;
    let bind_specs = if !bind_addr.ip().is_unspecified() {
        vec![BindSpec::udp(bind_addr, false)]
    } else {
        local_listener_ips(bind_addr.is_ipv4())
            .into_iter()
            .map(|ip| BindSpec::udp(SocketAddr::new(ip, bind_addr.port()), true))
            .collect()
    };
    let listeners = elevated::bind_service_sockets(
        ServiceRule {
            prefix: "XTerm TFTP",
            action: "tftp.firewall.allow",
            protocol: crate::firewall::FirewallProtocol::Udp,
            ports: vec![config.port],
            all_udp: true,
        },
        bind_specs,
        Some(BindSpec::udp(bind_addr, false)),
    )
    .await?
    .into_iter()
    .map(|socket| match socket {
        BoundSocket::Udp(socket) => UdpSocket::from_std(socket)
            .map_err(|error| format!("failed to prepare TFTP server socket: {error}")),
        BoundSocket::Tcp(_) => Err("The TFTP listener protocol was invalid.".to_string()),
    })
    .collect::<Result<Vec<_>, _>>()?;
    let root = canonical_shared_dir("TFTP", &config.shared_dir).await?;

    let shared = TransferRegistry::new();
    let active_requests: ActiveRequests = Arc::new(parking_lot::Mutex::new(HashSet::new()));
    let session_permits = Arc::new(Semaphore::new(TFTP_MAX_SESSIONS));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let stopped_config = config.clone();
    let mut accept_tasks = Vec::new();
    for socket in listeners {
        let local_ip = socket.local_addr().map_err(|error| error.to_string())?.ip();
        let emitter_app = app.clone();
        let context = SessionContext {
            emitter: Arc::new(move |event| emit_file_transfer(&emitter_app, event)),
            shared: shared.clone(),
            root: root.clone(),
            local_ip,
            session_permits: session_permits.clone(),
            active_requests: active_requests.clone(),
        };
        let task_app = app.clone();
        let task_config = stopped_config.clone();
        let loop_shutdown = shutdown_rx.clone();
        accept_tasks.push(tauri::async_runtime::spawn(async move {
            // A failed accept loop means the socket is gone; re-emit the
            // config so the UI reflects that the service is no longer running.
            if accept_loop(socket, context, loop_shutdown).await {
                emit_file_service_config(&task_app, task_config);
            }
        }));
    }

    logging::event("tftp.runtime", "tftp.start.success")
        .field("bind_addr", bind_addr)
        .field("shared_dir", &config.shared_dir)
        .info();
    Ok(TftpRuntimeHandle {
        shutdown_tx,
        accept_tasks,
    })
}

pub(crate) async fn stop_runtime<R: tauri::Runtime>(
    app: &AppHandle<R>,
    runtime: TftpRuntimeHandle,
    port: u16,
) -> Result<(), String> {
    let _ = runtime.shutdown_tx.send(true);
    for task in runtime.accept_tasks {
        await_runtime_task("tftp.accept", task).await;
    }
    firewall::remove_tftp_port_rule(app, port)
        .await
        .map_err(|error| error.user_message.clone())?;
    Ok(())
}

async fn await_runtime_task(name: &'static str, mut task: tauri::async_runtime::JoinHandle<()>) {
    tokio::select! {
        result = &mut task => if let Err(error) = result { logging::event("tftp.runtime", "tftp.task.join_failed").field("task", name).field("error", error.to_string()).warn(); },
        _ = tokio::time::sleep(TFTP_TASK_DRAIN_TIMEOUT) => { task.abort(); let _ = task.await; }
    }
}

/// Returns true when the loop ended because receiving on the socket failed
/// (as opposed to a requested shutdown).
async fn accept_loop(
    socket: UdpSocket,
    context: SessionContext,
    mut shutdown_rx: watch::Receiver<bool>,
) -> bool {
    let mut packet = [0u8; MAX_PACKET_SIZE];
    let mut sessions = JoinSet::new();
    let mut failed = false;
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() { break; }
            }
            received = socket.recv_from(&mut packet) => {
                let (size, peer) = match received {
                    Ok(received) => received,
                    Err(error) => {
                        logging::event("tftp.runtime", "tftp.accept.failed")
                            .field("error", error.to_string())
                            .warn();
                        failed = true;
                        break;
                    }
                };
                let request = match parse_request(&packet[..size]) {
                    Ok(request) => request,
                    Err(error) => {
                        if let super::error::TransferError::Send(code, message) = error {
                            let _ = send_error(&socket, peer, code, &message).await;
                        }
                        continue;
                    }
                };
                let key: ActiveRequestKey = (peer, request.opcode, request.filename.clone());
                if !context.active_requests.lock().insert(key.clone()) {
                    // A retransmission of a request that already owns a
                    // session; that session retransmits its response on its
                    // own timer, so a competing session would only leak a
                    // phantom transfer entry.
                    continue;
                }
                let session_context = context.clone();
                let session_shutdown = shutdown_rx.clone();
                let active_requests = context.active_requests.clone();
                sessions.spawn(async move {
                    let result =
                        serve_session(session_context, peer, request, session_shutdown).await;
                    active_requests.lock().remove(&key);
                    if let Err(error) = result {
                        logging::event("tftp.runtime", "tftp.transfer.failed")
                            .field("peer", peer)
                            .field("error", error)
                            .warn();
                    }
                });
            }
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(Err(error)) = completed {
                    logging::event("tftp.runtime", "tftp.transfer.task_failed")
                        .field("error", error.to_string())
                        .warn();
                }
            }
        }
    }
    // Sessions observe the shutdown flag through their own watch receivers;
    // give them a brief window to terminate transfers politely before aborting.
    let drain = async { while sessions.join_next().await.is_some() {} };
    if timeout(TFTP_TASK_DRAIN_TIMEOUT, drain).await.is_err() {
        sessions.abort_all();
        while sessions.join_next().await.is_some() {}
    }
    failed
}

/// Binds the request socket(s).  A specific bind address uses a single
/// socket; the wildcard binds one socket per concrete interface address
/// instead, so every reply is sent from the exact address the client used to
/// reach the server.  Routing-based source selection picks the wrong
/// interface on multi-homed hosts (overlapping subnets, virtual adapters),
/// and the client then never sees the response and keeps retransmitting.
fn local_listener_ips(ipv4: bool) -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces
            .into_iter()
            .map(|interface| interface.ip())
            .filter(|ip| !ip.is_unspecified() && ip.is_ipv4() == ipv4)
            .collect(),
        Err(error) => {
            logging::event("tftp.runtime", "tftp.listener.enum_failed")
                .field("error", error.to_string())
                .warn();
            Vec::new()
        }
    };
    ips.sort();
    ips.dedup();
    ips
}

#[cfg(test)]
async fn bind_listeners(bind_addr: SocketAddr) -> Result<Vec<UdpSocket>, String> {
    if !bind_addr.ip().is_unspecified() {
        return UdpSocket::bind(bind_addr)
            .await
            .map(|socket| vec![socket])
            .map_err(|error| error.to_string());
    }
    let mut sockets = Vec::new();
    for ip in local_listener_ips(bind_addr.is_ipv4()) {
        if let Ok(socket) = UdpSocket::bind(SocketAddr::new(ip, bind_addr.port())).await {
            sockets.push(socket);
        }
    }
    if sockets.is_empty() {
        sockets.push(
            UdpSocket::bind(bind_addr)
                .await
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(sockets)
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, sync::Arc, time::Duration};

    use tokio::{net::UdpSocket, sync::watch, time::timeout};

    use super::{super::session::SessionContext, accept_loop, bind_listeners, ActiveRequests};
    use crate::{file_service::models::TransferRegistry, ids};

    const IO_TIMEOUT: Duration = Duration::from_secs(5);

    fn request_packet(opcode: u16, filename: &str) -> Vec<u8> {
        let mut packet = opcode.to_be_bytes().to_vec();
        packet.extend_from_slice(filename.as_bytes());
        packet.push(0);
        packet.extend_from_slice(b"octet");
        packet.push(0);
        packet
    }

    async fn recv_packet(client: &UdpSocket, buffer: &mut [u8]) -> (usize, std::net::SocketAddr) {
        timeout(IO_TIMEOUT, client.recv_from(buffer))
            .await
            .expect("timed out waiting for a server packet")
            .expect("client socket failed")
    }

    /// Drives the real accept loop through a full WRQ upload and RRQ download
    /// over UDP, proving the session TID handoff works end to end.
    #[tokio::test]
    async fn upload_and_download_round_trip_over_udp() {
        let directory = std::env::temp_dir().join(format!("xterm-tftp-e2e-{}", ids::new_id()));
        std::fs::create_dir(&directory).unwrap();
        // The runtime stores a canonicalized root; path validation compares
        // against it, so the test must use the canonical form too (Windows
        // canonicalize returns verbatim \\?\ paths).
        let directory = std::fs::canonicalize(&directory).unwrap();
        let download_payload: Vec<u8> = (0..1500u32).map(|value| (value % 251) as u8).collect();
        std::fs::write(directory.join("seed.bin"), &download_payload).unwrap();

        let listener = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let active_requests: ActiveRequests = Arc::new(parking_lot::Mutex::new(Default::default()));
        let context = SessionContext {
            emitter: Arc::new(|_| {}),
            shared: TransferRegistry::new(),
            root: directory.clone(),
            local_ip: Ipv4Addr::LOCALHOST.into(),
            session_permits: Arc::new(tokio::sync::Semaphore::new(4)),
            active_requests: active_requests.clone(),
        };
        let server = tokio::spawn(accept_loop(listener, context, shutdown_rx));

        let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut buffer = [0u8; 2048];

        // --- WRQ upload ---
        client
            .send_to(&request_packet(2, "upload.bin"), listen_addr)
            .await
            .unwrap();
        let (size, transfer_addr) = recv_packet(&client, &mut buffer).await;
        assert_eq!(
            &buffer[..size],
            &[0, 4, 0, 0],
            "WRQ must be answered by ACK(0)"
        );
        assert_ne!(
            transfer_addr.port(),
            listen_addr.port(),
            "server must answer from a new transfer ID (ephemeral port)"
        );

        // A retransmitted WRQ (client never saw ACK(0)) must not spawn a
        // competing session; the existing session retransmits on its own.
        client
            .send_to(&request_packet(2, "upload.bin"), listen_addr)
            .await
            .unwrap();
        let duplicate = timeout(Duration::from_millis(500), client.recv_from(&mut buffer)).await;
        assert!(
            duplicate.is_err(),
            "duplicate WRQ should not produce another session response"
        );
        assert_eq!(active_requests.lock().len(), 1);

        let upload_payload = b"hello tftp upload";
        let mut data = 3u16.to_be_bytes().to_vec();
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(upload_payload);
        client.send_to(&data, transfer_addr).await.unwrap();
        let (size, _) = recv_packet(&client, &mut buffer).await;
        assert_eq!(&buffer[..size], &[0, 4, 0, 1], "final DATA must be ACKed");

        // The upload commits asynchronously; poll briefly for the rename.
        let committed = directory.join("upload.bin");
        for _ in 0..50 {
            if committed.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(std::fs::read(&committed).unwrap(), upload_payload);

        // --- RRQ download ---
        client
            .send_to(&request_packet(1, "seed.bin"), listen_addr)
            .await
            .unwrap();
        let mut received = Vec::new();
        let mut expected_block = 1u16;
        loop {
            let (size, from) = recv_packet(&client, &mut buffer).await;
            assert_eq!(
                u16::from_be_bytes([buffer[0], buffer[1]]),
                3,
                "expected DATA"
            );
            let block = u16::from_be_bytes([buffer[2], buffer[3]]);
            assert_eq!(block, expected_block, "blocks must arrive in order");
            received.extend_from_slice(&buffer[4..size]);
            let ack = [0, 4, buffer[2], buffer[3]];
            client.send_to(&ack, from).await.unwrap();
            expected_block = expected_block.wrapping_add(1);
            if size < 4 + 512 {
                break;
            }
        }
        assert_eq!(received, download_payload);

        shutdown_tx.send(true).unwrap();
        timeout(IO_TIMEOUT, server)
            .await
            .expect("accept loop did not shut down")
            .unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[tokio::test]
    async fn wildcard_bind_creates_one_listener_per_interface() {
        let sockets = bind_listeners("0.0.0.0:0".parse().unwrap()).await.unwrap();
        assert!(!sockets.is_empty());
        for socket in &sockets {
            let addr = socket.local_addr().unwrap();
            assert!(!addr.ip().is_unspecified());
        }
        assert!(
            sockets
                .iter()
                .any(|socket| socket.local_addr().unwrap().ip().is_loopback()),
            "wildcard listeners should include loopback for local clients"
        );
    }
}
