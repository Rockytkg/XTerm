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

/// 凭据行的缺省提交终止符（CR）：串口等无 NVT 层的链路直接使用；telnet 工厂
/// 按会话选项覆盖（默认 CR NUL，显式 CRLF 时回到 CR 由引擎编成 CR LF）。
pub(super) const LINE_ENDING_CR: &[u8] = b"\r";
/// CR NUL：RFC 854 对"裸 CR"的合法编码，也是 telnet 会话的默认终止符。部分
/// 设备链路（嵌套 telnet 的中间跳等）把 CR 与 LF 各自算作一次行提交，CR LF
/// 会被执行为两次；CR NUL 整链只有一个终止符，NUL 被任何 NVT 接收方吞掉。
pub(super) const LINE_ENDING_CR_NUL: &[u8] = b"\r\0";

// 刻意不派生 Debug/Clone：凭据含明文密码，只在 worker 内移动、随认证窗口结束
// 被丢弃，不应进入日志或被复制。
pub(super) struct StartupPasswordAuth {
    pub(super) username: Option<String>,
    pub(super) password: String,
    /// 追加在每条凭据行末尾的提交字节；telnet 会话由选项决定（默认 CR NUL）。
    pub(super) line_ending: &'static [u8],
}

pub(super) struct StartupAuthState {
    auth: StartupPasswordAuth,
    phase: StartupAuthPhase,
    /// 登录序列只能由用户推进（见到了用户名提示但未配置用户名）。置位后前端
    /// 输入立即放行——继续暂存会让设备永远等不到用户名；密码提示仍自动应答。
    needs_user: bool,
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
            needs_user: false,
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
        let prompt = strip_ansi_controls(self.prompt_tail())
            .trim()
            .to_ascii_lowercase();
        if looks_like_password_prompt(&prompt) {
            // A password prompt is always answered, in either phase.
            self.phase = StartupAuthPhase::Done;
            return Some(credential_line(&self.auth.password, self.auth.line_ending));
        }
        if self.phase == StartupAuthPhase::AwaitPrompt && looks_like_username_prompt(&prompt) {
            if let Some(username) = self.auth.username.as_deref() {
                self.phase = StartupAuthPhase::AwaitPassword;
                self.deadline = followup_deadline();
                self.scan.clear();
                return Some(credential_line(username, self.auth.line_ending));
            }
            // 密码-only 配置无法代答用户名：流程转为用户驱动，刷新一次截止时间，
            // 让用户输完用户名后出现的密码提示仍能被自动应答。
            self.needs_user = true;
            self.deadline = followup_deadline();
        }
        None
    }

    pub(super) fn is_finished(&self) -> bool {
        self.phase == StartupAuthPhase::Done
    }

    /// 自动登录窗口未关闭时返回其截止时刻。会话工作线程据此暂存前端输入：
    /// 窗口内落线的按键/终端自动应答（DA/CPR 查询响应等）会插在自动填写的
    /// 凭据行前面，把用户名顶成 "xxadmin" 之类导致登录失败；窗口外（登录完成、
    /// 超时、或转入只能由用户推进的阶段）输入立即放行。
    pub(super) fn input_hold_until(&self) -> Option<Instant> {
        if self.phase == StartupAuthPhase::Done || self.needs_user {
            return None;
        }
        let now = Instant::now();
        (self.deadline > now).then_some(self.deadline)
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
            line_ending: LINE_ENDING_CR,
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
        line_ending: LINE_ENDING_CR,
    }))
}

fn credential_line(value: &str, line_ending: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(value.len() + line_ending.len());
    payload.extend_from_slice(value.as_bytes());
    payload.extend_from_slice(line_ending);
    payload
}

/// 提示匹配前剥掉 ANSI 转义：带颜色/光标控制的提示符（如
/// "\x1b[1mPassword:\x1b[0m"）不以 ':' 结尾会整体失配。只处理 CSI
/// （ESC [ … 终止字符）与两字符 ESC 序列，普通文本原样保留。
/// 全程按 char 迭代，多字节字符不会被切碎。
fn strip_ansi_controls(text: &str) -> String {
    if !text.contains('\x1b') {
        return text.to_string();
    }
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            stripped.push(ch);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // CSI 参数/中间字节落在 0x30..=0x3F 与 0x20..=0x2F，终止字符在
            // 0x40..=0x7E；对端发来的畸形序列一直消费到文本结束也无妨。
            for inner in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&inner) {
                    break;
                }
            }
        } else {
            chars.next();
        }
    }
    stripped
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
    use super::{StartupAuthState, StartupPasswordAuth, LINE_ENDING_CR, LINE_ENDING_CR_NUL};

    fn auth(username: Option<&str>, password: &str) -> StartupPasswordAuth {
        StartupPasswordAuth {
            username: username.map(ToOwned::to_owned),
            password: password.to_string(),
            line_ending: LINE_ENDING_CR,
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

    #[test]
    fn unanswerable_username_prompt_releases_input_hold_but_keeps_password_armed() {
        let mut state = StartupAuthState::new(auth(None, "secret"));
        assert!(state.input_hold_until().is_some());

        // 无法代答的用户名提示：流程转为用户驱动，暂存的输入必须立即放行，
        // 否则设备永远等不到用户名；密码提示仍要在窗口内自动应答。
        assert_eq!(state.observe("Username:"), None);
        assert!(state.input_hold_until().is_none());
        assert!(!state.is_finished());

        assert_eq!(
            state.observe("admin\r\nPassword:"),
            Some(b"secret\r".to_vec())
        );
        assert!(state.is_finished());
    }

    #[test]
    fn styled_prompts_match_after_stripping_ansi_controls() {
        let mut state = StartupAuthState::new(auth(Some("admin"), "secret"));

        assert_eq!(
            state.observe("\x1b[1m\x1b[37mUsername:\x1b[0m "),
            Some(b"admin\r".to_vec())
        );
        assert_eq!(
            state.observe("\x1b[32mPassword:\x1b[0m"),
            Some(b"secret\r".to_vec())
        );
        assert!(state.is_finished());
    }

    #[test]
    fn ansi_stripping_tolerates_malformed_and_multibyte_sequences() {
        let mut state = StartupAuthState::new(auth(Some("admin"), "secret"));

        // ESC 后紧跟多字节字符、CSI 缺终止字符：都不能 panic，且不影响匹配。
        assert_eq!(
            state.observe("\x1b中\x1b[31mlogin:"),
            Some(b"admin\r".to_vec())
        );
    }

    #[test]
    fn credential_lines_use_the_configured_line_ending() {
        let mut auth = auth(Some("admin"), "secret");
        auth.line_ending = LINE_ENDING_CR_NUL;
        let mut state = StartupAuthState::new(auth);

        assert_eq!(state.observe("Username:"), Some(b"admin\r\0".to_vec()));
        assert_eq!(state.observe("Password:"), Some(b"secret\r\0".to_vec()));
    }

    #[test]
    fn input_hold_covers_the_whole_auth_window_and_lifts_on_completion() {
        let mut state = StartupAuthState::new(auth(Some("admin"), "secret"));
        assert!(state.input_hold_until().is_some());

        assert_eq!(state.observe("login:"), Some(b"admin\r".to_vec()));
        // 用户名已发、密码未发：窗口仍在，继续暂存输入。
        assert!(state.input_hold_until().is_some());

        assert_eq!(state.observe("Password:"), Some(b"secret\r".to_vec()));
        assert!(state.input_hold_until().is_none());
    }

    #[test]
    fn input_hold_releases_once_auth_is_done() {
        let mut state = StartupAuthState::new(auth(None, "secret"));
        assert!(state.input_hold_until().is_some());
        assert_eq!(state.observe("Password:"), Some(b"secret\r".to_vec()));
        assert!(state.input_hold_until().is_none());
        assert!(state.is_finished());
    }
}
