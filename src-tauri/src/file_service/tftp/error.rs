use std::{io, path::Path};

/// Errors that terminate a TFTP transfer.  The variant decides how
/// `serve_session` reports the failure to the peer, so the RFC 1350 error
/// code travels with the error instead of being recovered later by
/// string-prefix matching.
#[derive(Debug)]
pub(super) enum TransferError {
    /// The peer sent an ERROR packet.  Never answer an ERROR with an ERROR
    /// (RFC 1350), so nothing is sent back.
    Peer(String),
    /// The server is shutting down mid-transfer; the peer is not at fault.
    Shutdown,
    /// The peer went silent and retransmits are exhausted; an ERROR packet
    /// would reach nobody, so nothing is sent back.
    Timeout(String),
    /// Any other failure: send an ERROR packet with this code to the peer.
    Send(u16, String),
}

impl TransferError {
    pub(super) fn undefined(message: impl Into<String>) -> Self {
        Self::Send(0, message.into())
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
        Self::Send(1, message.into())
    }

    pub(super) fn access_violation(message: impl Into<String>) -> Self {
        Self::Send(2, message.into())
    }

    pub(super) fn illegal(message: impl Into<String>) -> Self {
        Self::Send(4, message.into())
    }

    pub(super) fn bad_option(message: impl Into<String>) -> Self {
        Self::Send(8, message.into())
    }
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Peer(message) | Self::Timeout(message) | Self::Send(_, message) => {
                formatter.write_str(message)
            }
            Self::Shutdown => formatter.write_str("TFTP server is shutting down"),
        }
    }
}

/// Maps an IO failure from a transfer file operation to the right RFC 1350
/// error code.  Checking `raw_os_error` keeps ENOSPC detection correct across
/// platforms — the OS error message text differs between Windows and Unix.
pub(super) fn io_transfer_error(action: &str, path: &Path, error: io::Error) -> TransferError {
    const ENOSPC: i32 = if cfg!(windows) { 112 } else { 28 };
    if error.raw_os_error() == Some(ENOSPC) {
        TransferError::Send(
            3,
            format!("disk full during {action} of '{}': {error}", path.display()),
        )
    } else {
        TransferError::undefined(format!("failed to {action} '{}': {error}", path.display()))
    }
}
