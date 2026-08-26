//! 服务监听启动的统一入口：代理与各文件服务协议都先以当前用户直接 bind，
//! 然后放行防火墙。Linux 下服务端口是特权端口（<1024）时，非 root 直接
//! bind 必然 EACCES，根本走不到防火墙的提权分支——这是文件服务不弹提权
//! 窗口的根因。此时改为经 pkexec 启动同二进制的 `--bind-elevated` helper，
//! 由 helper 以 root 一次性完成防火墙放行 + socket 绑定，再通过 Unix
//! domain socket 用 SCM_RIGHTS 把绑定好的 fd 回传给主进程，因此只需要
//! 一次提权授权。helper 只允许绑定 <1024 端口，避免被用来抢占任意高端口。

use std::net::SocketAddr;

use crate::{
    firewall::{self, FirewallCommandError, FirewallProtocol},
    logging,
};

/// 一个服务的防火墙放行描述：`prefix` 同时用作 iptables 链名来源与规则注释。
pub(crate) struct ServiceRule {
    pub(crate) prefix: &'static str,
    pub(crate) action: &'static str,
    pub(crate) protocol: FirewallProtocol,
    pub(crate) ports: Vec<u16>,
    /// TFTP 的传输 ID 是临时端口，除服务端口外还需要整段 UDP 放行规则。
    pub(crate) all_udp: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct BindSpec {
    pub(crate) addr: SocketAddr,
    pub(crate) protocol: FirewallProtocol,
    /// 可选绑定点（如 TFTP 的每网卡监听）：单个失败只跳过不报错。
    pub(crate) optional: bool,
}

impl BindSpec {
    pub(crate) fn tcp(addr: SocketAddr) -> Self {
        Self {
            addr,
            protocol: FirewallProtocol::Tcp,
            optional: false,
        }
    }

    pub(crate) fn udp(addr: SocketAddr, optional: bool) -> Self {
        Self {
            addr,
            protocol: FirewallProtocol::Udp,
            optional,
        }
    }
}

pub(crate) enum BoundSocket {
    Tcp(std::net::TcpListener),
    Udp(std::net::UdpSocket),
}

impl BoundSocket {
    fn set_nonblocking(&self) -> std::io::Result<()> {
        match self {
            Self::Tcp(listener) => listener.set_nonblocking(true),
            Self::Udp(socket) => socket.set_nonblocking(true),
        }
    }

    #[cfg(target_os = "linux")]
    fn protocol(&self) -> FirewallProtocol {
        match self {
            Self::Tcp(_) => FirewallProtocol::Tcp,
            Self::Udp(_) => FirewallProtocol::Udp,
        }
    }

    #[cfg(target_os = "linux")]
    fn raw_fd(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        match self {
            Self::Tcp(listener) => listener.as_raw_fd(),
            Self::Udp(socket) => socket.as_raw_fd(),
        }
    }
}

struct BindFailure {
    message: String,
    permission_denied: bool,
}

/// 先尝试直接 bind 全部监听点，成功后放行防火墙；Linux 下若因权限不足
/// 失败且目标都是特权端口，则走 pkexec helper 一并完成放行与绑定。
pub(crate) async fn bind_service_sockets(
    rule: ServiceRule,
    binds: Vec<BindSpec>,
    fallback: Option<BindSpec>,
) -> Result<Vec<BoundSocket>, String> {
    match bind_direct(&binds, fallback.as_ref()) {
        Ok(sockets) => match allow_rule(&rule).await {
            Ok(()) => Ok(sockets),
            #[cfg(target_os = "linux")]
            Err(error) if error.requires_elevation() => {
                // 端口已直接绑定成功（高端口），但防火墙放行仍需 root。这里改走
                // bind helper 的「仅防火墙」模式（空 binds），避免触发已失效的
                // `--firewall-elevated` 路径，且保证每次开启只提权一次。
                bind_elevated(&rule, &[], None).await?;
                Ok(sockets)
            }
            Err(error) => Err(error.user_message),
        },
        Err(failure) => {
            #[cfg(target_os = "linux")]
            if failure.permission_denied
                && binds
                    .iter()
                    .chain(fallback.as_ref())
                    .all(|spec| spec.addr.port() < 1024)
            {
                return bind_elevated(&rule, &binds, fallback.as_ref()).await;
            }
            #[cfg(not(target_os = "linux"))]
            let _ = failure.permission_denied;
            Err(failure.message)
        }
    }
}

/// 直接 bind：非可选点失败立即整体失败；可选点失败跳过并记录；全部可选点
/// 失败时退回 `fallback`（如 TFTP 从每网卡监听退回通配地址）。
fn bind_direct(
    binds: &[BindSpec],
    fallback: Option<&BindSpec>,
) -> Result<Vec<BoundSocket>, BindFailure> {
    let mut sockets = Vec::new();
    for spec in binds {
        match bind_one(spec) {
            Ok(socket) => sockets.push(socket),
            Err(error) if spec.optional => {
                logging::event("service.listener", "service.listener.bind_failed")
                    .field("bind_addr", spec.addr)
                    .field("error", error.to_string())
                    .warn();
            }
            Err(error) => {
                return Err(BindFailure {
                    permission_denied: error.kind() == std::io::ErrorKind::PermissionDenied,
                    message: format_bind_error(spec.addr, &error),
                });
            }
        }
    }
    if !sockets.is_empty() {
        return Ok(sockets);
    }
    match fallback {
        Some(spec) => bind_one(spec)
            .map(|socket| vec![socket])
            .map_err(|error| BindFailure {
                permission_denied: error.kind() == std::io::ErrorKind::PermissionDenied,
                message: format_bind_error(spec.addr, &error),
            }),
        None => Err(BindFailure {
            permission_denied: false,
            message: "No service bind address was available.".to_string(),
        }),
    }
}

fn bind_one(spec: &BindSpec) -> std::io::Result<BoundSocket> {
    let socket = match spec.protocol {
        FirewallProtocol::Tcp => std::net::TcpListener::bind(spec.addr).map(BoundSocket::Tcp),
        FirewallProtocol::Udp => std::net::UdpSocket::bind(spec.addr).map(BoundSocket::Udp),
    }?;
    // 主进程随后会把这些 fd 转交 tokio，必须是非阻塞的。
    socket.set_nonblocking()?;
    Ok(socket)
}

fn format_bind_error(addr: SocketAddr, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("Port {} is already in use on {}.", addr.port(), addr.ip())
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("The service could not bind to {addr} because access was denied.")
        }
        _ => format!("Failed to bind the service listener to {addr}: {error}"),
    }
}

async fn allow_rule(rule: &ServiceRule) -> Result<(), FirewallCommandError> {
    #[cfg(target_os = "linux")]
    {
        return allow_rule_linux(rule).await;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let result = if rule.all_udp {
            firewall::allow_service_port_and_all_udp_ports_for_current_app(
                rule.prefix,
                rule.action,
                rule.ports[0],
            )
            .await
        } else {
            firewall::allow_service_ports(
                rule.prefix,
                rule.action,
                rule.ports.clone(),
                rule.protocol,
            )
            .await
        };
        result.inspect_err(|error| {
            logging::event("firewall", "firewall.allow.failed")
                .field("prefix", rule.prefix)
                .field("detail", &error.detail)
                .warn();
        })
    }
}

/// Linux 下只做「非提权」的防火墙放行探测：真正需要 root 时由
/// [`bind_service_sockets`] 改走 bind helper（`--bind-elevated`）完成，避免依赖
/// 桌面上失效的 `--firewall-elevated` 路径。这里直接调用底层实现，拿到原始
/// 的权限错误供上层判断是否提权。
#[cfg(target_os = "linux")]
async fn allow_rule_linux(rule: &ServiceRule) -> Result<(), FirewallCommandError> {
    let prefix = rule.prefix;
    let action = rule.action;
    let ports = rule.ports.clone();
    let first_port = ports[0];
    let protocol = rule.protocol;
    let all_udp = rule.all_udp;
    let result = tokio::task::spawn_blocking(move || {
        if all_udp {
            firewall::allow_port_impl(prefix, first_port, protocol)?;
            firewall::allow_all_udp_ports_for_current_app_impl(prefix)
        } else {
            ports
                .iter()
                .try_for_each(|port| firewall::allow_port_impl(prefix, *port, protocol))
        }
    })
    .await
    .map_err(|error| {
        FirewallCommandError::new(
            "The firewall operation did not complete.",
            error.to_string(),
        )
    })?;
    match result {
        Ok(()) => {
            logging::event("firewall", action)
                .field("port", first_port)
                .info();
            Ok(())
        }
        Err(error) => {
            logging::event("firewall", "firewall.allow.failed")
                .field("prefix", prefix)
                .field("detail", &error.detail)
                .warn();
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
const ELEVATED_BIND_FLAG: &str = "--bind-elevated";

#[cfg(target_os = "linux")]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedBindRequest {
    prefix: String,
    firewall_ports: Vec<u16>,
    firewall_protocol: FirewallProtocol,
    all_udp: bool,
    binds: Vec<BindSpec>,
    fallback: Option<BindSpec>,
    reply: ElevatedBindReply,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct ElevatedBindReply {
    path: std::path::PathBuf,
    nonce: String,
}

#[cfg(target_os = "linux")]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedBindResponse {
    nonce: String,
    ok: bool,
    user_message: Option<String>,
    detail: Option<String>,
    /// 每个成功绑定的 socket 的协议，与随后通过 SCM_RIGHTS 传入的 fd 一一对应。
    protocols: Vec<FirewallProtocol>,
}

/// 提权 helper 的回复 Unix socket 读写超时，与防火墙提权共用同一预算。
#[cfg(target_os = "linux")]
const ELEVATED_IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(target_os = "linux")]
fn encode_elevated_token(request: &ElevatedBindRequest) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    URL_SAFE_NO_PAD.encode(serde_json::to_vec(request).unwrap_or_default())
}

#[cfg(target_os = "linux")]
fn decode_elevated_token(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Option<ElevatedBindRequest> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let first = args.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(first.to_str()?).ok()?;
    serde_json::from_slice(&decoded).ok()
}

#[cfg(target_os = "linux")]
async fn bind_elevated(
    rule: &ServiceRule,
    binds: &[BindSpec],
    fallback: Option<&BindSpec>,
) -> Result<Vec<BoundSocket>, String> {
    let request = ElevatedBindRequest {
        prefix: rule.prefix.to_string(),
        firewall_ports: rule.ports.clone(),
        firewall_protocol: rule.protocol,
        all_udp: rule.all_udp,
        binds: binds.to_vec(),
        fallback: fallback.cloned(),
        reply: ElevatedBindReply {
            path: std::env::temp_dir().join(format!(
                "xterm-bind-{}-{:016x}.sock",
                std::process::id(),
                rand::random::<u64>()
            )),
            nonce: format!("{}:{:016x}", std::process::id(), rand::random::<u64>()),
        },
    };
    // pkexec 会阻塞等待用户授权，放进 blocking 线程池。
    match tokio::task::spawn_blocking(move || run_elevated_bind(request)).await {
        Ok(Ok(sockets)) => Ok(sockets),
        Ok(Err(error)) => {
            logging::event("service.listener", "service.listener.elevated_failed")
                .field("prefix", rule.prefix)
                .field("detail", &error.detail)
                .warn();
            Err(error.user_message)
        }
        Err(join_error) => Err(format!("The service listener did not start: {join_error}")),
    }
}

/// 父进程侧：监听回复 Unix socket，启动 pkexec helper，收回结果与 fd。
#[cfg(target_os = "linux")]
fn run_elevated_bind(
    request: ElevatedBindRequest,
) -> Result<Vec<BoundSocket>, FirewallCommandError> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&request.reply.path).map_err(|error| {
        FirewallCommandError::new(
            "Unable to prepare the service listener approval channel.",
            error.to_string(),
        )
    })?;
    // helper 连接后该路径就没有用了；无论成败都删掉，避免 /tmp 残留。
    let _cleanup = SocketFileCleanup(request.reply.path.clone());

    let exe_path = firewall::stage_elevated_executable().map_err(|error| {
        FirewallCommandError::new(
            "Unable to prepare the administrator approval helper.",
            error.to_string(),
        )
    })?;
    let cleanup_path = exe_path.clone();
    let mut command = std::process::Command::new(exe_path);
    command
        .arg(ELEVATED_BIND_FLAG)
        .arg(encode_elevated_token(&request));
    let output = elevated_command::Command::new(command)
        .output()
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to request administrator approval for the service listener.",
                error.to_string(),
            )
        })?;

    let _ = std::fs::remove_file(cleanup_path);

    // helper 约定：成功 exit 0、校验失败 exit 2、执行失败但已回报 exit 1；
    // 其余状态（126/127）是 pkexec 拒绝或未能启动。
    if !matches!(output.status.code(), Some(0) | Some(1)) {
        return Err(FirewallCommandError::new(
            "Administrator approval was denied, so the service listener was not started.",
            format!("elevated command launch status {}", output.status),
        ));
    }

    let (mut stream, _) = accept_with_timeout(&listener)?;
    let response = read_bind_response(&mut stream)?;
    if response.nonce != request.reply.nonce {
        return Err(FirewallCommandError::new(
            "The service listener approval result failed validation.",
            "response nonce did not match the request",
        ));
    }
    if !response.ok {
        return Err(FirewallCommandError::new(
            response.user_message.unwrap_or_else(|| {
                "Unable to start the service listener after administrator approval.".to_string()
            }),
            response.detail.unwrap_or_else(|| {
                "elevated bind helper did not return an error detail".to_string()
            }),
        ));
    }

    let mut sockets = Vec::with_capacity(response.protocols.len());
    for protocol in response.protocols {
        use std::os::unix::io::FromRawFd;

        let fd = recv_fd(&stream)?;
        // SAFETY: fd 来自同一 helper 经 SCM_RIGHTS 传入，所有权随之移交；
        // 每个 fd 只在此封装一次。
        let socket = unsafe {
            match protocol {
                FirewallProtocol::Tcp => BoundSocket::Tcp(std::net::TcpListener::from_raw_fd(fd)),
                FirewallProtocol::Udp => BoundSocket::Udp(std::net::UdpSocket::from_raw_fd(fd)),
            }
        };
        socket.set_nonblocking().map_err(|error| {
            FirewallCommandError::new(
                "Unable to prepare the service listener socket.",
                error.to_string(),
            )
        })?;
        sockets.push(socket);
    }
    Ok(sockets)
}

#[cfg(target_os = "linux")]
struct SocketFileCleanup(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for SocketFileCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(target_os = "linux")]
fn accept_with_timeout(
    listener: &std::os::unix::net::UnixListener,
) -> Result<
    (
        std::os::unix::net::UnixStream,
        std::os::unix::net::SocketAddr,
    ),
    FirewallCommandError,
> {
    listener.set_nonblocking(true).map_err(|error| {
        FirewallCommandError::new(
            "Unable to wait for the service listener approval result.",
            error.to_string(),
        )
    })?;
    let deadline = std::time::Instant::now() + ELEVATED_IPC_TIMEOUT;
    loop {
        match listener.accept() {
            Ok(result) => return Ok(result),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(FirewallCommandError::new(
                        "The service listener approval request did not return a result.",
                        "timed out waiting for elevated bind helper response",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => {
                return Err(FirewallCommandError::new(
                    "Unable to wait for the service listener approval result.",
                    error.to_string(),
                ));
            }
        }
    }
}

/// 回复行必须逐字节读到换行：其后的 fd 由 recvmsg 携带，不能用 BufReader
/// 预读，否则 fd 附件与字节流会错位。
#[cfg(target_os = "linux")]
fn read_bind_response(
    stream: &mut std::os::unix::net::UnixStream,
) -> Result<ElevatedBindResponse, FirewallCommandError> {
    use std::io::Read;

    stream
        .set_read_timeout(Some(ELEVATED_IPC_TIMEOUT))
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to wait for the service listener approval result.",
                error.to_string(),
            )
        })?;
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let size = stream.read(&mut byte).map_err(|error| {
            FirewallCommandError::new(
                "Unable to read the service listener approval result.",
                error.to_string(),
            )
        })?;
        if size == 0 {
            return Err(FirewallCommandError::new(
                "The service listener approval result was invalid.",
                "elevated bind helper closed the reply channel",
            ));
        }
        if byte[0] == b'\n' {
            break;
        }
        buffer.push(byte[0]);
        if buffer.len() > 64 * 1024 {
            return Err(FirewallCommandError::new(
                "The service listener approval result was invalid.",
                "response line exceeded 64 KiB",
            ));
        }
    }
    serde_json::from_slice(&buffer).map_err(|error| {
        FirewallCommandError::new(
            "The service listener approval result was invalid.",
            error.to_string(),
        )
    })
}

/// 经 SCM_RIGHTS 发送一个 fd，附 1 字节载荷（stream 套接字要求每条消息
/// 至少 1 字节才能携带控制信息）。
#[cfg(target_os = "linux")]
fn send_fd(
    stream: &std::os::unix::net::UnixStream,
    fd: std::os::unix::io::RawFd,
) -> Result<(), FirewallCommandError> {
    use std::os::unix::io::AsRawFd;

    let payload = [0u8; 1];
    let iov = libc::iovec {
        iov_base: payload.as_ptr() as *mut libc::c_void,
        iov_len: 1,
    };
    let mut control =
        vec![0u8; unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as _) } as usize];
    // SAFETY: msg 指向有效的 iov 与 control 缓冲区，control 容量按
    // CMSG_SPACE 分配，写入不超过 CMSG_LEN(sizeof c_int)。
    let result = unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &iov as *const libc::iovec as *mut libc::iovec;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr() as *mut libc::c_void;
        message.msg_controllen = control.len() as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(FirewallCommandError::new(
                "Unable to hand the service listener socket back.",
                "CMSG_FIRSTHDR returned null while sending",
            ));
        }
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as _) as _;
        std::ptr::write(libc::CMSG_DATA(header) as *mut libc::c_int, fd);
        libc::sendmsg(stream.as_raw_fd(), &message, 0)
    };
    if result < 0 {
        return Err(FirewallCommandError::new(
            "Unable to hand the service listener socket back.",
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn recv_fd(
    stream: &std::os::unix::net::UnixStream,
) -> Result<std::os::unix::io::RawFd, FirewallCommandError> {
    use std::os::unix::io::AsRawFd;

    let mut payload = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr() as *mut libc::c_void,
        iov_len: 1,
    };
    let mut control =
        vec![0u8; unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as _) } as usize];
    // SAFETY: msg_iov/msg_control 分别指向有效的 iov 与 control 缓冲区，
    // 容量按 CMSG_SPACE 分配，recvmsg 写入不越界。
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr() as *mut libc::c_void;
    message.msg_controllen = control.len() as _;
    // SAFETY: message 及其引用的缓冲区均有效。
    let received = unsafe { libc::recvmsg(stream.as_raw_fd(), &mut message, 0) };
    if received <= 0 {
        return Err(FirewallCommandError::new(
            "Unable to receive the service listener socket.",
            std::io::Error::last_os_error().to_string(),
        ));
    }
    // SAFETY: message 已被 recvmsg 填充，header 落在 control 缓冲区内。
    let header = unsafe { libc::CMSG_FIRSTHDR(&message) };
    if header.is_null()
    // SAFETY: header 非空时指向 recvmsg 写入的控制消息头。
        || unsafe { (*header).cmsg_level != libc::SOL_SOCKET || (*header).cmsg_type != libc::SCM_RIGHTS }
    {
        return Err(FirewallCommandError::new(
            "Unable to receive the service listener socket.",
            "reply message did not carry a file descriptor",
        ));
    }
    // SAFETY: header 已通过校验，CMSG_DATA 区域含一个 c_int fd。
    Ok(unsafe { std::ptr::read(libc::CMSG_DATA(header) as *const libc::c_int) })
}

/// helper 入口（root）：放行防火墙、绑定特权端口，把 fd 回传父进程后退出。
/// 返回 true 表示当前进程是 helper，调用方应立即返回，不再启动应用。
#[cfg(target_os = "linux")]
pub(crate) fn handle_elevated_bind_helper() -> bool {
    use std::{ffi::OsStr, os::unix::net::UnixStream, process};

    let mut args = std::env::args_os();
    let _ = args.next();
    let Some(flag) = args.next() else {
        return false;
    };
    if flag != OsStr::new(ELEVATED_BIND_FLAG) {
        return false;
    }

    let Some(request) = decode_elevated_token(args).and_then(validate_elevated_bind_request) else {
        process::exit(2);
    };

    match run_bind_helper(&request) {
        Ok(sockets) => {
            let response = ElevatedBindResponse {
                nonce: request.reply.nonce.clone(),
                ok: true,
                user_message: None,
                detail: None,
                protocols: sockets.iter().map(BoundSocket::protocol).collect(),
            };
            if let Ok(mut stream) = UnixStream::connect(&request.reply.path) {
                let _ = write_response_line(&mut stream, &response);
                for socket in &sockets {
                    let _ = send_fd(&stream, socket.raw_fd());
                }
            }
            process::exit(0);
        }
        Err(error) => {
            let response = ElevatedBindResponse {
                nonce: request.reply.nonce.clone(),
                ok: false,
                user_message: Some(error.user_message.clone()),
                detail: Some(error.detail.clone()),
                protocols: Vec::new(),
            };
            if let Ok(mut stream) = UnixStream::connect(&request.reply.path) {
                let _ = write_response_line(&mut stream, &response);
            }
            process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn run_bind_helper(
    request: &ElevatedBindRequest,
) -> Result<Vec<BoundSocket>, FirewallCommandError> {
    for port in &request.firewall_ports {
        firewall::allow_port_impl(&request.prefix, *port, request.firewall_protocol)?;
    }
    if request.all_udp {
        firewall::allow_all_udp_ports_for_current_app_impl(&request.prefix)?;
    }

    let mut sockets = Vec::new();
    for spec in &request.binds {
        match bind_one(spec) {
            Ok(socket) => sockets.push(socket),
            Err(_) if spec.optional => {}
            Err(error) => {
                return Err(FirewallCommandError::new(
                    format!(
                        "The service could not bind to {} after administrator approval.",
                        spec.addr
                    ),
                    error.to_string(),
                ));
            }
        }
    }
    if sockets.is_empty() {
        // 「仅防火墙」模式：无任何绑定点，只放行防火墙，空 socket 列表即成功。
        if request.binds.is_empty() && request.fallback.is_none() {
            return Ok(sockets);
        }
        return match &request.fallback {
            Some(spec) => bind_one(spec).map(|socket| vec![socket]).map_err(|error| {
                FirewallCommandError::new(
                    format!(
                        "The service could not bind to {} after administrator approval.",
                        spec.addr
                    ),
                    error.to_string(),
                )
            }),
            None => Err(FirewallCommandError::new(
                "The service could not bind to any interface after administrator approval.",
                "all optional bind addresses failed",
            )),
        };
    }
    Ok(sockets)
}

/// helper 以 root 运行，必须收紧请求：规则前缀限定 XTerm 命名空间，绑定
/// 目标限定特权端口，回复路径限定绝对路径下的 xterm-bind-* 命名，防止
/// 被其他本地进程借道占用任意端口或改写无关防火墙规则。
#[cfg(target_os = "linux")]
fn validate_elevated_bind_request(request: ElevatedBindRequest) -> Option<ElevatedBindRequest> {
    let privileged = |spec: &BindSpec| spec.addr.port() > 0 && spec.addr.port() < 1024;
    if !request.prefix.starts_with("XTerm ") {
        return None;
    }
    if request.firewall_ports.is_empty() || request.firewall_ports.contains(&0) {
        return None;
    }
    if request.binds.len() > 32
        || !request.binds.iter().all(privileged)
        || request
            .fallback
            .as_ref()
            .is_some_and(|spec| !privileged(spec))
    {
        return None;
    }
    let reply_name = request
        .reply
        .path
        .file_name()
        .and_then(|name| name.to_str());
    if !request.reply.path.is_absolute()
        || !reply_name.is_some_and(|name| name.starts_with("xterm-bind-"))
        || request.reply.nonce.is_empty()
    {
        return None;
    }
    Some(request)
}

#[cfg(target_os = "linux")]
fn write_response_line(
    stream: &mut std::os::unix::net::UnixStream,
    response: &ElevatedBindResponse,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut payload = serde_json::to_vec(response)?;
    payload.push(b'\n');
    stream.write_all(&payload)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        os::unix::{
            io::{AsRawFd, FromRawFd},
            net::UnixStream,
        },
    };

    use super::*;

    #[test]
    fn fd_passing_round_trips_tcp_listener() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        send_fd(&sender, listener.as_raw_fd()).unwrap();
        let fd = recv_fd(&receiver).unwrap();
        // SAFETY: fd 由 send_fd 经 SCM_RIGHTS 复制而来，此处独占封装一次。
        let received = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        assert_eq!(received.local_addr().unwrap(), addr);
    }

    fn request(prefix: &str, port: u16, reply_path: &str) -> ElevatedBindRequest {
        ElevatedBindRequest {
            prefix: prefix.to_string(),
            firewall_ports: vec![port],
            firewall_protocol: FirewallProtocol::Tcp,
            all_udp: false,
            binds: vec![BindSpec::tcp(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
            ))],
            fallback: None,
            reply: ElevatedBindReply {
                path: reply_path.into(),
                nonce: "nonce".to_string(),
            },
        }
    }

    #[test]
    fn helper_validation_accepts_privileged_xterm_bind() {
        assert!(validate_elevated_bind_request(request(
            "XTerm SFTP",
            22,
            "/tmp/xterm-bind-a.sock"
        ))
        .is_some());
    }

    #[test]
    fn helper_validation_accepts_firewall_only_request() {
        // 高端口服务（如代理）只提权放行防火墙、不绑定端口：空 binds + 无
        // fallback 的「仅防火墙」请求必须被接受。
        let request = ElevatedBindRequest {
            prefix: "XTerm Proxy".to_string(),
            firewall_ports: vec![3128],
            firewall_protocol: FirewallProtocol::Tcp,
            all_udp: false,
            binds: Vec::new(),
            fallback: None,
            reply: ElevatedBindReply {
                path: "/tmp/xterm-bind-firewall.sock".into(),
                nonce: "nonce".to_string(),
            },
        };
        assert!(validate_elevated_bind_request(request).is_some());
    }

    #[test]
    fn helper_validation_rejects_foreign_prefix_and_high_port() {
        assert!(validate_elevated_bind_request(request(
            "Other Service",
            22,
            "/tmp/xterm-bind-a.sock"
        ))
        .is_none());
        // 高端口必须直接绑定，不允许借道 root helper 抢占。
        assert!(validate_elevated_bind_request(request(
            "XTerm Proxy",
            3128,
            "/tmp/xterm-bind-a.sock"
        ))
        .is_none());
    }

    #[test]
    fn helper_validation_rejects_suspicious_reply_path() {
        assert!(
            validate_elevated_bind_request(request("XTerm SFTP", 22, "xterm-bind-a.sock"))
                .is_none()
        );
        assert!(
            validate_elevated_bind_request(request("XTerm SFTP", 22, "/tmp/evil.sock")).is_none()
        );
    }
}
