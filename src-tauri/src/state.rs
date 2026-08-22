use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use parking_lot::{Mutex, MutexGuard};
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::watch;

use crate::{
    file_service::FileServiceManager,
    logging,
    proxy::ProxyManager,
    storage::Store,
    terminal::{
        api::dto::TerminalSessionChannelPayload,
        internal::{trzsz::TrzszRuntime, ResolvedConnection, TerminalSession},
        runtime_registry::{TerminalOutputSubscription, TerminalRuntimeRegistry},
    },
};

/// Serialized async handle for a cached SFTP subsystem.
#[derive(Clone)]
pub struct SftpSession {
    inner: Arc<tokio::sync::Mutex<russh_sftp::client::SftpSession>>,
    closed: Arc<AtomicBool>,
}

/// Heuristic for errors that indicate the SFTP/SSH protocol stream is broken
/// (as opposed to ordinary remote operation failures).
fn sftp_error_is_fatal(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    [
        "channel closed",
        "connection closed",
        "broken pipe",
        "disconnect",
        "eof",
        "protocol",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

impl SftpSession {
    pub fn new(sftp: russh_sftp::client::SftpSession) -> Self {
        let session = Self {
            inner: Arc::new(tokio::sync::Mutex::new(sftp)),
            closed: Arc::new(AtomicBool::new(false)),
        };
        session.spawn_keepalive();
        session
    }

    pub async fn run<R, F>(&self, job: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: for<'a> FnOnce(
                &'a russh_sftp::client::SftpSession,
            )
                -> Pin<Box<dyn Future<Output = Result<R, String>> + Send + 'a>>
            + Send
            + 'static,
    {
        if self.is_closed() {
            return Err("SFTP session is closed".to_string());
        }
        let sftp = self.inner.lock().await;
        if self.is_closed() {
            return Err("SFTP session is closed".to_string());
        }
        let result = job(&sftp).await;
        if result.is_err() {
            self.close();
        }
        result
    }

    pub async fn run_with_timeout<R, F>(&self, timeout: Duration, job: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: for<'a> FnOnce(
                &'a russh_sftp::client::SftpSession,
            )
                -> Pin<Box<dyn Future<Output = Result<R, String>> + Send + 'a>>
            + Send
            + 'static,
    {
        if self.is_closed() {
            return Err("SFTP session is closed".to_string());
        }
        // Lock waits are deliberately unbounded: a long transfer holding the
        // session must not fail queued commands. The keepalive loop owns
        // wedged-session detection (see spawn_keepalive).
        let sftp = self.inner.lock().await;
        if self.is_closed() {
            return Err("SFTP session is closed".to_string());
        }
        let result = tokio::time::timeout(timeout, job(&sftp)).await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                // Only errors that indicate a broken protocol stream justify
                // killing the whole session; ordinary remote errors (permits,
                // missing files, ...) must not tear it down.
                if sftp_error_is_fatal(&error) {
                    self.close();
                }
                Err(error)
            }
            Err(_) => {
                // An IO timeout does not prove the session is dead — the
                // keepalive probe makes that call. Closing here would kill
                // healthy sessions on every slow remote.
                Err(format!(
                    "SFTP operation timed out after {} seconds",
                    timeout.as_secs()
                ))
            }
        }
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn spawn_keepalive(&self) {
        let session = self.clone();
        tokio::spawn(async move {
            const SFTP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
            loop {
                tokio::time::sleep(SFTP_KEEPALIVE_INTERVAL).await;
                if session.is_closed() {
                    break;
                }
                let result = {
                    let sftp = session.inner.lock().await;
                    tokio::time::timeout(Duration::from_secs(10), sftp.metadata(".")).await
                };
                match result {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        log::warn!(target: "state.sftp", "SFTP keepalive failed: {error}");
                        session.close();
                        break;
                    }
                    Err(_) => {
                        log::warn!(target: "state.sftp", "SFTP keepalive timed out");
                        session.close();
                        break;
                    }
                }
            }
        });
    }
}

/// Typed connection lifecycle. Serialized with the same lowercase wire values
/// the previous bare strings used ("connecting", "connected", ...).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Failed,
}

impl ConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnecting => "disconnecting",
            Self::Disconnected => "disconnected",
            Self::Failed => "failed",
        }
    }

    /// Legal transition table. Illegal transitions are rejected by the caller
    /// (warn + keep the previous state) so bugs surface in development
    /// without panicking at runtime.
    fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        match (self, next) {
            // A new open attempt may start from any settled state.
            (_, Self::Connecting) => true,
            (
                Self::Connecting,
                Self::Connected | Self::Failed | Self::Disconnecting | Self::Disconnected,
            ) => true,
            (Self::Connected, Self::Disconnecting | Self::Disconnected | Self::Failed) => true,
            (Self::Disconnecting, Self::Disconnected | Self::Failed) => true,
            (Self::Disconnected, Self::Disconnecting) => true,
            (Self::Failed, Self::Disconnecting | Self::Disconnected) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionRuntimeState {
    pub protocol: String,
    pub state: ConnectionStatus,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SessionRuntimeState {
    pub lifecycle_state: String,
    pub active_channel_id: Option<u64>,
    pub next_channel_id: u64,
    pub next_subscription_id: u64,
}

/// Acquires a lock on the given mutex. Because `parking_lot::Mutex` never
/// poisons on panic, this cannot fail.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock()
}

pub struct AppState {
    store: Arc<Mutex<Store>>,
    paths: Mutex<crate::paths::AppPaths>,
    proxy: Mutex<ProxyManager>,
    file_service: Mutex<FileServiceManager>,
    file_service_operation_lock: Arc<tokio::sync::Mutex<()>>,
    terminal: TerminalRuntimeRegistry,
    transient_connections: Mutex<HashMap<String, ResolvedConnection>>,
    connection_runtime: Mutex<HashMap<String, ConnectionRuntimeState>>,
    connection_open_scopes: Mutex<HashMap<String, ConnectionOpenState>>,
    temporary_host_keys: Mutex<HashMap<String, TemporaryHostKeyTrust>>,
    sftp_sessions: Mutex<HashMap<String, SftpSession>>,
    sftp_session_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    trzsz_runtime: Mutex<TrzszRuntime>,
}

struct ConnectionOpenState {
    guard: Arc<()>,
    cancel: watch::Sender<bool>,
}

#[derive(Clone, Debug)]
pub struct ConnectionOpenScope {
    guard: Arc<()>,
    cancel: watch::Receiver<bool>,
}

impl ConnectionOpenScope {
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    fn matches(&self, guard: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.guard, guard)
    }

    pub async fn cancelled(&mut self) {
        while !*self.cancel.borrow_and_update() {
            if self.cancel.changed().await.is_err() {
                break;
            }
        }
    }
}

#[derive(Clone, Debug)]
struct TemporaryHostKeyTrust {
    host: String,
    port: u16,
    fingerprint: String,
}

impl AppState {
    pub fn new(store: Store, paths: crate::paths::AppPaths) -> Self {
        let proxy = ProxyManager::from_store(&store);
        let file_service = FileServiceManager::from_store(&store);
        Self {
            store: Arc::new(Mutex::new(store)),
            paths: Mutex::new(paths),
            proxy: Mutex::new(proxy),
            file_service: Mutex::new(file_service),
            file_service_operation_lock: Arc::new(tokio::sync::Mutex::new(())),
            terminal: TerminalRuntimeRegistry::new(),
            transient_connections: Mutex::new(HashMap::new()),
            connection_runtime: Mutex::new(HashMap::new()),
            connection_open_scopes: Mutex::new(HashMap::new()),
            temporary_host_keys: Mutex::new(HashMap::new()),
            sftp_sessions: Mutex::new(HashMap::new()),
            sftp_session_locks: Mutex::new(HashMap::new()),
            trzsz_runtime: Mutex::new(TrzszRuntime::default()),
        }
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        lock(&self.store)
    }

    pub async fn run_store_blocking<R, F>(&self, operation: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: FnOnce(&mut Store) -> Result<R, String> + Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock();
            operation(&mut store)
        })
        .await
        .map_err(|error| format!("store operation task failed: {error}"))?
    }

    pub fn paths(&self) -> MutexGuard<'_, crate::paths::AppPaths> {
        lock(&self.paths)
    }

    pub fn proxy(&self) -> MutexGuard<'_, ProxyManager> {
        lock(&self.proxy)
    }

    pub fn file_service(&self) -> MutexGuard<'_, FileServiceManager> {
        lock(&self.file_service)
    }

    pub fn file_service_operation_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        self.file_service_operation_lock.clone()
    }

    pub fn sessions(&self) -> MutexGuard<'_, HashMap<String, TerminalSession>> {
        lock(&self.terminal.sessions)
    }

    pub fn remember_transient_connection(&self, connection: ResolvedConnection) {
        logging::event(
            "state.transient_connections",
            "transient_connection.remember",
        )
        .field("connection_id", &connection.id)
        .field("protocol", connection.protocol)
        .maybe_field("host", connection.host.clone())
        .maybe_field("port", connection.port)
        .maybe_field("user", connection.user.clone())
        .debug();
        lock(&self.transient_connections).insert(connection.id.clone(), connection);
    }

    pub fn transient_connection(&self, connection_id: &str) -> Option<ResolvedConnection> {
        lock(&self.transient_connections)
            .get(connection_id)
            .cloned()
    }

    pub fn forget_transient_connection(&self, connection_id: &str) {
        if lock(&self.transient_connections)
            .remove(connection_id)
            .is_some()
        {
            logging::event("state.transient_connections", "transient_connection.forget")
                .field("connection_id", connection_id)
                .debug();
        }
    }

    pub fn monitor_task(&self, session_id: &str) -> Option<Arc<()>> {
        lock(&self.terminal.monitor_tasks).get(session_id).cloned()
    }

    pub fn bind_monitor_task(&self, session_id: &str, guard: Arc<()>) {
        lock(&self.terminal.monitor_tasks).insert(session_id.to_string(), guard);
        logging::event("state.monitor_tasks", "monitor_task.bind")
            .field("session_id", session_id)
            .debug();
    }

    pub fn bind_new_monitor_task(&self, session_id: &str) -> Arc<()> {
        let guard = Arc::new(());
        self.bind_monitor_task(session_id, guard.clone());
        guard
    }

    pub fn remove_monitor_task(&self, session_id: &str) {
        if lock(&self.terminal.monitor_tasks)
            .remove(session_id)
            .is_some()
        {
            logging::event("state.monitor_tasks", "monitor_task.remove")
                .field("session_id", session_id)
                .debug();
        }
    }

    pub fn bind_session_connection(&self, session_id: &str, connection_id: &str) {
        self.terminal
            .bind_session_connection(session_id, connection_id);
        logging::event("state.session_binding", "session_connection.bind")
            .field("session_id", session_id)
            .field("connection_id", connection_id)
            .debug();
    }

    pub fn connection_id_for_session(&self, session_id: &str) -> Option<String> {
        lock(&self.terminal.session_connections)
            .get(session_id)
            .cloned()
    }

    pub fn unbind_session_connection(&self, session_id: &str) -> Option<String> {
        let connection_id = self.terminal.unbind_session_connection(session_id);
        if let Some(connection_id) = &connection_id {
            logging::event("state.session_binding", "session_connection.unbind")
                .field("session_id", session_id)
                .field("connection_id", connection_id)
                .debug();
        }
        connection_id
    }

    pub fn release_session_resources(
        &self,
        session_id: &str,
        session: Option<&TerminalSession>,
    ) -> Option<String> {
        if session.is_some_and(|session| session.capabilities.metrics || session.capabilities.sftp)
        {
            self.remove_monitor_task(session_id);
            crate::terminal::internal::cancel_sftp_transfers_for_session(session_id);
            self.remove_sftp_session(session_id);
        }
        let connection_id = self.unbind_session_connection(session_id);
        if let Some(connection_id) = &connection_id {
            self.remove_temporary_host_key_for_connection(connection_id);
        }
        connection_id
    }

    pub fn set_connection_runtime(
        &self,
        connection_id: &str,
        protocol: impl Into<String>,
        state: ConnectionStatus,
        last_error: Option<String>,
    ) -> Option<ConnectionRuntimeState> {
        let mut runtime = lock(&self.connection_runtime);
        if let Some(existing) = runtime.get(connection_id) {
            if !existing.state.can_transition_to(state) {
                logging::event(
                    "state.connection_runtime",
                    "connection_runtime.transition_rejected",
                )
                .field("connection_id", connection_id)
                .field("from", existing.state.as_str())
                .field("to", state.as_str())
                .warn();
                return Some(existing.clone());
            }
        }
        let previous = runtime.insert(
            connection_id.to_string(),
            ConnectionRuntimeState {
                protocol: protocol.into(),
                state,
                last_error,
            },
        );
        if let Some(current) = runtime.get(connection_id) {
            logging::event("state.connection_runtime", "connection_runtime.set")
                .field("connection_id", connection_id)
                .field("protocol", &current.protocol)
                .field("state", current.state.as_str())
                .maybe_field("last_error", current.last_error.clone())
                .debug();
        }
        previous
    }

    pub fn begin_connection_open(&self, connection_id: &str) -> ConnectionOpenScope {
        let mut scopes = lock(&self.connection_open_scopes);
        if let Some(previous) = scopes.remove(connection_id) {
            let _ = previous.cancel.send(true);
        }
        let (cancel, receiver) = watch::channel(false);
        let guard = Arc::new(());
        scopes.insert(
            connection_id.to_string(),
            ConnectionOpenState {
                guard: guard.clone(),
                cancel,
            },
        );
        logging::event("state.connection_runtime", "connection_open.begin")
            .field("connection_id", connection_id)
            .debug();
        ConnectionOpenScope {
            guard,
            cancel: receiver,
        }
    }

    pub fn monitor_task_matches(&self, session_id: &str, guard: &Arc<()>) -> bool {
        lock(&self.terminal.monitor_tasks)
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(current, guard))
    }

    pub fn connection_open_matches(
        &self,
        connection_id: &str,
        scope: &ConnectionOpenScope,
    ) -> bool {
        lock(&self.connection_open_scopes)
            .get(connection_id)
            .is_some_and(|current| !*current.cancel.borrow() && scope.matches(&current.guard))
    }

    pub fn current_connection_open_scope(
        &self,
        connection_id: &str,
    ) -> Option<ConnectionOpenScope> {
        lock(&self.connection_open_scopes)
            .get(connection_id)
            .filter(|scope| !*scope.cancel.borrow())
            .map(|scope| ConnectionOpenScope {
                guard: scope.guard.clone(),
                cancel: scope.cancel.subscribe(),
            })
    }

    pub fn finish_connection_open(&self, connection_id: &str, scope: &ConnectionOpenScope) {
        let mut scopes = lock(&self.connection_open_scopes);
        let removed = scopes
            .get(connection_id)
            .is_some_and(|current| scope.matches(&current.guard));
        if removed {
            scopes.remove(connection_id);
            logging::event("state.connection_runtime", "connection_open.finish")
                .field("connection_id", connection_id)
                .debug();
        }
    }

    pub fn cancel_connection_open(&self, connection_id: &str) {
        let mut scopes = lock(&self.connection_open_scopes);
        let previous = scopes.remove(connection_id);
        if let Some(previous) = previous.as_ref() {
            let _ = previous.cancel.send(true);
        }
        logging::event("state.connection_runtime", "connection_open.cancel")
            .field("connection_id", connection_id)
            .field("cancelled", previous.is_some())
            .debug();
    }

    pub fn connection_runtime(&self, connection_id: &str) -> Option<ConnectionRuntimeState> {
        lock(&self.connection_runtime).get(connection_id).cloned()
    }

    pub fn set_session_lifecycle(
        &self,
        session_id: &str,
        lifecycle_state: impl Into<String>,
    ) -> Option<SessionRuntimeState> {
        let mut runtime = lock(&self.terminal.session_runtime);
        let next_channel_id = runtime
            .get(session_id)
            .map(|value| value.next_channel_id)
            .unwrap_or(1);
        let next_subscription_id = runtime
            .get(session_id)
            .map(|value| value.next_subscription_id)
            .unwrap_or(1);
        let active_channel_id = runtime
            .get(session_id)
            .and_then(|value| value.active_channel_id);
        let previous = runtime.insert(
            session_id.to_string(),
            SessionRuntimeState {
                lifecycle_state: lifecycle_state.into(),
                active_channel_id,
                next_channel_id,
                next_subscription_id,
            },
        );
        if let Some(current) = runtime.get(session_id) {
            logging::event("state.session_runtime", "session_lifecycle.set")
                .field("session_id", session_id)
                .field("lifecycle_state", &current.lifecycle_state)
                .maybe_field("active_channel_id", current.active_channel_id)
                .field("next_channel_id", current.next_channel_id)
                .debug();
        }
        previous
    }

    pub fn session_runtime(&self, session_id: &str) -> Option<SessionRuntimeState> {
        lock(&self.terminal.session_runtime)
            .get(session_id)
            .cloned()
    }

    /// Atomically returns the active channel or allocates a new one under the
    /// same lock. The `bool` is true when an already-active channel was
    /// returned, so concurrent attaches cannot both allocate fresh channels.
    pub fn activate_session_channel(&self, session_id: &str) -> (u64, bool) {
        let mut runtime = lock(&self.terminal.session_runtime);
        let entry = runtime
            .entry(session_id.to_string())
            .or_insert(SessionRuntimeState {
                lifecycle_state: "pending".to_string(),
                active_channel_id: None,
                next_channel_id: 1,
                next_subscription_id: 1,
            });
        if let Some(active_channel_id) = entry.active_channel_id {
            return (active_channel_id, true);
        }
        let channel_id = entry.next_channel_id;
        entry.next_channel_id = entry.next_channel_id.saturating_add(1).max(1);
        entry.active_channel_id = Some(channel_id);
        logging::event("state.session_runtime", "session_channel.activate")
            .field("session_id", session_id)
            .field("channel_id", channel_id)
            .debug();
        (channel_id, false)
    }

    pub fn deactivate_session_channel(
        &self,
        session_id: &str,
        channel_id: Option<u64>,
    ) -> Option<u64> {
        let mut runtime = lock(&self.terminal.session_runtime);
        let entry = runtime.get_mut(session_id)?;
        if channel_id.is_some() && entry.active_channel_id != channel_id {
            return entry.active_channel_id;
        }
        let previous = entry.active_channel_id.take();
        logging::event("state.session_runtime", "session_channel.deactivate")
            .field("session_id", session_id)
            .maybe_field("channel_id", channel_id)
            .maybe_field("previous_channel_id", previous)
            .debug();
        previous
    }

    pub fn session_ids_for_connection(&self, connection_id: &str) -> Vec<String> {
        self.terminal.session_ids_for_connection(connection_id)
    }

    pub fn serial_sessions_for_ports(&self, ports: &[String]) -> Vec<(String, TerminalSession)> {
        let normalized_ports: Vec<String> = ports
            .iter()
            .map(|port| port.trim().to_ascii_lowercase())
            .filter(|port| !port.is_empty())
            .collect();
        if normalized_ports.is_empty() {
            return Vec::new();
        }

        let sessions = lock(&self.terminal.sessions);
        sessions
            .iter()
            .filter(|(_session_id, session)| {
                session
                    .resources
                    .serial_port()
                    .map(|port| {
                        normalized_ports
                            .iter()
                            .any(|candidate| candidate == &port.to_ascii_lowercase())
                    })
                    .unwrap_or(false)
            })
            .map(|(session_id, session)| (session_id.clone(), session.clone()))
            .collect()
    }

    pub fn trust_host_key_once_for_connection(
        &self,
        connection_id: &str,
        host: &str,
        port: u16,
        fingerprint: &str,
    ) {
        lock(&self.temporary_host_keys).insert(
            connection_id.to_string(),
            TemporaryHostKeyTrust {
                host: host.to_string(),
                port,
                fingerprint: fingerprint.to_string(),
            },
        );
    }

    pub fn has_host_key_trusted_once_for_connection(
        &self,
        connection_id: &str,
        host: &str,
        port: u16,
    ) -> bool {
        lock(&self.temporary_host_keys)
            .get(connection_id)
            .is_some_and(|stored| stored.host == host && stored.port == port)
    }

    pub fn trusted_once_fingerprint_for_connection(
        &self,
        connection_id: &str,
        host: &str,
        port: u16,
    ) -> Option<String> {
        lock(&self.temporary_host_keys)
            .get(connection_id)
            .filter(|stored| stored.host == host && stored.port == port)
            .map(|stored| stored.fingerprint.clone())
    }

    pub fn remove_temporary_host_key_for_connection(&self, connection_id: &str) {
        lock(&self.temporary_host_keys).remove(connection_id);
    }

    pub fn sftp_session(&self, session_id: &str) -> Option<SftpSession> {
        lock(&self.sftp_sessions).get(session_id).cloned()
    }

    pub fn sftp_session_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.sftp_session_locks)
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub fn bind_sftp_session(&self, session_id: &str, session: SftpSession) {
        lock(&self.sftp_sessions).insert(session_id.to_string(), session);
        logging::event("state.sftp", "sftp_session.bind")
            .field("session_id", session_id)
            .debug();
    }

    pub fn remove_sftp_session(&self, session_id: &str) {
        lock(&self.sftp_session_locks).remove(session_id);
        self.invalidate_sftp_session(session_id);
    }

    pub(crate) fn invalidate_sftp_session(&self, session_id: &str) {
        if let Some(session) = lock(&self.sftp_sessions).remove(session_id) {
            session.close();
            logging::event("state.sftp", "sftp_session.invalidate")
                .field("session_id", session_id)
                .debug();
        }
    }

    pub fn reserve_terminal_output_subscription(
        &self,
        session_id: &str,
        channel: Channel<TerminalSessionChannelPayload>,
    ) -> u64 {
        let subscription_id = {
            let mut runtime = lock(&self.terminal.session_runtime);
            let entry = runtime
                .entry(session_id.to_string())
                .or_insert(SessionRuntimeState {
                    lifecycle_state: "pending".to_string(),
                    active_channel_id: None,
                    next_channel_id: 1,
                    next_subscription_id: 1,
                });
            let id = entry.next_subscription_id;
            entry.next_subscription_id = entry.next_subscription_id.saturating_add(1).max(1);
            id
        };
        logging::event(
            "state.terminal_output",
            "terminal_output_subscription.reserve",
        )
        .field("session_id", session_id)
        .field("subscription_id", subscription_id)
        .debug();
        lock(&self.terminal.output_subscriptions)
            .entry(session_id.to_string())
            .or_default()
            .insert(
                subscription_id,
                TerminalOutputSubscription {
                    session_id: session_id.to_string(),
                    channel_id: None,
                    channel,
                },
            );
        subscription_id
    }

    pub fn bind_terminal_output_lease(
        &self,
        session_id: &str,
        subscription_id: u64,
        channel_id: u64,
    ) -> bool {
        let assigned = lock(&self.terminal.output_subscriptions)
            .get_mut(session_id)
            .and_then(|session_channels| session_channels.get_mut(&subscription_id))
            .map(|subscription| {
                subscription.channel_id = Some(channel_id);
            })
            .is_some();
        if assigned {
            logging::event("state.terminal_output", "terminal_output_lease.bind")
                .field("session_id", session_id)
                .field("subscription_id", subscription_id)
                .field("channel_id", channel_id)
                .debug();
        }
        assigned
    }

    pub fn release_terminal_output_subscription(&self, session_id: &str, subscription_id: u64) {
        let mut channels = lock(&self.terminal.output_subscriptions);
        let removed = channels
            .get_mut(session_id)
            .and_then(|session_channels| session_channels.remove(&subscription_id));
        if channels
            .get(session_id)
            .is_some_and(|session_channels| session_channels.is_empty())
        {
            channels.remove(session_id);
        }
        if removed.is_some() {
            logging::event(
                "state.terminal_output",
                "terminal_output_subscription.release",
            )
            .field("session_id", session_id)
            .field("subscription_id", subscription_id)
            .debug();
        }
    }

    pub fn remove_terminal_output_channels(&self, session_id: &str) {
        if lock(&self.terminal.output_subscriptions)
            .remove(session_id)
            .is_some()
        {
            logging::event(
                "state.terminal_output",
                "terminal_output_channels.remove_session",
            )
            .field("session_id", session_id)
            .debug();
        }
    }

    pub fn send_terminal_output(
        &self,
        session_id: &str,
        payload: TerminalSessionChannelPayload,
    ) -> usize {
        let targets = {
            let channels = lock(&self.terminal.output_subscriptions);
            channels
                .get(session_id)
                .map(|session_channels| {
                    session_channels
                        .iter()
                        .filter(|(_, subscription)| subscription.accepts(&payload))
                        .map(|(subscription_id, subscription)| {
                            (*subscription_id, subscription.clone())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        if targets.is_empty() {
            return 0;
        }

        let mut delivered = 0_usize;
        let mut failed = Vec::new();
        for (subscription_id, subscription) in targets {
            match subscription.send(payload.clone()) {
                Ok(()) => {
                    delivered = delivered.saturating_add(1);
                }
                Err(error) => {
                    logging::event(
                        "state.terminal_output",
                        "terminal_output_channel.send_failed",
                    )
                    .field("session_id", session_id)
                    .field("subscription_id", subscription_id)
                    .field("error", error.to_string())
                    .warn();
                    failed.push(subscription_id);
                }
            }
        }

        if !failed.is_empty() {
            let mut channels = lock(&self.terminal.output_subscriptions);
            if let Some(session_channels) = channels.get_mut(session_id) {
                for subscription_id in failed {
                    session_channels.remove(&subscription_id);
                }
                if session_channels.is_empty() {
                    channels.remove(session_id);
                }
            }
        };
        delivered
    }

    pub fn trzsz_runtime(&self) -> MutexGuard<'_, TrzszRuntime> {
        lock(&self.trzsz_runtime)
    }
}

#[cfg(test)]
mod tests {
    use super::ConnectionStatus;

    #[test]
    fn connection_status_serializes_to_legacy_wire_values() {
        for (status, wire) in [
            (ConnectionStatus::Connecting, "connecting"),
            (ConnectionStatus::Connected, "connected"),
            (ConnectionStatus::Disconnecting, "disconnecting"),
            (ConnectionStatus::Disconnected, "disconnected"),
            (ConnectionStatus::Failed, "failed"),
        ] {
            assert_eq!(status.as_str(), wire);
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
        }
    }

    #[test]
    fn happy_path_transitions_are_allowed() {
        let path = [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnecting,
            ConnectionStatus::Disconnected,
        ];
        for pair in path.windows(2) {
            assert!(pair[0].can_transition_to(pair[1]), "{pair:?}");
        }
        assert!(ConnectionStatus::Connecting.can_transition_to(ConnectionStatus::Failed));
        assert!(ConnectionStatus::Connected.can_transition_to(ConnectionStatus::Failed));
        assert!(ConnectionStatus::Connected.can_transition_to(ConnectionStatus::Disconnected));
    }

    #[test]
    fn reopen_is_allowed_from_any_state() {
        for status in [
            ConnectionStatus::Connecting,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnecting,
            ConnectionStatus::Disconnected,
            ConnectionStatus::Failed,
        ] {
            assert!(status.can_transition_to(ConnectionStatus::Connecting));
            assert!(status.can_transition_to(status));
        }
    }

    #[test]
    fn backwards_and_skipped_transitions_are_rejected() {
        assert!(!ConnectionStatus::Disconnected.can_transition_to(ConnectionStatus::Connected));
        assert!(!ConnectionStatus::Failed.can_transition_to(ConnectionStatus::Connected));
        assert!(!ConnectionStatus::Disconnecting.can_transition_to(ConnectionStatus::Connected));
        assert!(!ConnectionStatus::Disconnected.can_transition_to(ConnectionStatus::Failed));
    }
}
