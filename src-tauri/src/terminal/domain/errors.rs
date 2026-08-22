use serde::Serialize;

use crate::terminal::internal::ConnectionError;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalApiError {
    pub error_code: String,
    pub message: String,
    pub recoverable: bool,
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

impl TerminalApiError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            error_code: "INVALID_REQUEST".to_string(),
            message: message.into(),
            recoverable: false,
            detail: None,
            args: None,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            error_code: "UNSUPPORTED_OPERATION".to_string(),
            message: message.into(),
            recoverable: false,
            detail: None,
            args: None,
        }
    }
}

impl From<String> for TerminalApiError {
    fn from(value: String) -> Self {
        Self {
            error_code: "INTERNAL_ERROR".to_string(),
            message: value.clone(),
            recoverable: true,
            detail: Some(value),
            args: None,
        }
    }
}

impl From<ConnectionError> for TerminalApiError {
    fn from(value: ConnectionError) -> Self {
        Self {
            error_code: value.code.to_string(),
            message: String::new(),
            recoverable: value.retryable,
            detail: Some(value.detail),
            args: value.args,
        }
    }
}
