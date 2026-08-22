use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
            SerialConnectionFactory,
        },
        protocol::common::{DriverFuture, ProtocolDriver},
    },
};

pub(crate) struct SerialProtocolDriver;

impl ProtocolDriver for SerialProtocolDriver {
    fn kind(&self) -> crate::terminal::domain::ProtocolKind {
        crate::terminal::domain::ProtocolKind::Serial
    }

    fn capabilities(&self) -> ConnectionCapabilities {
        ConnectionCapabilities::serial()
    }

    fn validate(&self, request: &ResolvedConnection) -> ConnectionResult<()> {
        let port = request.serial_port.as_deref().or(request.host.as_deref());
        if port.is_none_or(|value| value.trim().is_empty()) {
            return Err(ConnectionError::new(
                "serial_port_required",
                "serial port is required",
                false,
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
        Box::pin(async move { SerialConnectionFactory.open(app, state, request).await })
    }
}
