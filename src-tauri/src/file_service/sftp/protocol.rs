use std::{
    borrow::Cow, collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration,
};

use russh::{server, Channel, ChannelId};
use russh_sftp::{
    protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode},
    server::Handler,
};
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::TcpListener,
    sync::{watch, Mutex},
    task::JoinSet,
};

use crate::{
    elevated::{self, BindSpec, BoundSocket, ServiceRule},
    file_service::{
        manager::SharedPassword,
        models::{
            await_runtime_task, canonical_shared_dir, emit_file_transfer, parse_bind_address,
            validate_service_config, FileServiceConfig, TransferRegistry,
        },
    },
    logging,
    state::AppState,
};

const SFTP_MAX_READ: u32 = 256 * 1024;
const SFTP_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct SftpRuntimeHandle {
    shutdown_tx: watch::Sender<bool>,
    pub(crate) accept_task: tauri::async_runtime::JoinHandle<()>,
}

pub(crate) async fn start_runtime(
    app: AppHandle,
    state: &AppState,
    config: &FileServiceConfig,
    password: SharedPassword,
) -> Result<SftpRuntimeHandle, String> {
    validate_service_config("SFTP", config)?;
    let host_keys = load_or_create_host_keys(state).await?;
    let shared_dir = canonical_shared_dir("SFTP", &config.shared_dir).await?;
    let bind_addr = parse_bind_address("SFTP", &config.bind_ip, config.port)?;
    let raw_listener = elevated::bind_service_sockets(
        ServiceRule {
            prefix: "XTerm SFTP",
            action: "sftp.firewall.allow",
            protocol: crate::firewall::FirewallProtocol::Tcp,
            ports: vec![config.port],
            all_udp: false,
        },
        vec![BindSpec::tcp(bind_addr)],
        None,
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| "The SFTP listener was not created.".to_string())?;
    let listener = match raw_listener {
        BoundSocket::Tcp(listener) => TcpListener::from_std(listener)
            .map_err(|error| format!("failed to prepare SFTP server socket: {error}"))?,
        BoundSocket::Udp(_) => return Err("The SFTP listener protocol was invalid.".to_string()),
    };
    let mut preferred = russh::Preferred::default();
    let mut host_algorithms = preferred.key.to_vec();
    let rsa_ssh = russh::keys::ssh_key::Algorithm::Rsa { hash: None };
    if !host_algorithms.contains(&rsa_ssh) {
        host_algorithms.push(rsa_ssh);
    }
    preferred.key = Cow::Owned(host_algorithms);
    // Keep modern algorithms first, but interoperate with legacy switches
    // that only implement the group1/SHA-1 exchange.
    let mut kex = preferred.kex.to_vec();
    if !kex.contains(&russh::kex::DH_G1_SHA1) {
        kex.push(russh::kex::DH_G1_SHA1);
    }
    preferred.kex = Cow::Owned(kex);
    let mut server_config = server::Config {
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::from_millis(100)),
        keys: host_keys,
        preferred,
        ..Default::default()
    };
    server_config.inactivity_timeout = Some(Duration::from_secs(300));

    let shared = TransferRegistry::new();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let context = SftpServerContext {
        root: shared_dir,
        username: config.username.clone(),
        password,
        app: app.clone(),
        shared,
    };
    let accept_task = tauri::async_runtime::spawn(async move {
        accept_loop(listener, Arc::new(server_config), context, shutdown_rx).await;
    });

    logging::event("sftp.runtime", "sftp.start.success")
        .field("bind_addr", bind_addr)
        .field("shared_dir", &config.shared_dir)
        .info();
    Ok(SftpRuntimeHandle {
        shutdown_tx,
        accept_task,
    })
}

pub(crate) async fn stop_runtime(runtime: SftpRuntimeHandle, port: u16) -> Result<(), String> {
    let _ = runtime.shutdown_tx.send(true);
    let task_result =
        await_runtime_task("SFTP", SFTP_TASK_DRAIN_TIMEOUT, runtime.accept_task).await;
    let firewall_result = crate::firewall::remove_service_port_rule(
        "XTerm SFTP",
        "sftp.firewall.remove",
        port,
        crate::firewall::FirewallProtocol::Tcp,
    )
    .await
    .map_err(|error| error.user_message.clone());
    task_result?;
    firewall_result?;
    logging::event("sftp.runtime", "sftp.stop.success")
        .field("port", port)
        .info();
    Ok(())
}

#[derive(Clone)]
struct SftpServerContext {
    root: PathBuf,
    username: String,
    password: SharedPassword,
    app: AppHandle,
    shared: Arc<TransferRegistry>,
}

async fn accept_loop(
    listener: TcpListener,
    config: Arc<server::Config>,
    context: SftpServerContext,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() { break; }
            }
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(connection) => connection,
                    Err(error) => {
                        logging::event("sftp.runtime", "sftp.accept.failed")
                            .field("error", error.to_string())
                            .warn();
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let handler = SshSession::new(
                    context.root.clone(),
                    context.username.clone(),
                    context.password.clone(),
                    context.app.clone(),
                    context.shared.clone(),
                    peer,
                );
                let config = config.clone();
                connections.spawn(async move {
                    if let Err(error) = server::run_stream(config, stream, handler).await {
                        logging::event("sftp.runtime", "sftp.connection.failed")
                            .field("peer", peer)
                            .field("error", error.to_string())
                        .debug();
                    }
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    logging::event("sftp.runtime", "sftp.connection.task_failed")
                        .field("error", error.to_string())
                        .warn();
                }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

#[derive(Clone)]
struct SshSession {
    root: PathBuf,
    username: String,
    password: SharedPassword,
    app: AppHandle,
    shared: Arc<TransferRegistry>,
    peer: SocketAddr,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<server::Msg>>>>,
}

impl SshSession {
    fn new(
        root: PathBuf,
        username: String,
        password: SharedPassword,
        app: AppHandle,
        shared: Arc<TransferRegistry>,
        peer: SocketAddr,
    ) -> Self {
        Self {
            root,
            username,
            password,
            app,
            shared,
            peer,
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl server::Server for SshSession {
    type Handler = Self;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        self.clone()
    }
}

impl server::Handler for SshSession {
    type Error = anyhow::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if user == self.username && password == *self.password.read() {
            Ok(server::Auth::Accept)
        } else {
            // 审计拒绝事件：只记用户名与来源地址，绝不记口令。
            logging::event("sftp.runtime", "sftp.auth.rejected")
                .field("username", user)
                .field("peer", self.peer.to_string())
                .info();
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        _: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }
        let channel = self
            .channels
            .lock()
            .await
            .remove(&channel_id)
            .ok_or_else(|| anyhow::anyhow!("SFTP channel not found"))?;
        session.channel_success(channel_id)?;
        let handler = SftpSession::new(
            self.root.clone(),
            self.app.clone(),
            self.shared.clone(),
            self.peer,
        );
        russh_sftp::server::run(channel.into_stream(), handler).await;
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        session.close(channel)?;
        Ok(())
    }
}

struct FileHandle {
    path: PathBuf,
    transfer_id: String,
    writable: bool,
}

struct SftpSession {
    root: PathBuf,
    app: AppHandle,
    shared: Arc<TransferRegistry>,
    peer: SocketAddr,
    handles: HashMap<String, FileHandle>,
    dirs: HashMap<String, PathBuf>,
    completed_dirs: std::collections::HashSet<String>,
    next_handle: u64,
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        for file_handle in self.handles.values() {
            if let Some(event) = self.shared.finish_transfer(
                &file_handle.transfer_id,
                Some("SFTP connection closed before the transfer completed".to_string()),
            ) {
                emit_file_transfer(&self.app, event);
            }
        }
    }
}

impl SftpSession {
    fn new(root: PathBuf, app: AppHandle, shared: Arc<TransferRegistry>, peer: SocketAddr) -> Self {
        Self {
            root,
            app,
            shared,
            peer,
            handles: HashMap::new(),
            dirs: HashMap::new(),
            completed_dirs: std::collections::HashSet::new(),
            next_handle: 1,
        }
    }

    fn handle(&mut self, prefix: &str) -> String {
        let value = format!("{prefix}-{}", self.next_handle);
        self.next_handle += 1;
        value
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let relative = path.trim_start_matches('/');
        let candidate = self.root.join(relative);
        if relative.split('/').any(|part| part == "..") {
            return Err(StatusCode::PermissionDenied);
        }
        Ok(candidate)
    }

    async fn secure_path(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let candidate = self.resolve(path)?;
        let canonical = match fs::canonicalize(&candidate).await {
            Ok(path) => path,
            Err(_) => {
                let parent = candidate.parent().ok_or(StatusCode::PermissionDenied)?;
                let parent = fs::canonicalize(parent)
                    .await
                    .map_err(|_| StatusCode::PermissionDenied)?;
                parent.join(candidate.file_name().ok_or(StatusCode::PermissionDenied)?)
            }
        };
        if !canonical.starts_with(&self.root) {
            return Err(StatusCode::PermissionDenied);
        }
        Ok(canonical)
    }

    fn attrs(metadata: &std::fs::Metadata) -> FileAttributes {
        let mut attrs = FileAttributes {
            size: Some(metadata.len()),
            ..Default::default()
        };
        attrs.set_dir(metadata.is_dir());
        attrs.set_regular(metadata.is_file());
        attrs
    }

    fn status(id: u32, code: StatusCode, message: &str) -> Status {
        Status {
            id,
            status_code: code,
            error_message: message.to_string(),
            language_tag: "en-US".to_string(),
        }
    }
}

impl Handler for SftpSession {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        _id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let writable = pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::APPEND);
        let path = self.secure_path(&filename).await?;
        let metadata = fs::metadata(&path).await.ok();
        if !writable && !metadata.as_ref().is_some_and(|value| value.is_file()) {
            return Err(StatusCode::NoSuchFile);
        }
        if writable && metadata.as_ref().is_some_and(|value| value.is_dir()) {
            return Err(StatusCode::Failure);
        }
        if writable && metadata.is_none() && !pflags.contains(OpenFlags::CREATE) {
            return Err(StatusCode::NoSuchFile);
        }
        if writable && pflags.contains(OpenFlags::EXCLUDE) && metadata.is_some() {
            return Err(StatusCode::Failure);
        }
        if writable && pflags.contains(OpenFlags::TRUNCATE) {
            fs::File::create(&path)
                .await
                .map_err(|_| StatusCode::Failure)?;
        }
        let handle = self.handle("file");
        let transfer_id = crate::ids::new_id();
        self.shared.start_transfer(
            &transfer_id,
            if writable { "write" } else { "read" },
            &filename,
            &self.peer.to_string(),
            metadata.as_ref().map_or(0, std::fs::Metadata::len),
        );
        if let Some(event) = self.shared.transfer_event(&transfer_id, false, None) {
            emit_file_transfer(&self.app, event);
        }
        self.handles.insert(
            handle.clone(),
            FileHandle {
                path,
                transfer_id,
                writable,
            },
        );
        Ok(Handle { id: _id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let file_handle = self.handles.get(&handle).ok_or(StatusCode::Failure)?;
        let mut file = fs::File::open(&file_handle.path)
            .await
            .map_err(|_| StatusCode::NoSuchFile)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|_| StatusCode::Failure)?;
        let mut data = vec![0; len.min(SFTP_MAX_READ) as usize];
        let size = file
            .read(&mut data)
            .await
            .map_err(|_| StatusCode::Failure)?;
        data.truncate(size);
        if size == 0 {
            return Err(StatusCode::Eof);
        }
        if let Some(event) = self
            .shared
            .record_progress(&file_handle.transfer_id, size as u64)
        {
            emit_file_transfer(&self.app, event);
        }
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let file_handle = self.handles.get(&handle).ok_or(StatusCode::Failure)?;
        if !file_handle.writable {
            return Err(StatusCode::PermissionDenied);
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&file_handle.path)
            .await
            .map_err(|_| StatusCode::Failure)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|_| StatusCode::Failure)?;
        file.write_all(&data)
            .await
            .map_err(|_| StatusCode::Failure)?;
        if let Some(event) = self
            .shared
            .record_progress(&file_handle.transfer_id, data.len() as u64)
        {
            emit_file_transfer(&self.app, event);
        }
        Ok(Self::status(id, StatusCode::Ok, "Ok"))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        if let Some(file_handle) = self.handles.remove(&handle) {
            if let Some(event) = self.shared.finish_transfer(&file_handle.transfer_id, None) {
                emit_file_transfer(&self.app, event);
            }
        }
        Ok(Self::status(id, StatusCode::Ok, "Ok"))
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let path = self.secure_path(&path).await?;
        let metadata = fs::metadata(path)
            .await
            .map_err(|_| StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: Self::attrs(&metadata),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let normalized = if path.trim().is_empty() {
            "/".to_string()
        } else {
            format!("/{}", path.trim_start_matches('/'))
        };
        Ok(Name {
            id,
            files: vec![File::dummy(normalized)],
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let path = self.secure_path(&path).await?;
        let metadata = fs::metadata(&path)
            .await
            .map_err(|_| StatusCode::NoSuchFile)?;
        if !metadata.is_dir() {
            return Err(StatusCode::Failure);
        }
        let handle = self.handle("dir");
        self.dirs.insert(handle.clone(), path);
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if self.completed_dirs.contains(&handle) {
            return Err(StatusCode::Eof);
        }
        let path = self.dirs.get(&handle).ok_or(StatusCode::Failure)?.clone();
        let mut entries = fs::read_dir(path).await.map_err(|_| StatusCode::Failure)?;
        let mut files = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| StatusCode::Failure)?
        {
            let metadata = entry.metadata().await.map_err(|_| StatusCode::Failure)?;
            files.push(File::new(
                entry.file_name().to_string_lossy(),
                Self::attrs(&metadata),
            ));
        }
        self.completed_dirs.insert(handle);
        if files.is_empty() {
            return Err(StatusCode::Eof);
        }
        Ok(Name { id, files })
    }
}

async fn load_or_create_host_keys(
    state: &AppState,
) -> Result<Vec<russh::keys::PrivateKey>, String> {
    let path = state.paths().data_dir().join("sftp_host_key");
    let rsa_path = state.paths().data_dir().join("sftp_host_key_rsa");
    let mut keys = Vec::new();
    if let Ok(raw) = fs::read(&path).await {
        keys.push(
            russh::keys::PrivateKey::from_openssh(raw)
                .map_err(|error| format!("failed to parse persisted SFTP host key: {error}"))?,
        );
    } else {
        let key = russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .map_err(|error| format!("failed to generate SFTP host key: {error}"))?;
        let pem = key
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .map_err(|error| format!("failed to encode SFTP host key: {error}"))?;
        fs::write(&path, pem.as_bytes())
            .await
            .map_err(|error| format!("failed to persist SFTP host key: {error}"))?;
        keys.push(key);
    }
    if let Ok(raw) = fs::read(&rsa_path).await {
        keys.push(
            russh::keys::PrivateKey::from_openssh(raw)
                .map_err(|error| format!("failed to parse persisted RSA host key: {error}"))?,
        );
    } else {
        let key = russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::ssh_key::Algorithm::Rsa { hash: None },
        )
        .map_err(|error| format!("failed to generate RSA host key: {error}"))?;
        let pem = key
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .map_err(|error| format!("failed to encode RSA host key: {error}"))?;
        fs::write(&rsa_path, pem.as_bytes())
            .await
            .map_err(|error| format!("failed to persist RSA host key: {error}"))?;
        keys.push(key);
    }
    Ok(keys)
}
