use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
            VncConnectionFactory,
        },
        protocol::common::{DriverFuture, ProtocolDriver},
    },
};

pub(crate) struct VncProtocolDriver;

impl ProtocolDriver for VncProtocolDriver {
    fn kind(&self) -> crate::terminal::domain::ProtocolKind {
        crate::terminal::domain::ProtocolKind::Vnc
    }

    fn capabilities(&self) -> ConnectionCapabilities {
        ConnectionCapabilities::vnc()
    }

    fn validate(&self, request: &ResolvedConnection) -> ConnectionResult<()> {
        if request
            .host
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ConnectionError::validation(
                "vnc_host_required",
                "VNC host is required",
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
        Box::pin(async move { VncConnectionFactory.open(app, state, request).await })
    }
}
