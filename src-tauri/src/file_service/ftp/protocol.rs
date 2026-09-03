use std::{ops::RangeInclusive, sync::Arc, time::Duration};

use async_trait::async_trait;
use libunftp::{
    notification::{DataEvent, DataListener, EventMeta},
    options::{ActivePassiveMode, Shutdown},
    ServerBuilder,
};
use tauri::AppHandle;
use tokio::sync::watch;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};
use unftp_sbe_fs::Filesystem;

use crate::{
    elevated::{self, ServiceRule},
    file_service::{
        firewall,
        manager::SharedPassword,
        models::{
            await_runtime_task, canonical_shared_dir, emit_file_service_config, emit_file_transfer,
            validate_service_config, FileServiceConfig, TransferRegistry, DEFAULT_FTP_PASSIVE_END,
            DEFAULT_FTP_PASSIVE_START,
        },
    },
    logging,
};

const FTP_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

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

/// 恒定时间比较，避免口令校验在首个不匹配字节处提前返回而泄露时序信息。
/// 长度不等时直接失败（长度本身不视为秘密）。
fn passwords_equal(provided: &str, expected: &str) -> bool {
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    if provided.len() != expected.len() {
        return false;
    }
    provided
        .iter()
        .zip(expected.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
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

pub(crate) async fn start_runtime(
    app: AppHandle,
    config: &FileServiceConfig,
    password: SharedPassword,
) -> Result<FtpRuntimeHandle, String> {
    validate_service_config("FTP", config)?;
    let root = canonical_shared_dir("FTP", &config.shared_dir).await?;
    let passive_ports = DEFAULT_FTP_PASSIVE_START..=DEFAULT_FTP_PASSIVE_END;
    let bind_addr = format!("{}:{}", config.bind_ip, config.port);
    // libunftp 直接监听真实地址，确保主动模式下控制连接和数据连接看到
    // 相同的远端 IP。防火墙规则独立管理，不再引入会丢失对端地址的代理。
    let firewall_ports = std::iter::once(config.port)
        .chain(passive_ports.clone())
        .collect::<Vec<_>>();
    elevated::allow_service_rule(&ServiceRule {
        prefix: "XTerm FTP",
        action: "ftp.firewall.allow",
        protocol: crate::firewall::FirewallProtocol::Tcp,
        ports: firewall_ports,
        all_udp: false,
    })
    .await?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let auth = Arc::new(PasswordAuthenticator {
        username: config.username.clone(),
        password,
    });
    let shared = TransferRegistry::new();
    let listener = TransferListener {
        app: app.clone(),
        shared,
    };
    let server_passive_ports = passive_ports.clone();
    let stopped_config = config.clone();
    let status_app = app.clone();
    let shutdown_observer = shutdown_rx.clone();
    let accept_task = tauri::async_runtime::spawn(async move {
        let root_for_storage = root.clone();
        let shutdown_indicator = async move {
            let mut receiver = shutdown_rx;
            let _ = receiver.changed().await;
            Shutdown::new().grace_period(Duration::from_secs(5))
        };
        let server = ServerBuilder::with_authenticator(
            Box::new(move || {
                Filesystem::new(root_for_storage.clone()).expect("validated FTP root")
            }),
            auth,
        )
        .passive_ports(server_passive_ports)
        .active_passive_mode(ActivePassiveMode::ActiveAndPassive)
        .idle_session_timeout(300)
        .notify_data(listener)
        .shutdown_indicator(shutdown_indicator)
        .build();
        let result = match server {
            Ok(server) => server
                .listen(bind_addr)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        let failed = result.is_err() && !*shutdown_observer.borrow();
        if let Err(error) = result {
            logging::event("ftp.runtime", "ftp.task.failed")
                .field("error", error)
                .warn();
        }
        if failed {
            emit_file_service_config(&status_app, stopped_config);
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
    task_result?;
    firewall_result?;
    logging::event("ftp.runtime", "ftp.stop.success")
        .field("port", port)
        .info();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::passwords_equal;

    #[test]
    fn password_comparison_matches_only_identical_passwords() {
        assert!(passwords_equal("s3cret", "s3cret"));
        assert!(!passwords_equal("s3cret", "s3creT"));
        assert!(!passwords_equal("s3cret", "s3cret-longer"));
        assert!(!passwords_equal("", "s3cret"));
    }
}
