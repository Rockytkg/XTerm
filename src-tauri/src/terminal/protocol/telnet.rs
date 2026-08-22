use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
            TelnetConnectionFactory,
        },
        protocol::common::{DriverFuture, ProtocolDriver},
    },
};

pub(crate) struct TelnetProtocolDriver;

impl ProtocolDriver for TelnetProtocolDriver {
    fn kind(&self) -> crate::terminal::domain::ProtocolKind {
        crate::terminal::domain::ProtocolKind::Telnet
    }

    fn capabilities(&self) -> ConnectionCapabilities {
        ConnectionCapabilities::telnet()
    }

    fn validate(&self, request: &ResolvedConnection) -> ConnectionResult<()> {
        if request
            .host
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ConnectionError::validation(
                "telnet_host_required",
                "Telnet host is required",
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
        Box::pin(async move { TelnetConnectionFactory.open(app, state, request).await })
    }
}
