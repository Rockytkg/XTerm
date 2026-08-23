use crate::logging;

#[cfg(any(windows, unix))]
use std::ffi::OsStr;

use std::{
    io::{BufRead, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    process::Command,
    time::Duration,
};

#[cfg(target_os = "linux")]
pub(crate) fn stage_elevated_executable() -> Result<std::path::PathBuf, std::io::Error> {
    let source = std::env::current_exe()?;
    let target = std::env::temp_dir().join(format!(
        "xterm-elevated-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::copy(source, &target)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))?;
    Ok(target)
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FirewallProtocol {
    Tcp,
    Udp,
}

impl FirewallProtocol {
    // Only the Windows rule description uses the wire protocol name; the
    // Linux path formats its own iptables rules and macOS uses pfctl protos.
    #[cfg(windows)]
    fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
}

// Windows and macOS name their rules "{prefix} {port}"; Linux derives chain
// names from the prefix instead and never calls this.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn rule_name(prefix: &str, port: u16) -> String {
    format!("{prefix} {port}")
}

#[derive(Debug)]
pub(crate) struct FirewallCommandError {
    pub(crate) user_message: String,
    pub(crate) detail: String,
}

impl FirewallCommandError {
    fn new(user_message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            user_message: user_message.into(),
            detail: detail.into(),
        }
    }
}

struct FirewallTaskRequest {
    prefix: &'static str,
    action: &'static str,
    operation: FirewallOperation,
    protocol: FirewallProtocol,
    ports: Vec<u16>,
    all_ports: bool,
}

impl FirewallTaskRequest {
    fn new(
        prefix: &'static str,
        action: &'static str,
        operation: FirewallOperation,
        protocol: FirewallProtocol,
        ports: Vec<u16>,
        all_ports: bool,
    ) -> Result<Self, FirewallCommandError> {
        if ports.is_empty() {
            return Err(FirewallCommandError::new(
                "The firewall port list cannot be empty.",
                "empty port list",
            ));
        }
        Ok(Self {
            prefix,
            action,
            operation,
            protocol,
            ports,
            all_ports,
        })
    }
}

pub(crate) async fn allow_service_ports(
    prefix: &'static str,
    action: &'static str,
    ports: Vec<u16>,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let task_ports = ports.clone();
    run_firewall_task(
        FirewallTaskRequest::new(
            prefix,
            action,
            FirewallOperation::Allow,
            protocol,
            ports,
            false,
        )?,
        move || allow_ports_impl(prefix, &task_ports, protocol),
    )
    .await
}

pub(crate) async fn allow_service_port_and_all_udp_ports_for_current_app(
    prefix: &'static str,
    action: &'static str,
    port: u16,
) -> Result<(), FirewallCommandError> {
    // Windows / Linux / macOS all need the service port plus a blanket UDP
    // allow rule: TFTP picks ephemeral transfer IDs, so the negotiated data
    // ports cannot be enumerated up front.
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let task_prefix = prefix;
        return run_firewall_task(
            FirewallTaskRequest::new(
                prefix,
                action,
                FirewallOperation::Allow,
                FirewallProtocol::Udp,
                vec![port],
                true,
            )?,
            move || {
                allow_port_impl(task_prefix, port, FirewallProtocol::Udp)?;
                allow_all_udp_ports_for_current_app_impl(task_prefix)
            },
        )
        .await;
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        allow_service_ports(prefix, action, vec![port], FirewallProtocol::Udp).await
    }
}

pub(crate) async fn remove_service_port_rule(
    prefix: &'static str,
    action: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_service_ports_rule(prefix, action, vec![port], protocol).await
}

pub(crate) async fn remove_service_ports_rule(
    prefix: &'static str,
    action: &'static str,
    ports: Vec<u16>,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let task_ports = ports.clone();
    run_firewall_task(
        FirewallTaskRequest::new(
            prefix,
            action,
            FirewallOperation::Remove,
            protocol,
            ports,
            false,
        )?,
        move || remove_ports_rule_impl(prefix, &task_ports, protocol),
    )
    .await
}

pub(crate) async fn remove_service_port_and_all_udp_ports_for_current_app_rule(
    prefix: &'static str,
    action: &'static str,
    port: u16,
) -> Result<(), FirewallCommandError> {
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let task_prefix = prefix;
        return run_firewall_task(
            FirewallTaskRequest::new(
                prefix,
                action,
                FirewallOperation::Remove,
                FirewallProtocol::Udp,
                vec![port],
                true,
            )?,
            move || {
                remove_all_udp_ports_for_current_app_rule_impl(task_prefix)?;
                remove_port_rule_impl(task_prefix, port, FirewallProtocol::Udp)
            },
        )
        .await;
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        remove_service_port_rule(prefix, action, port, FirewallProtocol::Udp).await
    }
}

async fn run_firewall_task<F>(
    request: FirewallTaskRequest,
    task: F,
) -> Result<(), FirewallCommandError>
where
    F: FnOnce() -> Result<(), FirewallCommandError> + Send + 'static,
{
    match tokio::task::spawn_blocking(task).await {
        Ok(Ok(())) => {
            logging::event("firewall", request.action)
                .field("port", request.ports[0])
                .info();
            Ok(())
        }
        Ok(Err(error)) if firewall_error_requires_elevation(&error.detail) => {
            run_elevated_firewall_task(
                request.prefix,
                request.ports,
                request.operation,
                request.protocol,
                request.all_ports,
            )
        }
        Ok(Err(error)) => Err(error),
        Err(error) => Err(FirewallCommandError::new(
            "The firewall operation did not complete.",
            error.to_string(),
        )),
    }
}

fn firewall_error_requires_elevation(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("0x80070005")
        || lower.contains("access is denied")
        || lower.contains("e_accessdenied")
        || lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("not permitted")
        || lower.contains("must be root")
        || lower.contains("authorization failed")
}

fn allow_ports_impl(
    prefix: &'static str,
    ports: &[u16],
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    ports
        .iter()
        .try_for_each(|port| allow_port_impl(prefix, *port, protocol))
}

fn remove_ports_rule_impl(
    prefix: &'static str,
    ports: &[u16],
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    ports
        .iter()
        .try_for_each(|port| remove_port_rule_impl(prefix, *port, protocol))
}

#[cfg(any(windows, unix))]
const ELEVATED_FIREWALL_FLAG: &str = "--firewall-elevated";

#[cfg(any(windows, unix))]
const ELEVATED_FIREWALL_IPC_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(any(windows, unix))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum FirewallOperation {
    Allow,
    Remove,
}

#[cfg(any(windows, unix))]
impl FirewallOperation {
    fn user_action(self) -> &'static str {
        match self {
            Self::Allow => "apply",
            Self::Remove => "remove",
        }
    }
}

#[cfg(any(windows, unix))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirewallRuleSpec {
    prefix: String,
    ports: Vec<u16>,
    protocol: FirewallProtocol,
    all_ports: bool,
}

#[cfg(any(windows, unix))]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedFirewallRequest {
    operation: FirewallOperation,
    rule: FirewallRuleSpec,
    reply: ElevatedFirewallReplyEndpoint,
}

#[cfg(any(windows, unix))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedFirewallReplyEndpoint {
    host: IpAddr,
    port: u16,
    nonce: String,
}

#[cfg(any(windows, unix))]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElevatedFirewallResponse {
    nonce: String,
    ok: bool,
    user_message: Option<String>,
    detail: Option<String>,
}

#[cfg(any(windows, unix))]
pub(crate) fn handle_elevated_helper() -> bool {
    use std::process;

    let mut args = std::env::args_os();
    let _ = args.next();

    let Some(flag) = args.next() else {
        return false;
    };
    if flag != OsStr::new(ELEVATED_FIREWALL_FLAG) {
        return false;
    }

    let Some(request) = parse_elevated_request(args).and_then(validate_elevated_request) else {
        process::exit(2);
    };

    let result = match request.operation {
        FirewallOperation::Allow if request.rule.all_ports => request
            .rule
            .ports
            .iter()
            .try_for_each(|port| {
                allow_port_direct(&request.rule.prefix, *port, request.rule.protocol)
            })
            .and_then(|_| allow_all_udp_ports_for_current_app_direct(&request.rule.prefix)),
        FirewallOperation::Allow => request.rule.ports.iter().try_for_each(|port| {
            allow_port_direct(&request.rule.prefix, *port, request.rule.protocol)
        }),
        FirewallOperation::Remove if request.rule.all_ports => request
            .rule
            .ports
            .iter()
            .try_for_each(|port| {
                remove_port_rule_direct(&request.rule.prefix, *port, request.rule.protocol)
            })
            .and_then(|_| remove_all_udp_ports_for_current_app_rule_direct(&request.rule.prefix)),
        FirewallOperation::Remove => request.rule.ports.iter().try_for_each(|port| {
            remove_port_rule_direct(&request.rule.prefix, *port, request.rule.protocol)
        }),
    };
    let ok = result.is_ok();
    let response = ElevatedFirewallResponse {
        nonce: request.reply.nonce.clone(),
        ok,
        user_message: result
            .as_ref()
            .err()
            .map(|error| error.user_message.clone()),
        detail: result.err().map(|error| error.detail),
    };
    let _ = send_elevated_response(&request.reply, &response);
    process::exit(if ok { 0 } else { 1 });
}

fn run_elevated_firewall_task(
    prefix: &'static str,
    ports: Vec<u16>,
    operation: FirewallOperation,
    protocol: FirewallProtocol,
    all_ports: bool,
) -> Result<(), FirewallCommandError> {
    #[cfg(not(target_os = "linux"))]
    let exe_path = std::env::current_exe().map_err(|error| {
        FirewallCommandError::new(
            "Unable to request administrator approval for the firewall change.",
            error.to_string(),
        )
    })?;
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).map_err(
        |error| {
            FirewallCommandError::new(
                "Unable to prepare the firewall approval response channel.",
                error.to_string(),
            )
        },
    )?;
    let local_addr = listener.local_addr().map_err(|error| {
        FirewallCommandError::new(
            "Unable to prepare the firewall approval response channel.",
            error.to_string(),
        )
    })?;
    let nonce = elevated_nonce(local_addr.port());
    let request = ElevatedFirewallRequest {
        operation,
        rule: FirewallRuleSpec {
            prefix: prefix.to_string(),
            ports,
            protocol,
            all_ports,
        },
        reply: ElevatedFirewallReplyEndpoint {
            host: local_addr.ip(),
            port: local_addr.port(),
            nonce: nonce.clone(),
        },
    };
    #[cfg(target_os = "linux")]
    let exe_path = stage_elevated_executable().map_err(|error| {
        FirewallCommandError::new(
            "Unable to prepare the administrator approval helper.",
            error.to_string(),
        )
    })?;
    #[cfg(target_os = "linux")]
    let cleanup_path = exe_path.clone();
    let mut command = Command::new(exe_path);
    command
        .arg(ELEVATED_FIREWALL_FLAG)
        .arg(encode_elevated_request(&request));

    let output = elevated_command::Command::new(command)
        .output()
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to request administrator approval for the firewall change.",
                error.to_string(),
            )
        })?;

    #[cfg(target_os = "linux")]
    let _ = std::fs::remove_file(cleanup_path);
    let response = match wait_for_elevated_response(listener, &nonce) {
        Ok(response) => response,
        Err(error) if !elevated_command_started(&output) => {
            return Err(FirewallCommandError::new(
                "Administrator approval was denied, so the firewall rule was not changed.",
                format!("{}; {}", elevated_status_detail(&output), error.detail),
            ));
        }
        Err(error) => return Err(error),
    };
    if response.ok {
        return Ok(());
    }

    Err(FirewallCommandError::new(
        response.user_message.unwrap_or_else(|| {
            format!(
                "Unable to {} the firewall rule after administrator approval.",
                operation.user_action()
            )
        }),
        response
            .detail
            .unwrap_or_else(|| "elevated helper did not return an error detail".to_string()),
    ))
}

#[cfg(windows)]
fn elevated_command_started(output: &std::process::Output) -> bool {
    output.status.code().is_some_and(|code| code > 32)
}

#[cfg(not(windows))]
fn elevated_command_started(output: &std::process::Output) -> bool {
    output.status.success()
}

fn elevated_status_detail(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    format!(
        "elevated command launch status {}; stdout: {}; stderr: {}",
        output.status,
        if stdout.is_empty() {
            "<empty>"
        } else {
            &stdout
        },
        if stderr.is_empty() {
            "<empty>"
        } else {
            &stderr
        },
    )
}

#[cfg(any(windows, unix))]
fn encode_elevated_request(request: &ElevatedFirewallRequest) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let payload = serde_json::to_vec(request).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(payload)
}

#[cfg(any(windows, unix))]
fn parse_elevated_request(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Option<ElevatedFirewallRequest> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let first = args.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(first.to_str()?).ok()?;
    serde_json::from_slice::<ElevatedFirewallRequest>(&decoded).ok()
}

#[cfg(any(windows, unix))]
fn validate_elevated_request(request: ElevatedFirewallRequest) -> Option<ElevatedFirewallRequest> {
    if request.rule.ports.is_empty() || request.rule.ports.contains(&0) {
        return None;
    }
    if !request.rule.prefix.starts_with("XTerm ") {
        return None;
    }
    if !request.reply.host.is_loopback()
        || request.reply.port == 0
        || request.reply.nonce.is_empty()
    {
        return None;
    }
    Some(request)
}

#[cfg(any(windows, unix))]
fn elevated_nonce(port: u16) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    URL_SAFE_NO_PAD.encode(format!("{}:{port}:{now}", std::process::id()))
}

#[cfg(any(windows, unix))]
fn send_elevated_response(
    endpoint: &ElevatedFirewallReplyEndpoint,
    response: &ElevatedFirewallResponse,
) -> Result<(), FirewallCommandError> {
    if !endpoint.host.is_loopback() {
        return Err(FirewallCommandError::new(
            "Unable to return the firewall approval result.",
            "reply endpoint is not loopback",
        ));
    }
    let addr = SocketAddr::new(endpoint.host, endpoint.port);
    let mut stream =
        TcpStream::connect_timeout(&addr, ELEVATED_FIREWALL_IPC_TIMEOUT).map_err(|error| {
            FirewallCommandError::new(
                "Unable to return the firewall approval result.",
                error.to_string(),
            )
        })?;
    stream
        .set_write_timeout(Some(ELEVATED_FIREWALL_IPC_TIMEOUT))
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to return the firewall approval result.",
                error.to_string(),
            )
        })?;
    let payload = serde_json::to_vec(response).map_err(|error| {
        FirewallCommandError::new(
            "Unable to serialize the firewall approval result.",
            error.to_string(),
        )
    })?;
    stream.write_all(&payload).map_err(|error| {
        FirewallCommandError::new(
            "Unable to return the firewall approval result.",
            error.to_string(),
        )
    })?;
    stream.write_all(b"\n").map_err(|error| {
        FirewallCommandError::new(
            "Unable to return the firewall approval result.",
            error.to_string(),
        )
    })
}

#[cfg(any(windows, unix))]
fn wait_for_elevated_response(
    listener: TcpListener,
    nonce: &str,
) -> Result<ElevatedFirewallResponse, FirewallCommandError> {
    let (stream, peer_addr) = accept_with_timeout(listener)?;
    if !peer_addr.ip().is_loopback() {
        return Err(FirewallCommandError::new(
            "The firewall approval result came from an invalid source.",
            peer_addr.to_string(),
        ));
    }
    stream
        .set_read_timeout(Some(ELEVATED_FIREWALL_IPC_TIMEOUT))
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to wait for the firewall approval result.",
                error.to_string(),
            )
        })?;
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|error| {
        FirewallCommandError::new(
            "Unable to read the firewall approval result.",
            error.to_string(),
        )
    })?;
    let response = serde_json::from_str::<ElevatedFirewallResponse>(&line).map_err(|error| {
        FirewallCommandError::new(
            "The firewall approval result was invalid.",
            error.to_string(),
        )
    })?;
    if response.nonce != nonce {
        return Err(FirewallCommandError::new(
            "The firewall approval result failed validation.",
            "response nonce did not match the request",
        ));
    }
    Ok(response)
}

#[cfg(any(windows, unix))]
fn accept_with_timeout(
    listener: TcpListener,
) -> Result<(TcpStream, SocketAddr), FirewallCommandError> {
    listener.set_nonblocking(true).map_err(|error| {
        FirewallCommandError::new(
            "Unable to wait for the firewall approval result.",
            error.to_string(),
        )
    })?;
    let deadline = std::time::Instant::now() + ELEVATED_FIREWALL_IPC_TIMEOUT;
    loop {
        match listener.accept() {
            Ok(result) => return Ok(result),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(FirewallCommandError::new(
                        "The firewall approval request did not return a result.",
                        "timed out waiting for elevated helper response",
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return Err(FirewallCommandError::new(
                    "Unable to wait for the firewall approval result.",
                    error.to_string(),
                ));
            }
        }
    }
}

#[cfg(windows)]
fn allow_port_impl(
    prefix: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_windows_direct(prefix, port, protocol)
}

#[cfg(windows)]
fn allow_port_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_windows_direct(prefix, port, protocol)
}

#[cfg(windows)]
fn allow_port_windows_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    use windows_firewall::{
        add_rule_or_update, Action, Direction, FirewallRule, Profile, Protocol,
    };

    let windows_protocol = match protocol {
        FirewallProtocol::Tcp => Protocol::Tcp,
        FirewallProtocol::Udp => Protocol::Udp,
    };
    let rule = FirewallRule::builder()
        .name(rule_name(prefix, port))
        .description(format!(
            "Allow inbound {} traffic for {prefix} on port {port}.",
            protocol.as_str()
        ))
        .grouping(prefix)
        .direction(Direction::In)
        .enabled(true)
        .action(Action::Allow)
        .protocol(windows_protocol)
        .local_ports([port])
        .profiles(Profile::All)
        .build();

    add_rule_or_update(&rule)
        .map(|_| ())
        .map_err(|error| map_windows_error("Unable to add the Windows Firewall rule.", error))
}

#[cfg(any(windows, target_os = "macos"))]
fn all_udp_ports_rule_name(prefix: &str) -> String {
    format!("{prefix} UDP Data Ports")
}

#[cfg(windows)]
fn allow_all_udp_ports_for_current_app_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    allow_all_udp_ports_for_current_app_direct(prefix)
}

#[cfg(windows)]
fn allow_all_udp_ports_for_current_app_direct(prefix: &str) -> Result<(), FirewallCommandError> {
    use windows_firewall::{
        add_rule_or_update, remove_rule, rule_exists, Action, Direction, FirewallRule, Port,
        Profile, Protocol,
    };

    let name = all_udp_ports_rule_name(prefix);
    let rule = FirewallRule::builder()
        .name(name.clone())
        .description(format!(
            "Allow inbound UDP data traffic for {prefix}. TFTP uses ephemeral transfer IDs."
        ))
        .grouping(prefix)
        .direction(Direction::In)
        .enabled(true)
        .action(Action::Allow)
        .protocol(Protocol::Udp)
        .local_ports([Port::Any])
        .profiles(Profile::All)
        .build();

    if rule_exists(&name).map_err(|error| {
        map_windows_error("Unable to inspect the Windows Firewall rules.", error)
    })? {
        remove_rule(&name).map_err(|error| {
            map_windows_error("Unable to replace the Windows Firewall rule.", error)
        })?;
    }

    add_rule_or_update(&rule)
        .map(|_| ())
        .map_err(|error| map_windows_error("Unable to add the Windows Firewall rule.", error))
}

#[cfg(windows)]
fn remove_port_rule_impl(
    prefix: &'static str,
    port: u16,
    _protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_windows_direct(prefix, port)
}

#[cfg(windows)]
fn remove_port_rule_direct(
    prefix: &str,
    port: u16,
    _protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_windows_direct(prefix, port)
}

#[cfg(windows)]
fn remove_port_rule_windows_direct(prefix: &str, port: u16) -> Result<(), FirewallCommandError> {
    use windows_firewall::{remove_rule, rule_exists};

    let name = rule_name(prefix, port);
    if !rule_exists(&name).map_err(|error| {
        map_windows_error("Unable to inspect the Windows Firewall rules.", error)
    })? {
        return Ok(());
    }

    remove_rule(&name)
        .map_err(|error| map_windows_error("Unable to remove the Windows Firewall rule.", error))
}

#[cfg(windows)]
fn remove_all_udp_ports_for_current_app_rule_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    remove_all_udp_ports_for_current_app_rule_direct(prefix)
}

#[cfg(windows)]
fn remove_all_udp_ports_for_current_app_rule_direct(
    prefix: &str,
) -> Result<(), FirewallCommandError> {
    use windows_firewall::{remove_rule, rule_exists};

    let name = all_udp_ports_rule_name(prefix);
    if !rule_exists(&name).map_err(|error| {
        map_windows_error("Unable to inspect the Windows Firewall rules.", error)
    })? {
        return Ok(());
    }

    remove_rule(&name)
        .map_err(|error| map_windows_error("Unable to remove the Windows Firewall rule.", error))
}

#[cfg(target_os = "linux")]
fn allow_all_udp_ports_for_current_app_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    allow_all_udp_ports_for_current_app_direct(prefix)
}

#[cfg(target_os = "linux")]
fn allow_all_udp_ports_for_current_app_direct(prefix: &str) -> Result<(), FirewallCommandError> {
    let ipt = iptables::new(false).map_err(|error| {
        FirewallCommandError::new(
            "Unable to initialize the Linux firewall integration.",
            error.to_string(),
        )
    })?;
    let chain = linux_chain_name(prefix);
    ensure_linux_chain(&ipt, &chain, FirewallProtocol::Udp)?;
    ipt.append_unique("filter", &chain, &accept_all_udp_rule(prefix))
        .map_err(|error| {
            FirewallCommandError::new("Unable to add the Linux firewall rule.", error.to_string())
        })
}

#[cfg(target_os = "linux")]
fn remove_all_udp_ports_for_current_app_rule_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    remove_all_udp_ports_for_current_app_rule_direct(prefix)
}

#[cfg(target_os = "linux")]
fn remove_all_udp_ports_for_current_app_rule_direct(
    prefix: &str,
) -> Result<(), FirewallCommandError> {
    let ipt = iptables::new(false).map_err(|error| {
        FirewallCommandError::new(
            "Unable to initialize the Linux firewall integration.",
            error.to_string(),
        )
    })?;
    let chain = linux_chain_name(prefix);
    let rule = accept_all_udp_rule(prefix);
    match ipt.exists("filter", &chain, &rule) {
        Ok(true) => ipt.delete("filter", &chain, &rule).map_err(|error| {
            FirewallCommandError::new(
                "Unable to remove the Linux firewall rule.",
                error.to_string(),
            )
        })?,
        Ok(false) => {}
        Err(error) => {
            return Err(FirewallCommandError::new(
                "Unable to inspect the Linux firewall rule.",
                error.to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn map_windows_error(
    fallback_message: &str,
    error: windows_firewall::WindowsFirewallError,
) -> FirewallCommandError {
    let detail = error.to_string();
    let user_message = if firewall_error_requires_elevation(&detail) {
        "Windows blocked the firewall change because administrator approval is required."
    } else {
        fallback_message
    };
    FirewallCommandError::new(user_message, detail)
}

#[cfg(target_os = "linux")]
fn allow_port_impl(
    prefix: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_linux_direct(prefix, port, protocol)
}

#[cfg(target_os = "linux")]
fn allow_port_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_linux_direct(prefix, port, protocol)
}

#[cfg(target_os = "linux")]
fn allow_port_linux_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let ipt = iptables::new(false).map_err(|error| {
        FirewallCommandError::new(
            "Unable to initialize the Linux firewall integration.",
            error.to_string(),
        )
    })?;

    let chain = linux_chain_name(prefix);
    ensure_linux_chain(&ipt, &chain, protocol)?;
    ipt.append_unique("filter", &chain, &accept_rule(prefix, port, protocol))
        .map_err(|error| {
            FirewallCommandError::new("Unable to add the Linux firewall rule.", error.to_string())
        })
}

#[cfg(target_os = "linux")]
fn remove_port_rule_impl(
    prefix: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_linux_direct(prefix, port, protocol)
}

#[cfg(target_os = "linux")]
fn remove_port_rule_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_linux_direct(prefix, port, protocol)
}

#[cfg(target_os = "linux")]
fn remove_port_rule_linux_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let ipt = iptables::new(false).map_err(|error| {
        FirewallCommandError::new(
            "Unable to initialize the Linux firewall integration.",
            error.to_string(),
        )
    })?;

    let chain = linux_chain_name(prefix);
    let rule = accept_rule(prefix, port, protocol);
    match ipt.exists("filter", &chain, &rule) {
        Ok(true) => ipt.delete("filter", &chain, &rule).map_err(|error| {
            FirewallCommandError::new(
                "Unable to remove the Linux firewall rule.",
                error.to_string(),
            )
        })?,
        Ok(false) => {}
        Err(error) => {
            return Err(FirewallCommandError::new(
                "Unable to inspect the Linux firewall rule.",
                error.to_string(),
            ));
        }
    }

    cleanup_linux_chain(&ipt, &chain, protocol)
}

#[cfg(target_os = "linux")]
fn ensure_linux_chain(
    ipt: &iptables::IPTables,
    chain: &str,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let chain_exists = ipt.chain_exists("filter", chain).map_err(|error| {
        FirewallCommandError::new(
            "Unable to inspect the Linux firewall chain.",
            error.to_string(),
        )
    })?;
    if !chain_exists {
        ipt.new_chain("filter", chain).map_err(|error| {
            FirewallCommandError::new(
                "Unable to create the Linux firewall chain.",
                error.to_string(),
            )
        })?;
    }

    let jump_rule = linux_jump_rule(chain, protocol);
    let jump_exists = ipt.exists("filter", "INPUT", &jump_rule).map_err(|error| {
        FirewallCommandError::new(
            "Unable to inspect the Linux firewall hook.",
            error.to_string(),
        )
    })?;
    if !jump_exists {
        ipt.append("filter", "INPUT", &jump_rule).map_err(|error| {
            FirewallCommandError::new(
                "Unable to hook the Linux firewall chain into INPUT.",
                error.to_string(),
            )
        })?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn cleanup_linux_chain(
    ipt: &iptables::IPTables,
    chain: &str,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    let jump_rule = linux_jump_rule(chain, protocol);
    if matches!(ipt.exists("filter", "INPUT", &jump_rule), Ok(true)) {
        ipt.delete("filter", "INPUT", &jump_rule).map_err(|error| {
            FirewallCommandError::new(
                "Unable to detach the Linux firewall chain from INPUT.",
                error.to_string(),
            )
        })?;
    }

    if matches!(ipt.chain_exists("filter", chain), Ok(true)) {
        ipt.flush_chain("filter", chain).map_err(|error| {
            FirewallCommandError::new(
                "Unable to clear the Linux firewall chain.",
                error.to_string(),
            )
        })?;
        ipt.delete_chain("filter", chain).map_err(|error| {
            FirewallCommandError::new(
                "Unable to remove the Linux firewall chain.",
                error.to_string(),
            )
        })?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_chain_name(prefix: &str) -> String {
    prefix
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

#[cfg(target_os = "linux")]
fn linux_protocol(protocol: FirewallProtocol) -> &'static str {
    match protocol {
        FirewallProtocol::Tcp => "tcp",
        FirewallProtocol::Udp => "udp",
    }
}

#[cfg(target_os = "linux")]
fn linux_jump_rule(chain: &str, protocol: FirewallProtocol) -> String {
    format!("-p {} -j {chain}", linux_protocol(protocol))
}

#[cfg(target_os = "linux")]
fn accept_rule(prefix: &str, port: u16, protocol: FirewallProtocol) -> String {
    format!(
        "-p {} --dport {port} -m comment --comment \"{prefix}\" -j ACCEPT",
        linux_protocol(protocol)
    )
}

#[cfg(target_os = "linux")]
fn accept_all_udp_rule(prefix: &str) -> String {
    format!("-p udp -m comment --comment \"{prefix} ephemeral\" -j ACCEPT")
}

#[cfg(target_os = "macos")]
fn allow_port_impl(
    prefix: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_macos_direct(prefix, port, protocol)
}

#[cfg(target_os = "macos")]
fn allow_port_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    allow_port_macos_direct(prefix, port, protocol)
}

#[cfg(target_os = "macos")]
fn allow_port_macos_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    use pfctl::{
        AnchorKind, Endpoint, FilterRuleAction, FilterRuleBuilder, PfCtl, Port, Proto, RulesetKind,
    };

    let mut pf = PfCtl::new().map_err(map_pfctl_init_error)?;
    pf.try_enable().map_err(map_pfctl_runtime_error(
        "Unable to enable Packet Filter for the firewall rule.",
    ))?;
    pf.try_add_anchor(prefix, AnchorKind::Filter)
        .map_err(map_pfctl_runtime_error(
            "Unable to create the macOS firewall anchor.",
        ))?;
    pf.flush_rules(prefix, RulesetKind::Filter)
        .map_err(map_pfctl_runtime_error(
            "Unable to refresh the macOS firewall rules.",
        ))?;

    let pf_protocol = match protocol {
        FirewallProtocol::Tcp => Proto::Tcp,
        FirewallProtocol::Udp => Proto::Udp,
    };
    let rule = FilterRuleBuilder::default()
        .action(FilterRuleAction::Pass)
        .quick(true)
        .proto(pf_protocol)
        .to(Endpoint::new(pfctl::Ip::Any, Port::from(port)))
        .label(rule_name(prefix, port))
        .build()
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to prepare the macOS firewall rule.",
                error.to_string(),
            )
        })?;

    pf.add_rule(prefix, &rule).map_err(map_pfctl_runtime_error(
        "Unable to add the macOS firewall rule.",
    ))
}

#[cfg(target_os = "macos")]
fn remove_port_rule_impl(
    prefix: &'static str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_macos_direct(prefix, port, protocol)
}

#[cfg(target_os = "macos")]
fn remove_port_rule_direct(
    prefix: &str,
    port: u16,
    protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_macos_direct(prefix, port, protocol)
}

#[cfg(target_os = "macos")]
fn remove_port_rule_macos_direct(
    prefix: &str,
    _port: u16,
    _protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    use pfctl::{AnchorKind, PfCtl, RulesetKind};

    let mut pf = PfCtl::new().map_err(map_pfctl_init_error)?;
    if pf.flush_rules(prefix, RulesetKind::Filter).is_err() {
        return Ok(());
    }
    let _ = pf.try_remove_anchor(prefix, AnchorKind::Filter);
    Ok(())
}

// TFTP negotiates ephemeral UDP transfer IDs, so a single-port rule is not
// enough. Mirrors the Windows/Linux behaviour by appending a blanket inbound
// UDP allow rule to the service anchor.
#[cfg(target_os = "macos")]
fn allow_all_udp_ports_for_current_app_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    allow_all_udp_ports_for_current_app_direct(prefix)
}

#[cfg(target_os = "macos")]
fn allow_all_udp_ports_for_current_app_direct(prefix: &str) -> Result<(), FirewallCommandError> {
    use pfctl::{AnchorKind, Endpoint, FilterRuleAction, FilterRuleBuilder, PfCtl, Port, Proto};

    let mut pf = PfCtl::new().map_err(map_pfctl_init_error)?;
    pf.try_enable().map_err(map_pfctl_runtime_error(
        "Unable to enable Packet Filter for the firewall rule.",
    ))?;
    // Do not flush here: the service-port rule for the same anchor was added
    // by `allow_port_macos_direct` right before this call.
    pf.try_add_anchor(prefix, AnchorKind::Filter)
        .map_err(map_pfctl_runtime_error(
            "Unable to create the macOS firewall anchor.",
        ))?;

    let rule = FilterRuleBuilder::default()
        .action(FilterRuleAction::Pass)
        .quick(true)
        .proto(Proto::Udp)
        .to(Endpoint::new(pfctl::Ip::Any, Port::Any))
        .label(all_udp_ports_rule_name(prefix))
        .build()
        .map_err(|error| {
            FirewallCommandError::new(
                "Unable to prepare the macOS firewall rule.",
                error.to_string(),
            )
        })?;

    pf.add_rule(prefix, &rule).map_err(map_pfctl_runtime_error(
        "Unable to add the macOS firewall rule.",
    ))
}

#[cfg(target_os = "macos")]
fn remove_all_udp_ports_for_current_app_rule_impl(
    prefix: &'static str,
) -> Result<(), FirewallCommandError> {
    remove_all_udp_ports_for_current_app_rule_direct(prefix)
}

// PF anchors cannot drop a single labelled rule; the anchor is per-service,
// so flushing it removes the blanket UDP rule together with the port rule.
#[cfg(target_os = "macos")]
fn remove_all_udp_ports_for_current_app_rule_direct(
    prefix: &str,
) -> Result<(), FirewallCommandError> {
    remove_port_rule_macos_direct(prefix, 0, FirewallProtocol::Udp)
}

#[cfg(target_os = "macos")]
fn map_pfctl_init_error(error: pfctl::Error) -> FirewallCommandError {
    let detail = error.to_string();
    let user_message = if firewall_error_requires_elevation(&detail) {
        "Administrator permission is required to update the macOS firewall rule."
    } else {
        "Unable to initialize the macOS firewall integration."
    };
    FirewallCommandError::new(user_message, detail)
}

#[cfg(target_os = "macos")]
fn map_pfctl_runtime_error(
    fallback_message: &'static str,
) -> impl FnOnce(pfctl::Error) -> FirewallCommandError {
    move |error| {
        let detail = error.to_string();
        let user_message = if firewall_error_requires_elevation(&detail) {
            "Administrator permission is required to update the macOS firewall rule."
        } else {
            fallback_message
        };
        FirewallCommandError::new(user_message, detail)
    }
}

#[cfg(any(windows, unix))]
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn allow_port_impl(
    _prefix: &'static str,
    _port: u16,
    _protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    Err(FirewallCommandError::new(
        "Automatic firewall rule management is not implemented for this platform.",
        "unsupported platform",
    ))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn remove_port_rule_impl(
    _prefix: &'static str,
    _port: u16,
    _protocol: FirewallProtocol,
) -> Result<(), FirewallCommandError> {
    Err(FirewallCommandError::new(
        "Automatic firewall rule management is not implemented for this platform.",
        "unsupported platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_request_token_round_trips_space_containing_rule() {
        let request = ElevatedFirewallRequest {
            operation: FirewallOperation::Allow,
            rule: FirewallRuleSpec {
                prefix: "XTerm Proxy".to_string(),
                ports: vec![3128],
                protocol: FirewallProtocol::Tcp,
                all_ports: false,
            },
            reply: ElevatedFirewallReplyEndpoint {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 49152,
                nonce: "nonce".to_string(),
            },
        };

        let token = encode_elevated_request(&request);
        assert!(!token.contains([' ', '"', '\\']));

        let parsed = parse_elevated_request(std::iter::once(token.into())).expect("request");
        assert!(matches!(parsed.operation, FirewallOperation::Allow));
        assert_eq!(parsed.rule.prefix, "XTerm Proxy");
        assert_eq!(parsed.rule.ports, vec![3128]);
        assert!(matches!(parsed.rule.protocol, FirewallProtocol::Tcp));
        assert!(!parsed.rule.all_ports);
        assert_eq!(parsed.reply.nonce, "nonce");
    }

    #[test]
    fn elevated_ipc_accepts_matching_nonce_response() {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let nonce = "nonce-ok".to_string();
        let endpoint = ElevatedFirewallReplyEndpoint {
            host: addr.ip(),
            port: addr.port(),
            nonce: nonce.clone(),
        };
        let response = ElevatedFirewallResponse {
            nonce: nonce.clone(),
            ok: true,
            user_message: None,
            detail: None,
        };

        let sender = std::thread::spawn(move || send_elevated_response(&endpoint, &response));
        let received = wait_for_elevated_response(listener, &nonce).expect("response");
        sender.join().unwrap().expect("send");

        assert!(received.ok);
        assert_eq!(received.nonce, nonce);
    }

    #[test]
    fn elevated_ipc_rejects_mismatched_nonce_response() {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = ElevatedFirewallReplyEndpoint {
            host: addr.ip(),
            port: addr.port(),
            nonce: "actual".to_string(),
        };
        let response = ElevatedFirewallResponse {
            nonce: "actual".to_string(),
            ok: true,
            user_message: None,
            detail: None,
        };

        let sender = std::thread::spawn(move || send_elevated_response(&endpoint, &response));
        let error = wait_for_elevated_response(listener, "expected").expect_err("nonce mismatch");
        sender.join().unwrap().expect("send");

        assert!(error.detail.contains("nonce"));
    }
}
