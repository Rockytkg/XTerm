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
    pub video: bool,
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
            video: false,
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
            video: false,
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
            video: false,
        }
    }

    pub fn vnc() -> Self {
        Self {
            shell: false,
            exec: false,
            subsystem: false,
            sftp: false,
            metrics: false,
            resize: false,
            encoding_detection: false,
            serial_signals: false,
            raw_output: false,
            serial_baud_detection: false,
            video: true,
        }
    }

    pub fn rdp() -> Self {
        Self {
            video: true,
            // RDP 桌面尺寸可随窗口变化（Display Control DVC 动态分辨率）。
            resize: true,
            ..Self::vnc()
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProtocolKind {
    Ssh,
    Telnet,
    Serial,
    Vnc,
    Rdp,
}

impl ProtocolKind {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ssh" => Some(Self::Ssh),
            "telnet" => Some(Self::Telnet),
            "serial" => Some(Self::Serial),
            "vnc" => Some(Self::Vnc),
            "rdp" => Some(Self::Rdp),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Telnet => "telnet",
            Self::Serial => "serial",
            Self::Vnc => "vnc",
            Self::Rdp => "rdp",
        }
    }

    pub fn requires_password_credential(&self) -> bool {
        matches!(self, Self::Telnet | Self::Serial | Self::Vnc | Self::Rdp)
    }
}

impl fmt::Display for ProtocolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionCapabilities, ProtocolKind};

    #[test]
    fn serial_capabilities_keep_serial_features() {
        let capabilities = ConnectionCapabilities::serial();
        assert!(capabilities.serial_signals);
        assert!(capabilities.serial_baud_detection);
        assert!(!capabilities.video);
    }

    #[test]
    fn vnc_capabilities_are_video_only() {
        let capabilities = ConnectionCapabilities::vnc();
        assert!(capabilities.video);
        assert!(!capabilities.shell);
        assert!(!capabilities.resize);
        assert!(!capabilities.serial_signals);
        assert!(!capabilities.serial_baud_detection);
    }

    #[test]
    fn vnc_is_a_password_credential_protocol() {
        assert!(ProtocolKind::Vnc.requires_password_credential());
        assert_eq!(ProtocolKind::from_str("VNC"), Some(ProtocolKind::Vnc));
        assert_eq!(ProtocolKind::Vnc.as_str(), "vnc");
    }

    #[test]
    fn rdp_capabilities_are_video_with_resize() {
        let capabilities = ConnectionCapabilities::rdp();
        assert!(capabilities.video);
        assert!(capabilities.resize);
        assert!(!capabilities.shell);
        assert!(!capabilities.sftp);
        assert!(!capabilities.serial_signals);
        assert!(!capabilities.serial_baud_detection);
    }

    #[test]
    fn rdp_is_a_password_credential_protocol() {
        assert!(ProtocolKind::Rdp.requires_password_credential());
        assert_eq!(ProtocolKind::from_str("RDP"), Some(ProtocolKind::Rdp));
        assert_eq!(ProtocolKind::Rdp.as_str(), "rdp");
    }
}
