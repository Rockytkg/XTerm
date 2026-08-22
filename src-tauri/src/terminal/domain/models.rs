use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionCapabilities {
    pub shell: bool,
    pub exec: bool,
    pub subsystem: bool,
    pub sftp: bool,
    pub metrics: bool,
    pub resize: bool,
    pub encoding_detection: bool,
    pub serial_signals: bool,
    pub raw_output: bool,
    pub serial_baud_detection: bool,
}

impl ConnectionCapabilities {
    pub fn ssh() -> Self {
        Self {
            shell: true,
            exec: true,
            subsystem: true,
            sftp: true,
            metrics: true,
            resize: true,
            encoding_detection: true,
            serial_signals: false,
            raw_output: true,
            serial_baud_detection: false,
        }
    }

    pub fn telnet() -> Self {
        Self {
            shell: true,
            exec: false,
            subsystem: false,
            sftp: false,
            metrics: false,
            resize: true,
            encoding_detection: true,
            serial_signals: false,
            raw_output: false,
            serial_baud_detection: false,
        }
    }

    pub fn serial() -> Self {
        Self {
            shell: true,
            exec: false,
            subsystem: false,
            sftp: false,
            metrics: false,
            resize: true,
            encoding_detection: true,
            serial_signals: true,
            raw_output: false,
            serial_baud_detection: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProtocolKind {
    Ssh,
    Telnet,
    Serial,
}

impl ProtocolKind {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ssh" => Some(Self::Ssh),
            "telnet" => Some(Self::Telnet),
            "serial" => Some(Self::Serial),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Telnet => "telnet",
            Self::Serial => "serial",
        }
    }

    pub fn requires_password_credential(&self) -> bool {
        matches!(self, Self::Telnet | Self::Serial)
    }
}

impl fmt::Display for ProtocolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
