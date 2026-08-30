use serde::{Deserialize, Serialize};

use crate::terminal::{
    domain::{ConnectionCapabilities, ProtocolKind},
    internal::SerialProbeResult,
};
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalEventEnvelope<T: Serialize> {
    pub r#type: &'static str,
    pub name: String,
    pub payload: T,
}

impl<T: Serialize> TerminalEventEnvelope<T> {
    pub fn new(name: impl Into<String>, payload: T) -> Self {
        Self {
            r#type: "event",
            name: name.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionOpenCommand {
    pub connection_id: String,
    #[serde(default)]
    pub open_request_id: Option<String>,
    #[serde(default)]
    pub trust_host_key: Option<bool>,
    #[serde(default)]
    pub accept_host_key_once: Option<bool>,
    #[serde(default)]
    pub terminal_scrollback: Option<u32>,
    #[serde(default)]
    pub terminal_type: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub realtime_encoding_detection: Option<bool>,
    #[serde(default)]
    pub cols: Option<u32>,
    #[serde(default)]
    pub rows: Option<u32>,
    #[serde(default)]
    pub ssh_credential: Option<SshCredentialOverride>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub(crate) enum ConnectionOpenResponse {
    Connected {
        connection_id: String,
        session_id: String,
        protocol: ProtocolKind,
        capabilities: ConnectionCapabilities,
        serial_port: Option<String>,
        baud_rate: Option<u32>,
        serial_scores: Option<Vec<SerialProbeResult>>,
    },
    HostKeyChallenge {
        awaiting: &'static str,
        connection_id: String,
        protocol: ProtocolKind,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionAuthenticateCommand {
    pub connection_id: String,
    #[serde(default)]
    pub open_request_id: Option<String>,
    #[serde(default)]
    pub trust_host_key: Option<bool>,
    #[serde(default)]
    pub accept_host_key_once: Option<bool>,
    #[serde(default)]
    pub terminal_scrollback: Option<u32>,
    #[serde(default)]
    pub cols: Option<u32>,
    #[serde(default)]
    pub rows: Option<u32>,
    #[serde(default)]
    pub ssh_credential: Option<SshCredentialOverride>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "authMethod")]
pub(crate) enum SshCredentialOverride {
    Password {
        #[serde(default)]
        username: Option<String>,
        password: String,
    },
    Key {
        #[serde(default)]
        username: Option<String>,
        private_key: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionCloseCommand {
    pub connection_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionOpenCancelCommand {
    pub connection_id: String,
    #[serde(default)]
    pub open_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCloseCommand {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionEncodingDetectionCommand {
    pub session_id: String,
    #[serde(default)]
    pub channel_id: Option<u64>,
    pub enabled: bool,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSerialRedetectCommand {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMetricsEnabledCommand {
    pub session_id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionDetachCommand {
    pub session_id: String,
    pub channel_id: Option<u64>,
    pub subscription_id: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionAttachResult {
    pub accepted: bool,
    pub session_id: String,
    pub connection_id: String,
    pub channel_id: u64,
    pub already_active: bool,
    pub subscription_id: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum TerminalClientFrame {
    #[serde(rename = "terminal.input.text")]
    InputText {
        session_id: String,
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data: String,
    },
    #[serde(rename = "terminal.input.bytes")]
    InputBytes {
        session_id: String,
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data_base64: String,
    },
    #[serde(rename = "terminal.resize")]
    Resize {
        session_id: String,
        channel_id: Option<u64>,
        cols: u32,
        rows: u32,
        width_px: Option<u32>,
        height_px: Option<u32>,
    },
    #[serde(rename = "terminal.raw_output")]
    RawOutput {
        session_id: String,
        channel_id: Option<u64>,
        enabled: bool,
    },
    #[serde(rename = "renderedOffset")]
    RenderedOffset {
        session_id: String,
        channel_id: u64,
        offset: u64,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum TerminalSessionChannelPayload {
    Text {
        connection_id: String,
        session_id: String,
        channel_id: u64,
        data: String,
        encoding: String,
        start_offset: usize,
        end_offset: usize,
    },
    Bytes {
        connection_id: String,
        session_id: String,
        channel_id: u64,
        data_base64: String,
        encoding: String,
        start_offset: usize,
        end_offset: usize,
    },
}

impl TerminalSessionChannelPayload {
    pub(crate) fn session_id(&self) -> &str {
        match self {
            Self::Text { session_id, .. } | Self::Bytes { session_id, .. } => session_id,
        }
    }

    pub(crate) fn channel_id(&self) -> u64 {
        match self {
            Self::Text { channel_id, .. } | Self::Bytes { channel_id, .. } => *channel_id,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionStatusChangedPayload {
    pub connection_id: String,
    pub session_id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionHostKeyChallengePayload {
    pub connection_id: String,
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionHostKeyChallengePayload, TerminalClientFrame, TerminalSessionChannelPayload,
    };
    use serde_json::json;

    #[test]
    fn terminal_client_frame_accepts_camel_case_fields() {
        let frame: TerminalClientFrame = serde_json::from_value(json!({
            "type": "terminal.input.bytes",
            "sessionId": "session-1",
            "channelId": 7,
            "dataBase64": "YWJj"
        }))
        .expect("frame should deserialize from frontend payload");

        match frame {
            TerminalClientFrame::InputBytes {
                session_id,
                channel_id,
                input_sequence,
                data_base64,
            } => {
                assert_eq!(session_id, "session-1");
                assert_eq!(channel_id, Some(7));
                assert_eq!(input_sequence, None);
                assert_eq!(data_base64, "YWJj");
            }
            _ => panic!("unexpected frame variant"),
        }
    }

    #[test]
    fn terminal_output_payload_serializes_camel_case_fields() {
        let payload = TerminalSessionChannelPayload::Text {
            connection_id: "connection-1".to_string(),
            session_id: "session-1".to_string(),
            channel_id: 7,
            data: "abc".to_string(),
            encoding: "utf-8".to_string(),
            start_offset: 0,
            end_offset: 3,
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["kind"], "text");
        assert_eq!(value["connectionId"], "connection-1");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["channelId"], 7);
        assert_eq!(value["startOffset"], 0);
        assert_eq!(value["endOffset"], 3);
    }

    #[test]
    fn rendered_offset_frame_accepts_contract_fields() {
        let frame: TerminalClientFrame = serde_json::from_value(json!({
            "type": "renderedOffset",
            "sessionId": "session-1",
            "channelId": 7,
            "offset": 12345
        }))
        .expect("renderedOffset frame should deserialize from frontend payload");

        match frame {
            TerminalClientFrame::RenderedOffset {
                session_id,
                channel_id,
                offset,
            } => {
                assert_eq!(session_id, "session-1");
                assert_eq!(channel_id, 7);
                assert_eq!(offset, 12345);
            }
            _ => panic!("unexpected frame variant"),
        }
    }

    #[test]
    fn host_key_challenge_carries_the_open_request_session() {
        let payload = ConnectionHostKeyChallengePayload {
            connection_id: "connection-1".to_string(),
            session_id: "pending-session-1".to_string(),
            host: "example.com".to_string(),
            port: 22,
            algorithm: "ssh-ed25519".to_string(),
            fingerprint: "SHA256:test".to_string(),
        };

        let value = serde_json::to_value(payload).expect("challenge should serialize");
        assert_eq!(value["connectionId"], "connection-1");
        assert_eq!(value["sessionId"], "pending-session-1");
    }
}
