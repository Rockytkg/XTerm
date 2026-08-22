use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::{
    file_service::{
        ftp,
        models::{
            default_port, validate_file_service_config, FileServiceConfig, FileServiceSettings,
        },
        password, sftp, tftp, FileServiceProtocol,
    },
    network_interface::validate_bind_ip,
    state::AppState,
    storage::repository::SettingsRepository,
};

/// Shared, hot-updatable authentication secrets for the running FTP/SFTP
/// servers. The runtimes hold a clone of this handle so `set_password` can
/// update authentication without restarting the listener.
pub(crate) type SharedPassword = Arc<parking_lot::RwLock<String>>;

enum Runtime {
    Tftp(tftp::TftpRuntimeHandle),
    Ftp(ftp::FtpRuntimeHandle),
    Sftp(sftp::SftpRuntimeHandle),
}

impl Runtime {
    fn is_running(&self) -> bool {
        match self {
            Self::Tftp(runtime) => runtime
                .accept_tasks
                .iter()
                .any(|task| !task.inner().is_finished()),
            Self::Ftp(runtime) => !runtime.accept_task.inner().is_finished(),
            Self::Sftp(runtime) => !runtime.accept_task.inner().is_finished(),
        }
    }
}

pub(crate) struct FileServiceManager {
    pub(crate) settings: FileServiceSettings,
    runtime: Option<Runtime>,
    password_handle: SharedPassword,
}

impl FileServiceManager {
    pub(crate) fn from_store(store: &impl SettingsRepository) -> Self {
        let settings = FileServiceSettings::from_store(store);
        let password_handle = Arc::new(parking_lot::RwLock::new(settings.config.password.clone()));
        Self {
            settings,
            runtime: None,
            password_handle,
        }
    }

    pub(crate) fn config_snapshot(&self) -> FileServiceConfig {
        self.settings.config_snapshot(self.is_running())
    }

    pub(crate) fn password_handle(&self) -> SharedPassword {
        self.password_handle.clone()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.runtime.as_ref().is_some_and(Runtime::is_running)
    }

    fn has_runtime(&self) -> bool {
        self.runtime.is_some()
    }

    fn take_runtime(&mut self) -> Option<Runtime> {
        self.runtime.take()
    }
}

pub(crate) struct FileServiceService<'a> {
    app: AppHandle,
    state: &'a AppState,
}

impl<'a> FileServiceService<'a> {
    pub(crate) fn new(app: AppHandle, state: &'a AppState) -> Self {
        Self { app, state }
    }

    pub(crate) fn config(&self) -> FileServiceConfig {
        self.state.file_service().config_snapshot()
    }

    pub(crate) async fn start(
        &self,
        protocol: String,
        bind_ip: String,
        shared_dir: String,
    ) -> Result<FileServiceConfig, String> {
        let protocol = FileServiceProtocol::parse(&protocol)?;
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        let mut config = self.config();
        config.protocol = protocol.as_str().to_string();
        config.bind_ip = bind_ip;
        config.shared_dir = shared_dir;
        // 监听端口固定为协议默认值（TFTP 69 / FTP 21 / SFTP 22），随协议切换。
        config.port = default_port(protocol.as_str());
        self.apply_protocol(config, protocol).await
    }

    pub(crate) async fn stop(&self) -> Result<FileServiceConfig, String> {
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        self.stop_runtime().await
    }

    pub(crate) async fn update_bind_ip(
        &self,
        bind_ip: String,
    ) -> Result<FileServiceConfig, String> {
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        let mut config = self.config();
        config.bind_ip = bind_ip;
        self.apply_or_save(config).await
    }

    pub(crate) async fn update_shared_dir(
        &self,
        shared_dir: String,
    ) -> Result<FileServiceConfig, String> {
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        let mut config = self.config();
        config.shared_dir = shared_dir;
        self.apply_or_save(config).await
    }

    pub(crate) async fn update_credentials(
        &self,
        username: String,
        password: String,
    ) -> Result<FileServiceConfig, String> {
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        let mut config = self.config();
        config.username = username;
        // An empty password keeps the current one; a non-empty password is
        // stored in the keyring, never in redb.
        if !password.is_empty() {
            config.password = password::set_password(&password::KeyringPasswordVault, &password)?;
        }
        self.apply_or_save(config).await
    }

    /// Stores the file service password in the OS credential vault and
    /// hot-updates the running server's authentication without a restart.
    /// An empty password resets the service to the built-in default.
    pub(crate) async fn set_password(
        &self,
        new_password: String,
    ) -> Result<FileServiceConfig, String> {
        let operation_lock = self.state.file_service_operation_lock();
        let _guard = operation_lock.lock().await;
        let resolved = password::set_password(&password::KeyringPasswordVault, &new_password)?;
        let mut manager = self.state.file_service();
        manager.settings.config.password = resolved.clone();
        *manager.password_handle.write() = resolved;
        Ok(manager.config_snapshot())
    }

    async fn apply_or_save(&self, config: FileServiceConfig) -> Result<FileServiceConfig, String> {
        validate_bind_ip(&config.bind_ip)?;
        if self.state.file_service().is_running() {
            self.apply(config).await
        } else {
            if self.state.file_service().has_runtime() {
                self.stop_runtime().await?;
            }
            self.persist(&config)?;
            let mut manager = self.state.file_service();
            manager.settings.config = config;
            Ok(manager.config_snapshot())
        }
    }

    async fn apply(&self, config: FileServiceConfig) -> Result<FileServiceConfig, String> {
        let protocol = FileServiceProtocol::parse(&config.protocol)?;
        self.apply_protocol(config, protocol).await
    }

    async fn apply_protocol(
        &self,
        config: FileServiceConfig,
        protocol: FileServiceProtocol,
    ) -> Result<FileServiceConfig, String> {
        validate_file_service_config(&config).await?;
        self.persist(&config)?;
        if self.state.file_service().has_runtime() {
            self.stop_runtime().await?;
        }
        let password_handle = {
            let mut manager = self.state.file_service();
            // `running` is derived from the live runtime, never a stored
            // setting: persisting a stale `true` here would make a later
            // stop report the service as still running.
            manager.settings.config = FileServiceConfig {
                running: false,
                ..config.clone()
            };
            // Keep the shared handle in sync so a restarted runtime always
            // authenticates with the latest password.
            *manager.password_handle.write() = config.password.clone();
            manager.password_handle()
        };
        let runtime = match protocol {
            FileServiceProtocol::Tftp => {
                Runtime::Tftp(tftp::start_runtime(self.app.clone(), config.clone()).await?)
            }
            FileServiceProtocol::Ftp => {
                Runtime::Ftp(ftp::start_runtime(self.app.clone(), &config, password_handle).await?)
            }
            FileServiceProtocol::Sftp => Runtime::Sftp(
                sftp::start_runtime(self.app.clone(), self.state, &config, password_handle).await?,
            ),
        };
        let mut manager = self.state.file_service();
        manager.runtime = Some(runtime);
        Ok(manager.config_snapshot())
    }

    async fn stop_runtime(&self) -> Result<FileServiceConfig, String> {
        let (runtime, config) = {
            let mut manager = self.state.file_service();
            (manager.take_runtime(), manager.settings.config.clone())
        };
        if let Some(runtime) = runtime {
            match runtime {
                Runtime::Tftp(runtime) => {
                    tftp::stop_runtime(&self.app, runtime, config.port).await?
                }
                Runtime::Ftp(runtime) => ftp::stop_runtime(runtime, config.port).await?,
                Runtime::Sftp(runtime) => sftp::stop_runtime(runtime, config.port).await?,
            }
        }
        // Return a fresh snapshot so `running` reflects the runtime just
        // stopped instead of whatever flag was last stored in the settings.
        Ok(self.state.file_service().config_snapshot())
    }

    fn persist(&self, config: &FileServiceConfig) -> Result<(), String> {
        let store = self.state.store();
        SettingsRepository::set_setting(
            &*store,
            crate::file_service::models::FILE_SERVICE_BIND_IP_KEY,
            &config.bind_ip,
        )?;
        SettingsRepository::set_setting(
            &*store,
            crate::file_service::models::FILE_SERVICE_PROTOCOL_KEY,
            &config.protocol,
        )?;
        SettingsRepository::set_setting(
            &*store,
            crate::file_service::models::FILE_SERVICE_SHARED_DIR_KEY,
            &config.shared_dir,
        )?;
        SettingsRepository::set_setting(
            &*store,
            crate::file_service::models::FILE_SERVICE_USERNAME_KEY,
            &config.username,
        )
    }
}

pub(crate) async fn shutdown_runtime<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let operation_lock = state.file_service_operation_lock();
    let _guard = operation_lock.lock().await;
    let (runtime, config) = {
        let mut manager = state.file_service();
        (manager.take_runtime(), manager.settings.config.clone())
    };
    // 退出时尽力而为：各协议的 stop_runtime 内部都会移除自己的防火墙规则，
    // 这里统一忽略错误。
    match runtime {
        Some(Runtime::Tftp(runtime)) => {
            let _ = tftp::stop_runtime(app, runtime, config.port).await;
        }
        Some(Runtime::Ftp(runtime)) => {
            let _ = ftp::stop_runtime(runtime, config.port).await;
        }
        Some(Runtime::Sftp(runtime)) => {
            let _ = sftp::stop_runtime(runtime, config.port).await;
        }
        None => {}
    }
}
