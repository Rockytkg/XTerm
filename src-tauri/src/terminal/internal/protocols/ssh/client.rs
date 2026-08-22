use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use parking_lot::Mutex;
use ring::digest;

use super::{
    core::{SessionTransportRuntime, SessionWorkerEvent, TerminalSize, TransportCommand},
    ssh_transport::spawn_ssh_transport_actor,
};

pub(crate) type SharedSshSession = Arc<tokio::sync::Mutex<russh::client::Handle<RusshClient>>>;

pub(super) struct SshShellTransport {
    pub(super) session: SharedSshSession,
    pub(super) channel: russh::Channel<russh::client::Msg>,
}

pub(super) struct SshSessionTransport {
    pub(super) transport: SshShellTransport,
    pub(super) initial_size: TerminalSize,
}

impl SessionTransportRuntime for SshSessionTransport {
    fn initial_size(&self) -> Option<TerminalSize> {
        Some(self.initial_size)
    }

    fn supports_raw_bytes(&self) -> bool {
        true
    }

    fn spawn(
        self: Box<Self>,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
        event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    ) {
        spawn_ssh_transport_actor(session_id, self.transport, rx, event_tx);
    }
}

#[derive(Clone)]
pub struct RusshClient {
    pub(super) host_key: Arc<Mutex<Option<RusshHostKey>>>,
}

#[derive(Clone, Debug)]
pub(super) struct RusshHostKey {
    pub(super) algorithm: String,
    pub(super) fingerprint: String,
}

impl RusshClient {
    pub(super) fn new(host_key: Arc<Mutex<Option<RusshHostKey>>>) -> Self {
        Self { host_key }
    }
}

impl russh::client::Handler for RusshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let algorithm = server_public_key.algorithm().to_string();
        use russh::keys::PublicKeyBase64;

        let digest = digest::digest(&digest::SHA256, &server_public_key.public_key_bytes());
        let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(digest.as_ref()));
        let mut host_key = self.host_key.lock();
        *host_key = Some(RusshHostKey {
            algorithm,
            fingerprint,
        });
        Ok(true)
    }
}
