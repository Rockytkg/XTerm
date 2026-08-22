use std::time::{Duration, Instant};

use crate::{
    credentials::credential_secret_by_id,
    state::AppState,
    terminal::internal::core::{
        ConnectionError, ResolvedConnection, STARTUP_AUTH_FOLLOWUP_TIMEOUT_MS,
    },
};

/// Bounded tail of recent session output scanned for login/password prompts.
/// Peers may print banners first or split a prompt across transport reads, so
/// detection cannot rely on individual output chunks.
const PROMPT_SCAN_BUFFER_BYTES: usize = 512;

#[derive(Clone, Debug)]
pub(super) struct StartupPasswordAuth {
    pub(super) username: Option<String>,
    pub(super) password: String,
}

#[derive(Debug)]
pub(super) struct StartupAuthState {
    auth: StartupPasswordAuth,
    phase: StartupAuthPhase,
    scan: String,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupAuthPhase {
    AwaitPrompt,
    AwaitPassword,
    Done,
}

impl StartupAuthState {
    pub(super) fn new(auth: StartupPasswordAuth) -> Self {
        Self {
            auth,
            phase: StartupAuthPhase::AwaitPrompt,
            scan: String::new(),
            deadline: followup_deadline(),
        }
    }

    /// Observes decoded session output and returns the credential line to write
    /// once a login/password prompt is recognized. Keeps waiting until a prompt
    /// matches or the deadline passes; once finished it stays finished.
    pub(super) fn observe(&mut self, text: &str) -> Option<Vec<u8>> {
        if self.phase == StartupAuthPhase::Done {
            return None;
        }
        if Instant::now() > self.deadline {
            self.phase = StartupAuthPhase::Done;
            return None;
        }
        self.push_scan(text);
        let prompt = self.prompt_tail().to_ascii_lowercase();
        if looks_like_password_prompt(&prompt) {
            // A password prompt is always answered, in either phase.
            self.phase = StartupAuthPhase::Done;
            return Some(credential_line(&self.auth.password));
        }
        if self.phase == StartupAuthPhase::AwaitPrompt && looks_like_username_prompt(&prompt) {
            if let Some(username) = self.auth.username.as_deref() {
                self.phase = StartupAuthPhase::AwaitPassword;
                self.deadline = followup_deadline();
                self.scan.clear();
                return Some(credential_line(username));
            }
        }
        None
    }

    pub(super) fn is_finished(&self) -> bool {
        self.phase == StartupAuthPhase::Done
    }

    fn push_scan(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.scan.push_str(text);
        if self.scan.len() > PROMPT_SCAN_BUFFER_BYTES {
            let mut start = self.scan.len() - PROMPT_SCAN_BUFFER_BYTES;
            while !self.scan.is_char_boundary(start) {
                start += 1;
            }
            self.scan.drain(..start);
        }
    }

    fn prompt_tail(&self) -> &str {
        let trimmed = self.scan.trim_end_matches(['\0', '\r', '\n', ' ']);
        let tail_start = trimmed
            .rfind(['\r', '\n'])
            .map(|index| index + 1)
            .unwrap_or(0);
        trimmed[tail_start..].trim()
    }
}

fn followup_deadline() -> Instant {
    Instant::now() + Duration::from_millis(STARTUP_AUTH_FOLLOWUP_TIMEOUT_MS)
}

pub(super) fn resolve_startup_password_auth(
    state: &AppState,
    request: &ResolvedConnection,
    error_code: &'static str,
) -> Result<Option<StartupPasswordAuth>, ConnectionError> {
    resolve_startup_password_auth_inner(state, request).map_err(|error| {
        ConnectionError::with_args(
            error_code,
            error.clone(),
            serde_json::json!({ "detail": error }),
            false,
        )
    })
}

fn resolve_startup_password_auth_inner(
    state: &AppState,
    request: &ResolvedConnection,
) -> Result<Option<StartupPasswordAuth>, String> {
    if !request.protocol.requires_password_credential() {
        return Ok(None);
    }

    let user = request
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    // Inline password from a deep-link URI takes priority.
    if let Some(password) = request.inline_password.as_deref().filter(|v| !v.is_empty()) {
        return Ok(Some(StartupPasswordAuth {
            username: user,
            password: password.to_string(),
        }));
    }

    // Fall back to saved credential.
    let Some(credential_id) = request
        .saved_credential_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let secret = credential_secret_by_id(state, credential_id)?
        .ok_or_else(|| format!("selected credential '{credential_id}' does not exist"))?;
    if secret.cred_type() != "password" {
        return Err(format!(
            "saved credential '{credential_id}' cannot be used for {} startup authentication",
            request.protocol
        ));
    }
    let password = secret
        .password()
        .ok_or_else(|| "saved password credential is missing its password".to_string())?
        .to_string();
    if password.is_empty() {
        return Ok(None);
    }
    Ok(Some(StartupPasswordAuth {
        username: user,
        password,
    }))
}

fn credential_line(value: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(value.len() + 1);
    payload.extend_from_slice(value.as_bytes());
    payload.push(b'\r');
    payload
}

fn looks_like_password_prompt(prompt: &str) -> bool {
    prompt.ends_with(':')
        && (prompt.contains("password")
            || prompt.contains("passcode")
            || prompt.contains("pass phrase")
            || prompt.contains("passphrase"))
}

fn looks_like_username_prompt(prompt: &str) -> bool {
    prompt.ends_with(':')
        && (prompt.contains("login")
            || prompt.contains("username")
            || prompt.contains("user name")
            || prompt == "user:")
}

#[cfg(test)]
mod tests {
    use super::{StartupAuthState, StartupPasswordAuth};

    fn auth(username: Option<&str>, password: &str) -> StartupPasswordAuth {
        StartupPasswordAuth {
            username: username.map(ToOwned::to_owned),
            password: password.to_string(),
        }
    }

    #[test]
    fn username_prompt_transitions_to_password_prompt() {
        let mut state = StartupAuthState::new(auth(Some("admin"), "secret"));

        assert_eq!(state.observe("login:"), Some(b"admin\r".to_vec()));
        assert!(!state.is_finished());
        assert_eq!(
            state.observe("admin\r\nPassword:"),
            Some(b"secret\r".to_vec())
        );
        assert!(state.is_finished());
        assert_eq!(state.observe("Password:"), None);
    }

    #[test]
    fn password_prompt_can_be_answered_directly() {
        let mut state = StartupAuthState::new(auth(None, "secret"));

        assert_eq!(state.observe("Password: "), Some(b"secret\r".to_vec()));
        assert!(state.is_finished());
    }

    #[test]
    fn banner_before_prompt_does_not_abort_detection() {
        let mut state = StartupAuthState::new(auth(Some("admin"), "secret"));

        assert_eq!(state.observe("Welcome to the device\r\n"), None);
        assert!(!state.is_finished());
        assert_eq!(state.observe("login:"), Some(b"admin\r".to_vec()));
    }

    #[test]
    fn prompt_split_across_chunks_still_matches() {
        let mut state = StartupAuthState::new(auth(None, "secret"));

        assert_eq!(state.observe("Pass"), None);
        assert_eq!(state.observe("word:"), Some(b"secret\r".to_vec()));
    }

    #[test]
    fn login_prompt_without_username_keeps_waiting_for_password() {
        let mut state = StartupAuthState::new(auth(None, "secret"));

        assert_eq!(state.observe("login:"), None);
        assert!(!state.is_finished());
        assert_eq!(state.observe("\r\nPassword:"), Some(b"secret\r".to_vec()));
    }
}
