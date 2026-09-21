use std::{future::Future, pin::Pin};

use tauri::AppHandle;

use crate::{
    state::AppState,
    terminal::{
        domain::{ConnectionCapabilities, ProtocolKind},
        internal::{
            ConnectionOpenResult, ConnectionResult, ResolvedConnection, SshRuntimeMetricsRequest,
        },
    },
};

use super::{
    rdp::RdpProtocolDriver, serial::SerialProtocolDriver, ssh::SshProtocolDriver,
    telnet::TelnetProtocolDriver, vnc::VncProtocolDriver,
};

pub(crate) type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait ProtocolDriver: Send + Sync {
    fn kind(&self) -> ProtocolKind;

    fn capabilities(&self) -> ConnectionCapabilities;

    fn validate(&self, _request: &ResolvedConnection) -> ConnectionResult<()> {
        Ok(())
    }

    fn open<'a>(
        &'a self,
        app: AppHandle,
        state: &'a AppState,
        request: ResolvedConnection,
    ) -> DriverFuture<'a, ConnectionResult<ConnectionOpenResult>>;

    fn start_metrics<'a>(
        &'a self,
        _app: AppHandle,
        _state: &'a AppState,
        _request: SshRuntimeMetricsRequest,
    ) -> DriverFuture<'a, Result<(), String>> {
        Box::pin(async { Err("metrics are not supported for this protocol".to_string()) })
    }

    fn stop_metrics(
        &self,
        _state: &AppState,
        _request: SshRuntimeMetricsRequest,
    ) -> Result<(), String> {
        Err("metrics are not supported for this protocol".to_string())
    }

    fn discard_pending_connection(&self, _connection_id: &str) {}
}

static SSH_DRIVER: SshProtocolDriver = SshProtocolDriver;
static TELNET_DRIVER: TelnetProtocolDriver = TelnetProtocolDriver;
static SERIAL_DRIVER: SerialProtocolDriver = SerialProtocolDriver;
static VNC_DRIVER: VncProtocolDriver = VncProtocolDriver;
// RDP 驱动无论 `rdp` cargo feature 是否开启都注册：feature 关闭时 open 返回
// rdp_not_supported 错误码，前端可据此提示该构建不含 RDP 支持。
static RDP_DRIVER: RdpProtocolDriver = RdpProtocolDriver;
static PROTOCOL_DRIVERS: [&dyn ProtocolDriver; 5] = [
    &SSH_DRIVER,
    &TELNET_DRIVER,
    &SERIAL_DRIVER,
    &VNC_DRIVER,
    &RDP_DRIVER,
];

pub(crate) struct ProtocolRegistry;

impl ProtocolRegistry {
    pub(crate) fn drivers(&self) -> &'static [&'static dyn ProtocolDriver] {
        &PROTOCOL_DRIVERS
    }

    pub(crate) fn driver(&self, protocol: ProtocolKind) -> Option<&'static dyn ProtocolDriver> {
        PROTOCOL_DRIVERS
            .iter()
            .copied()
            .find(|driver| driver.kind() == protocol)
    }

    pub(crate) fn require_driver(&self, protocol: ProtocolKind) -> &'static dyn ProtocolDriver {
        self.driver(protocol)
            .expect("ProtocolKind must have a registered ProtocolDriver")
    }

    pub(crate) fn discard_pending_connections(&self, connection_id: &str) {
        for driver in self.drivers() {
            driver.discard_pending_connection(connection_id);
        }
    }
}

pub(crate) fn protocol_registry() -> ProtocolRegistry {
    ProtocolRegistry
}
