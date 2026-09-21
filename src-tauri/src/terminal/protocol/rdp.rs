use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection},
        protocol::common::{DriverFuture, ProtocolDriver},
    },
};

pub(crate) struct RdpProtocolDriver;

impl ProtocolDriver for RdpProtocolDriver {
    fn kind(&self) -> crate::terminal::domain::ProtocolKind {
        crate::terminal::domain::ProtocolKind::Rdp
    }

    fn capabilities(&self) -> ConnectionCapabilities {
        ConnectionCapabilities::rdp()
    }

    fn validate(&self, request: &ResolvedConnection) -> ConnectionResult<()> {
        if request
            .host
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ConnectionError::validation(
                "rdp_host_required",
                "RDP host is required",
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
        Box::pin(async move { open_rdp(app, state, request).await })
    }
}

#[cfg(feature = "rdp")]
async fn open_rdp(
    app: AppHandle,
    state: &AppState,
    request: ResolvedConnection,
) -> ConnectionResult<ConnectionOpenResult> {
    crate::terminal::internal::RdpConnectionFactory
        .open(app, state, request)
        .await
}

/// 未启用 `rdp` cargo feature 的构建不包含 IronRDP 依赖；驱动仍注册以便
/// 前端得到明确的错误码，而不是"未知协议"。
#[cfg(not(feature = "rdp"))]
async fn open_rdp(
    _app: AppHandle,
    _state: &AppState,
    _request: ResolvedConnection,
) -> ConnectionResult<ConnectionOpenResult> {
    Err(ConnectionError::new(
        "rdp_not_supported",
        "RDP support is not compiled into this build (cargo feature `rdp` is disabled)",
        false,
    ))
}
