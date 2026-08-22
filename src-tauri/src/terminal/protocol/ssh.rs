use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            discard_pending_ssh_connection, run_runtime_metrics_monitor, ConnectionError,
            ConnectionOpenResult, ConnectionResult, ResolvedConnection, SshConnectionFactory,
            SshRuntimeMetricsRequest,
        },
        protocol::common::{DriverFuture, ProtocolDriver},
    },
};

pub(crate) struct SshProtocolDriver;

impl ProtocolDriver for SshProtocolDriver {
    fn kind(&self) -> crate::terminal::domain::ProtocolKind {
        crate::terminal::domain::ProtocolKind::Ssh
    }

    fn capabilities(&self) -> ConnectionCapabilities {
        ConnectionCapabilities::ssh()
    }

    fn validate(&self, request: &ResolvedConnection) -> ConnectionResult<()> {
        if request
            .host
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ConnectionError::validation(
                "ssh_host_required",
                "SSH host is required",
            ));
        }
        if request
            .user
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ConnectionError::validation(
                "ssh_user_required",
                "SSH user is required",
            ));
        }
        Ok(())
    }

    fn open<'a>(
        &'a self,
        app: AppHandle,
        state: &'a AppState,
        request: ResolvedConnection,
    ) -> DriverFuture<'a, ConnectionResult<ConnectionOpenResult>> {
        Box::pin(async move { SshConnectionFactory.open(app, state, request).await })
    }

    fn start_metrics<'a>(
        &'a self,
        app: AppHandle,
        state: &'a AppState,
        request: SshRuntimeMetricsRequest,
    ) -> DriverFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let session_id = request.session_id.clone();
            let connection_id = request.connection_id.clone();
            if state.monitor_task(&session_id).is_some() {
                return Ok(());
            }
            let guard = state.bind_new_monitor_task(&session_id);
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    run_runtime_metrics_monitor(app, connection_id, session_id, guard).await
                {
                    log::debug!(target: "terminal.ssh", "runtime metrics monitor stopped: {error}");
                }
            });
            Ok(())
        })
    }

    fn stop_metrics(
        &self,
        state: &AppState,
        request: SshRuntimeMetricsRequest,
    ) -> Result<(), String> {
        state.remove_monitor_task(&request.session_id);
        Ok(())
    }

    fn discard_pending_connection(&self, connection_id: &str) {
        discard_pending_ssh_connection(connection_id);
    }
}
