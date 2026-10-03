use std::{future::Future, ops::RangeInclusive, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use libunftp::{
    notification::{DataEvent, DataListener, EventMeta},
    options::{ActivePassiveMode, Shutdown},
    Server, ServerBuilder,
};
use tauri::AppHandle;
use tokio::sync::watch;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, DefaultUser, Principal};
use unftp_sbe_fs::Filesystem;

use crate::{
    elevated::{self, ServiceRule},
    file_service::{
        firewall::{self, FTP_RULE_PREFIX},
        manager::SharedPassword,
        models::{
            await_runtime_task, canonical_shared_dir, emit_file_service_config, emit_file_transfer,
            parse_bind_address, validate_service_config, FileServiceConfig, TransferRegistry,
            DEFAULT_FTP_PASSIVE_END, DEFAULT_FTP_PASSIVE_START,
        },
        password::passwords_equal,
    },
    logging,
};

const FTP_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const FTP_GRACE_PERIOD: Duration = Duration::from_secs(5);
const FTP_IDLE_SESSION_TIMEOUT_SECS: u64 = 300;

pub(crate) struct FtpRuntimeHandle {
    shutdown_tx: watch::Sender<bool>,
    pub(crate) accept_task: tauri::async_runtime::JoinHandle<()>,
    passive_ports: RangeInclusive<u16>,
}

#[derive(Debug)]
struct PasswordAuthenticator {
    username: String,
    password: SharedPassword,
}

#[async_trait]
impl Authenticator for PasswordAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        credentials: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        // 审计拒绝事件：只记用户名，绝不记口令。
        if username != self.username {
            logging::event("ftp.runtime", "ftp.auth.rejected")
                .field("username", username)
                .info();
            return Err(AuthenticationError::BadUser);
        }
        let provided = credentials.password.as_deref().unwrap_or("");
        let expected = self.password.read().clone();
        if !passwords_equal(provided, &expected) {
            logging::event("ftp.runtime", "ftp.auth.rejected")
                .field("username", username)
                .info();
            return Err(AuthenticationError::BadPassword);
        }
        Ok(Principal {
            username: username.to_string(),
        })
    }
}

#[derive(Debug)]
struct TransferListener {
    app: AppHandle,
    shared: Arc<TransferRegistry>,
}

#[async_trait]
impl DataListener for TransferListener {
    async fn receive_data_event(&self, event: DataEvent, meta: EventMeta) {
        let (direction, path, bytes) = match event {
            DataEvent::Got { path, bytes } => ("read", path, bytes),
            DataEvent::Put { path, bytes } => ("write", path, bytes),
            _ => return,
        };
        let transfer_id = crate::ids::new_id();
        self.shared
            .start_transfer(&transfer_id, direction, &path, &meta.username, bytes);
        if let Some(event) = self.shared.transfer_event(&transfer_id, false, None) {
            emit_file_transfer(&self.app, event);
        }
        // DataEvent 是传输结束时一次性触发并携带最终字节数：把它计入状态，
        // 让下面的完成事件能带上完整的 transferred 总量（返回值无需再发事件）。
        let _ = self.shared.record_progress(&transfer_id, bytes);
        if let Some(event) = self.shared.finish_transfer(&transfer_id, None) {
            emit_file_transfer(&self.app, event);
        }
    }
}

/// 组装 libunftp 服务器及其固定选项集。不触碰 AppHandle，使 e2e 测试可以
/// 用完全相同的配置验证真实的数据通道行为（尤其是 0.0.0.0 通配绑定）。
fn build_server(
    root: PathBuf,
    auth: Arc<PasswordAuthenticator>,
    passive_ports: RangeInclusive<u16>,
    data_listener: impl DataListener + 'static,
    shutdown_indicator: impl Future<Output = Shutdown> + Send + Sync + 'static,
) -> Result<Server<Filesystem, DefaultUser>, String> {
    // libunftp 的存储工厂闭包签名无法返回 Result；先在这里打开一次根目录，
    // 把配置性错误变成干净的启动失败，闭包内的 expect 只剩“运行期目录被删除”
    // 这一极端路径。
    Filesystem::new(&root)
        .map_err(|error| format!("failed to open FTP shared directory: {error}"))?;
    ServerBuilder::with_authenticator(
        Box::new(move || Filesystem::new(root.clone()).expect("FTP root validated at startup")),
        auth,
    )
    .passive_ports(passive_ports)
    .active_passive_mode(ActivePassiveMode::ActiveAndPassive)
    .idle_session_timeout(FTP_IDLE_SESSION_TIMEOUT_SECS)
    .notify_data(data_listener)
    .shutdown_indicator(shutdown_indicator)
    .build()
    .map_err(|error| error.to_string())
}

pub(crate) async fn start_runtime(
    app: AppHandle,
    config: &FileServiceConfig,
    password: SharedPassword,
) -> Result<FtpRuntimeHandle, String> {
    validate_service_config("FTP", config)?;
    let root = canonical_shared_dir("FTP", &config.shared_dir).await?;
    let passive_ports = DEFAULT_FTP_PASSIVE_START..=DEFAULT_FTP_PASSIVE_END;
    let bind_addr = parse_bind_address("FTP", &config.bind_ip, config.port)?;
    // libunftp 只接受地址字符串并自行 bind，无法注入预绑定 socket，因此 Linux 下
    // 特权端口（21）无法复用 pkexec bind helper。先同步探测 bind，让 EADDRINUSE/
    // EACCES 立即返回，而不是在 accept task 内失败后才靠事件向前端纠正状态。
    std::net::TcpListener::bind(bind_addr)
        .map_err(|error| crate::elevated::format_bind_error(bind_addr, &error))?;
    // 探测 socket 随语句结束立即释放，真正的监听仍由 libunftp 在 accept task 内完成。
    // libunftp 直接监听真实地址，确保主动模式下控制连接和数据连接看到
    // 相同的远端 IP。防火墙规则独立管理，不再引入会丢失对端地址的代理。
    elevated::allow_service_rule(&ServiceRule {
        prefix: FTP_RULE_PREFIX,
        action: "ftp.firewall.allow",
        protocol: crate::firewall::FirewallProtocol::Tcp,
        ports: std::iter::once(config.port)
            .chain(passive_ports.clone())
            .collect(),
        all_udp: false,
    })
    .await?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let auth = Arc::new(PasswordAuthenticator {
        username: config.username.clone(),
        password,
    });
    let listener = TransferListener {
        app: app.clone(),
        shared: TransferRegistry::new(),
    };
    let task_port = config.port;
    let task_passive_ports = passive_ports.clone();
    let stopped_config = config.clone();
    let status_app = app.clone();
    let shutdown_observer = shutdown_rx.clone();
    let accept_task = tauri::async_runtime::spawn(async move {
        let shutdown_indicator = async move {
            let mut receiver = shutdown_rx;
            let _ = receiver.changed().await;
            Shutdown::new().grace_period(FTP_GRACE_PERIOD)
        };
        let result = match build_server(
            root,
            auth,
            task_passive_ports.clone(),
            listener,
            shutdown_indicator,
        ) {
            Ok(server) => server
                .listen(bind_addr.to_string())
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            logging::event("ftp.runtime", "ftp.task.failed")
                .field("error", &error)
                .warn();
            if !*shutdown_observer.borrow() {
                // 服务从未对外提供：就地回收启动时放行的防火墙规则（删除对不
                // 存在的规则幂等），否则规则会残留到用户下次停止或退出应用时。
                if let Err(cleanup_error) =
                    firewall::remove_ftp_ports(task_port, task_passive_ports).await
                {
                    logging::event("ftp.runtime", "ftp.firewall.cleanup_failed")
                        .field("error", cleanup_error)
                        .warn();
                }
                emit_file_service_config(&status_app, stopped_config);
            }
        }
    });
    logging::event("ftp.runtime", "ftp.start.success")
        .field("bind_ip", &config.bind_ip)
        .field("port", config.port)
        .field("passive_start", DEFAULT_FTP_PASSIVE_START)
        .field("passive_end", DEFAULT_FTP_PASSIVE_END)
        .info();
    Ok(FtpRuntimeHandle {
        shutdown_tx,
        accept_task,
        passive_ports,
    })
}

pub(crate) async fn stop_runtime(runtime: FtpRuntimeHandle, port: u16) -> Result<(), String> {
    let _ = runtime.shutdown_tx.send(true);
    let task_result = await_runtime_task("FTP", FTP_TASK_DRAIN_TIMEOUT, runtime.accept_task).await;
    let firewall_result = firewall::remove_ftp_ports(port, runtime.passive_ports).await;
    // 两个清理步骤相互独立：任一失败都要上报，不能让先到的 `?` 吞掉另一个错误。
    let errors = [task_result, firewall_result]
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    logging::event("ftp.runtime", "ftp.stop.success")
        .field("port", port)
        .info();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        ops::RangeInclusive,
        path::PathBuf,
        sync::Arc,
        time::Duration,
    };

    use async_trait::async_trait;
    use libunftp::{
        notification::{DataEvent, DataListener, EventMeta},
        options::Shutdown,
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::{
            tcp::{OwnedReadHalf, OwnedWriteHalf},
            TcpListener, TcpStream,
        },
        sync::watch,
        time::timeout,
    };

    use super::{build_server, PasswordAuthenticator};

    const IO_TIMEOUT: Duration = Duration::from_secs(10);

    #[derive(Debug)]
    struct NopListener;

    #[async_trait]
    impl DataListener for NopListener {
        async fn receive_data_event(&self, _event: DataEvent, _meta: EventMeta) {}
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    struct FtpClient {
        reader: BufReader<OwnedReadHalf>,
        writer: OwnedWriteHalf,
    }

    impl FtpClient {
        async fn connect(addr: SocketAddr) -> Self {
            let stream = timeout(IO_TIMEOUT, TcpStream::connect(addr))
                .await
                .expect("control connect timed out")
                .expect("control connect failed");
            let (reader, writer) = stream.into_split();
            let mut client = Self {
                reader: BufReader::new(reader),
                writer,
            };
            let (code, _) = client.read_reply().await;
            assert_eq!(code, 220, "server greeting");
            client
        }

        async fn command(&mut self, command: &str) -> (u16, String) {
            self.writer
                .write_all(command.as_bytes())
                .await
                .expect("write command");
            self.writer.write_all(b"\r\n").await.expect("write CRLF");
            self.read_reply().await
        }

        async fn read_reply(&mut self) -> (u16, String) {
            let mut text = String::new();
            loop {
                let mut line = String::new();
                let read = timeout(IO_TIMEOUT, self.reader.read_line(&mut line))
                    .await
                    .expect("reply timed out")
                    .expect("read reply");
                assert!(read > 0, "control connection closed unexpectedly");
                text.push_str(&line);
                if line.len() >= 4
                    && line.as_bytes()[3] == b' '
                    && line[..3].bytes().all(|byte| byte.is_ascii_digit())
                {
                    break;
                }
            }
            (text[..3].parse().unwrap(), text)
        }
    }

    async fn login(client: &mut FtpClient) {
        let (code, _) = client.command("USER admin").await;
        assert_eq!(code, 331, "USER should ask for the password");
        let (code, _) = client.command("PASS admin").await;
        assert_eq!(code, 230, "PASS should authenticate");
        let (code, _) = client.command("TYPE I").await;
        assert_eq!(code, 200, "TYPE I reply");
    }

    /// 发出 PASV 并连上通告的数据地址，返回数据流和通告的 IPv4 地址。
    /// 通配绑定时 PASV 必须通告客户端接入的具体接口地址——若回退为
    /// 0.0.0.0，握手正常但数据连接无处可去，正是本测试守护的回归。
    async fn passive_data_stream(client: &mut FtpClient) -> (TcpStream, Ipv4Addr) {
        let (code, text) = client.command("PASV").await;
        assert_eq!(code, 227, "PASV reply: {text}");
        let start = text.find('(').expect("PASV reply lacks an address tuple");
        let end = text[start..]
            .find(')')
            .map(|offset| start + offset)
            .expect("PASV reply lacks a closing parenthesis");
        let parts = text[start + 1..end]
            .split(',')
            .map(|part| part.trim().parse::<u8>().expect("PASV reply octet"))
            .collect::<Vec<_>>();
        assert_eq!(parts.len(), 6, "PASV reply must hold 6 octets: {text}");
        let host = Ipv4Addr::new(parts[0], parts[1], parts[2], parts[3]);
        assert!(
            !host.is_unspecified(),
            "PASV must not advertise the wildcard address: {text}"
        );
        let port = u16::from_be_bytes([parts[4], parts[5]]);
        let stream = timeout(IO_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .expect("passive data connect timed out")
            .expect("passive data connect failed");
        (stream, host)
    }

    async fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
        let mut data = Vec::new();
        timeout(IO_TIMEOUT, stream.read_to_end(&mut data))
            .await
            .expect("data read timed out")
            .expect("data read failed");
        data
    }

    struct TestServer {
        shutdown: watch::Sender<bool>,
        task: tokio::task::JoinHandle<Result<(), libunftp::ServerError>>,
        root: PathBuf,
        control_addr: SocketAddr,
    }

    /// 以 0.0.0.0 通配绑定启动与生产完全相同的 libunftp 配置（build_server）。
    async fn start_test_server(passive_ports: RangeInclusive<u16>) -> TestServer {
        let root = std::env::temp_dir().join(format!("xterm-ftp-e2e-{}", crate::ids::new_id()));
        std::fs::create_dir(&root).unwrap();
        // 与生产路径一致使用 canonical 形式（Windows 上为 \\?\ 前缀路径）。
        let root = std::fs::canonicalize(root).unwrap();
        let control_port = free_port();
        let auth = Arc::new(PasswordAuthenticator {
            username: "admin".to_string(),
            password: Arc::new(parking_lot::RwLock::new("admin".to_string())),
        });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let indicator = async move {
            let mut receiver = shutdown_rx;
            let _ = receiver.changed().await;
            Shutdown::new().grace_period(Duration::from_millis(100))
        };
        let server = build_server(root.clone(), auth, passive_ports, NopListener, indicator)
            .expect("build FTP server");
        let task = tokio::spawn(server.listen(format!("0.0.0.0:{control_port}")));
        let control_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, control_port));
        // listen() 内部的 bind 是异步的；轮询直到控制端口可连。
        for attempt in 0..100 {
            match TcpStream::connect(control_addr).await {
                Ok(stream) => {
                    drop(stream);
                    return TestServer {
                        shutdown: shutdown_tx,
                        task,
                        root,
                        control_addr,
                    };
                }
                Err(error) if attempt == 99 => {
                    panic!("FTP server did not start listening: {error}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        unreachable!()
    }

    impl TestServer {
        async fn stop(self) {
            let _ = self.shutdown.send(true);
            let _ = timeout(IO_TIMEOUT, self.task).await;
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 经 127.0.0.1 走完被动下载/上传和主动列表：0.0.0.0 绑定下握手成功之后
    /// 数据通道必须同样可用。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wildcard_bind_serves_passive_and_active_transfers() {
        let passive_start = free_port();
        let server = start_test_server(passive_start..=passive_start + 4).await;
        let payload: Vec<u8> = (0..4096u32).map(|value| (value % 251) as u8).collect();
        std::fs::write(server.root.join("seed.bin"), &payload).unwrap();

        let mut client = FtpClient::connect(server.control_addr).await;
        login(&mut client).await;

        let (mut data, host) = passive_data_stream(&mut client).await;
        assert!(host.is_loopback(), "loopback session advertises {host}");
        let (code, _) = client.command("RETR seed.bin").await;
        assert_eq!(code, 150, "RETR preliminary reply");
        let received = read_to_end(&mut data).await;
        let (code, _) = client.read_reply().await;
        assert_eq!(code, 226, "RETR completion reply");
        assert_eq!(received, payload);

        let (mut data, _) = passive_data_stream(&mut client).await;
        let (code, _) = client.command("STOR upload.bin").await;
        assert_eq!(code, 150, "STOR preliminary reply");
        data.write_all(b"hello ftp upload").await.unwrap();
        data.shutdown().await.unwrap();
        let (code, _) = client.read_reply().await;
        assert_eq!(code, 226, "STOR completion reply");
        assert_eq!(
            std::fs::read(server.root.join("upload.bin")).unwrap(),
            b"hello ftp upload"
        );

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (code, _) = client
            .command(&format!("PORT 127,0,0,1,{},{}", port >> 8, port & 0xFF))
            .await;
        assert_eq!(code, 200, "PORT reply");
        let accept = tokio::spawn(async move {
            let (mut stream, _) = timeout(IO_TIMEOUT, listener.accept())
                .await
                .expect("active data connect timed out")
                .expect("active data accept failed");
            read_to_end(&mut stream).await
        });
        let (code, _) = client.command("NLST").await;
        assert_eq!(code, 150, "NLST preliminary reply");
        let listing = String::from_utf8(accept.await.unwrap()).unwrap();
        let (code, _) = client.read_reply().await;
        assert_eq!(code, 226, "NLST completion reply");
        assert!(listing.contains("seed.bin"), "listing: {listing}");
        assert!(listing.contains("upload.bin"), "listing: {listing}");

        server.stop().await;
    }

    /// 多网卡主机上，经非回环网卡接入时 PASV 必须通告该网卡的地址，
    /// 而不是 0.0.0.0 或其它网卡的地址。无可用网卡时跳过。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wildcard_bind_advertises_the_per_connection_interface_ip() {
        let interface_ip = if_addrs::get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|interface| match interface.ip() {
                IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => Some(ip),
                _ => None,
            })
            .next();
        let Some(interface_ip) = interface_ip else {
            eprintln!("no usable non-loopback IPv4 interface; skipping");
            return;
        };
        let passive_start = free_port();
        let server = start_test_server(passive_start..=passive_start + 4).await;
        std::fs::write(server.root.join("nic.txt"), b"via interface").unwrap();

        let control_addr = SocketAddr::new(IpAddr::V4(interface_ip), server.control_addr.port());
        let mut client = FtpClient::connect(control_addr).await;
        login(&mut client).await;
        let (mut data, host) = passive_data_stream(&mut client).await;
        assert_eq!(
            host, interface_ip,
            "PASV must advertise the interface the client connected through"
        );
        let (code, _) = client.command("RETR nic.txt").await;
        assert_eq!(code, 150, "RETR preliminary reply");
        let received = read_to_end(&mut data).await;
        let (code, _) = client.read_reply().await;
        assert_eq!(code, 226, "RETR completion reply");
        assert_eq!(received, b"via interface");

        server.stop().await;
    }
}
