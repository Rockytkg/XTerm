pub(crate) mod api;
pub(crate) mod app;
pub(crate) mod domain;
pub(crate) mod events;
pub(crate) mod internal;
pub(crate) mod protocol;
pub(crate) mod runtime_registry;
pub(crate) use internal::shutdown_all_sessions;

use app::{ConnectionApplicationService, SessionApplicationService};

pub(crate) fn connection_service() -> ConnectionApplicationService {
    ConnectionApplicationService::new()
}

pub(crate) fn session_service() -> SessionApplicationService {
    SessionApplicationService::new()
}
