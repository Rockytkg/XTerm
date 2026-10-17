//! 退格擦除字符的自适应探测（串口 / Telnet 共用）。
//!
//! 背景：擦除键在线上应发什么字节没有统一标准。VT100 硬件的 BACKSPACE 键发
//! BS(0x08)，X11 xterm 惯例发 DEL(0x7f)；网络设备 CLI 多按 VT100/HyperTerminal
//! 惯例只认 0x08，而 Linux tty 默认 VERASE=^? 只认 0x7f。串口没有任何协商机制，
//! Telnet 也只有 LINEMODE SLC（RFC 1184）一种权威来源，且设备服务器基本不实现。
//!
//! 这里利用的是协议之外仍然成立的机制：回显。行编辑发生在对端，擦除是否被接受
//! 一定会体现在回显里（BS SP BS / 应用层重绘 / 脱字符回显 / 沉默）。AUTO 模式
//! 先发 xterm 原生 DEL 候选并按回显收敛；候选被明确拒绝（脱字符回显，或回显活
//! 跃下的沉默）时透明补发另一候选——设备上首次退格延迟一个探测窗即完成学习，
//! 之后的按键按锁定值改写。
//!
//! 探测与改写只作用于单字节按键分块（[0x7f] / [0x08]）：粘贴、脚本批量输入与
//! trzsz 数据里的 0x7f 不是按键，一律直通，避免把批量数据里的 DEL 字节改写成
//! 会被对端解释成擦除的 0x08。

use std::time::{Duration, Instant};

pub(crate) const ERASE_DEL: u8 = 0x7f;
pub(crate) const ERASE_BS: u8 = 0x08;

/// 输出要判定为“对本次按键的响应”，到达距按键发送不得超过该窗口。用于把异步
/// 日志等无关输出排除在 accept 判定之外；对 ≥300 baud 的回显（擦除回显首字节
/// <100ms）仍然宽裕。
const CORRELATED_OUTPUT_WINDOW: Duration = Duration::from_millis(200);
/// 可打印输入后在该窗口内看到输出，即认为对端处于回显状态。
const ECHO_LIVENESS_WINDOW: Duration = Duration::from_millis(250);
/// 回显活性的有效期。只影响“沉默 ⇒ 拒绝”的判定：活性过期说明对端可能已进入
/// 不回显的状态（如密码输入），此时沉默不是拒绝证据，也绝不能补发第二字节
/// （对端可能已静默接受第一字节，补发会多删一个字符）。
const ECHO_LIVENESS_TTL: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Eq, PartialEq)]
enum EraseDetectMode {
    Auto,
    Off,
}

/// 探测期间上一输出字节是 `^` 或 ESC 时挂起，等待的配对字节决定脱字符回显
/// （`^?` / `^H`）或 CSI（应用层重绘）判定；状态挂在 probe 上，跨 read 分块有效。
#[derive(Clone, Copy)]
enum PendingSigil {
    Caret,
    Escape,
}

struct Probe {
    /// 本次探测发送的候选字节。
    candidate: u8,
    /// 已透明补发过另一候选。再次被拒绝时锁定当前候选，不再反复探测。
    retried: bool,
    sent_at: Instant,
    deadline: Instant,
    /// 窗口内见过任何输出（不区分早晚）。
    saw_output: bool,
    /// 相关窗内见过输出：对端确实响应了按键（如空行退格的 BEL）。
    saw_correlated_output: bool,
    pending_sigil: Option<PendingSigil>,
}

/// 退格映射结果：直通时零拷贝借用输入分块；改写只发生在单字节擦除按键上，
/// 结果也只有一个字节，用枚举避免按键热路径上的堆分配。
pub(crate) enum MappedInput<'a> {
    Borrowed(&'a [u8]),
    Byte(u8),
}

impl MappedInput<'_> {
    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(data) => data,
            Self::Byte(byte) => std::slice::from_ref(byte),
        }
    }
}

pub(crate) struct EraseDetector {
    mode: EraseDetectMode,
    /// 已锁定的擦除字节。来源可以是权威协商（Telnet SLC_EC）或回显学习。
    resolved: Option<u8>,
    probe: Option<Probe>,
    last_printable_sent_at: Option<Instant>,
    echo_live_until: Option<Instant>,
    window: Duration,
}

impl EraseDetector {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            mode: EraseDetectMode::Auto,
            resolved: None,
            probe: None,
            last_printable_sent_at: None,
            echo_live_until: None,
            window,
        }
    }

    /// 模式切换不丢已锁定的值：SLC 协商值与回显学习结果在切回 AUTO 后仍然
    /// 有效，无需重新协商/探测。显式 DEL/BS 只是暂停改写与探测。
    pub(crate) fn set_mode(&mut self, mode: Option<&str>) {
        self.mode = match mode
            .map(str::trim)
            .map(|value| value.to_ascii_uppercase())
            .as_deref()
        {
            Some("DEL") | Some("BS") => EraseDetectMode::Off,
            _ => EraseDetectMode::Auto,
        };
        if self.mode == EraseDetectMode::Off {
            self.probe = None;
        }
    }

    /// 权威来源（Telnet SLC_EC）直接锁定擦除字节，跳过探测。
    pub(crate) fn set_resolved(&mut self, byte: u8) {
        self.resolved = Some(byte);
        self.probe = None;
    }

    #[cfg(test)]
    pub(crate) fn resolved(&self) -> Option<u8> {
        self.resolved
    }

    /// 输入映射。只有单字节擦除按键参与探测与改写，其余分块一律零拷贝直通
    /// （但照常参与回显活性跟踪）。
    pub(crate) fn map_input<'a>(&mut self, data: &'a [u8], now: Instant) -> MappedInput<'a> {
        if data.iter().any(|&byte| is_echoable_input(byte)) {
            self.last_printable_sent_at = Some(now);
        }
        if self.mode == EraseDetectMode::Off || !is_erase_keypress(data) {
            return MappedInput::Borrowed(data);
        }
        let keypress = data[0];
        if let Some(resolved) = self.resolved {
            // 只改写 DEL 按键；显式 ^H（Ctrl+H）保留用户语义。
            return if keypress == ERASE_DEL {
                MappedInput::Byte(resolved)
            } else {
                MappedInput::Borrowed(data)
            };
        }
        match &mut self.probe {
            // 探测期间的后续 DEL 按键按当前候选发送：候选未被证实前重发同一字节
            // 没有意义；候选切换（补发）后这里自然跟随。
            Some(probe) if keypress == ERASE_DEL => {
                if probe.candidate == keypress {
                    MappedInput::Borrowed(data)
                } else {
                    MappedInput::Byte(probe.candidate)
                }
            }
            // 探测期间的显式 ^H 原样发送，不干扰进行中的判定。
            Some(_) => MappedInput::Borrowed(data),
            None => {
                self.probe = Some(Probe {
                    candidate: keypress,
                    retried: false,
                    sent_at: now,
                    deadline: now + self.window,
                    saw_output: false,
                    saw_correlated_output: false,
                    pending_sigil: None,
                });
                MappedInput::Borrowed(data)
            }
        }
    }

    /// 观察对端输出，返回需要透明补发的候选字节（候选被拒绝时的重试）。
    pub(crate) fn observe_output(&mut self, data: &[u8], now: Instant) -> Option<u8> {
        if data.is_empty() {
            return None;
        }
        if self
            .last_printable_sent_at
            .is_some_and(|at| now.duration_since(at) <= ECHO_LIVENESS_WINDOW)
        {
            self.echo_live_until = Some(now + ECHO_LIVENESS_TTL);
        }
        let probe = self.probe.as_mut()?;
        probe.saw_output = true;
        if now.duration_since(probe.sent_at) <= CORRELATED_OUTPUT_WINDOW {
            probe.saw_correlated_output = true;
        }
        let candidate = probe.candidate;
        let mut decision = None;
        for &byte in data {
            match probe.pending_sigil.take() {
                Some(PendingSigil::Caret) if byte == caret_notation(candidate) => {
                    decision = Some(ProbeDecision::Reject);
                    break;
                }
                // 控制字符的脱字符回显以 `^` 开头；不是本候选的配对则 ^ 作废，
                // 当前字节继续走普通判定。
                Some(PendingSigil::Escape) if byte == b'[' => {
                    decision = Some(ProbeDecision::Accept);
                    break;
                }
                _ => {}
            }
            if byte == 0x08 {
                // 擦除回显（BS SP BS）与应用层光标左移都以 BS 表达。
                decision = Some(ProbeDecision::Accept);
                break;
            }
            probe.pending_sigil = match byte {
                b'^' => Some(PendingSigil::Caret),
                0x1b => Some(PendingSigil::Escape),
                _ => None,
            };
        }
        match decision {
            Some(ProbeDecision::Accept) => {
                self.resolve_probe();
                None
            }
            Some(ProbeDecision::Reject) => self.reject_probe(now),
            None => None,
        }
    }

    /// 探测窗到期处理，返回需要透明补发的候选字节。
    pub(crate) fn poll(&mut self, now: Instant) -> Option<u8> {
        if !self
            .probe
            .as_ref()
            .is_some_and(|probe| now >= probe.deadline)
        {
            return None;
        }
        let probe = self.probe.take().unwrap_or_else(|| unreachable!());
        if probe.saw_correlated_output {
            // 有相关响应但无判定信号（BEL 响铃、整行重画等）：对端消费了按键，
            // 只是没用 BS 表达。
            self.resolved = Some(probe.candidate);
            log::debug!(target: "terminal.erase", "erase probe accepted 0x{:02x} via correlated output", probe.candidate);
            return None;
        }
        if !probe.saw_correlated_output && self.echo_live(now) {
            // 回显活跃，而候选既无擦除回显也无任何相关响应（迟到的输出视为
            // 无关噪声）：候选被对端忽略。
            return self.reject_probe_state(probe, now);
        }
        // 无回显（密码输入等）或活性已过期：判定不了就保持未决，
        // 不补发、不锁定，下次按键重新探测。
        None
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.probe.as_ref().map(|probe| probe.deadline)
    }

    fn echo_live(&self, now: Instant) -> bool {
        self.echo_live_until.is_some_and(|until| now <= until)
    }

    fn resolve_probe(&mut self) {
        if let Some(probe) = self.probe.take() {
            log::debug!(target: "terminal.erase", "erase probe accepted 0x{:02x}", probe.candidate);
            self.resolved = Some(probe.candidate);
        }
    }

    fn reject_probe(&mut self, now: Instant) -> Option<u8> {
        let probe = self.probe.take().unwrap_or_else(|| unreachable!());
        self.reject_probe_state(probe, now)
    }

    fn reject_probe_state(&mut self, mut probe: Probe, now: Instant) -> Option<u8> {
        if probe.retried {
            // 两个候选都被拒绝：锁定当前候选停止探测，把裁决权交还手动开关。
            log::debug!(target: "terminal.erase", "erase probe exhausted, keeping 0x{:02x}", probe.candidate);
            self.resolved = Some(probe.candidate);
            return None;
        }
        probe.retried = true;
        probe.candidate = alternate_erase(probe.candidate);
        probe.sent_at = now;
        probe.deadline = now + self.window;
        probe.saw_output = false;
        probe.saw_correlated_output = false;
        probe.pending_sigil = None;
        let retry = probe.candidate;
        self.probe = Some(probe);
        Some(retry)
    }
}

#[derive(Clone, Copy)]
enum ProbeDecision {
    Accept,
    Reject,
}

fn is_erase_keypress(data: &[u8]) -> bool {
    matches!(data, [ERASE_DEL] | [ERASE_BS])
}

/// 回显活性只跟踪“打字”式输入：可打印字符与回车。擦除键本身不算。
fn is_echoable_input(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7e) || byte >= 0x80 || byte == b'\r'
}

fn alternate_erase(byte: u8) -> u8 {
    if byte == ERASE_DEL {
        ERASE_BS
    } else {
        ERASE_DEL
    }
}

/// 终端以脱字符记法回显控制字符：^? 表示 DEL，^H 表示 BS。
fn caret_notation(byte: u8) -> u8 {
    byte ^ 0x40
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(400);

    fn detector() -> EraseDetector {
        EraseDetector::new(WINDOW)
    }

    fn at(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    fn mapped(detector: &mut EraseDetector, data: &[u8], now: Instant) -> Vec<u8> {
        detector.map_input(data, now).as_slice().to_vec()
    }

    /// 让回显活性生效：先发可打印输入，再在活性窗内看到输出。
    fn warm_echo(detector: &mut EraseDetector, now: Instant) {
        mapped(detector, b"a", now);
        assert_eq!(
            detector.observe_output(b"a", now + Duration::from_millis(5)),
            None
        );
    }

    #[test]
    fn first_del_keypress_arms_probe_and_passes_through() {
        let mut detector = detector();
        let now = at(0);
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], now), [ERASE_DEL]);
        assert_eq!(detector.next_deadline(), Some(now + WINDOW));
    }

    #[test]
    fn erase_echo_resolves_del_candidate() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        assert_eq!(
            detector.observe_output(b"\x08 \x08", now + Duration::from_millis(20)),
            None
        );
        assert_eq!(detector.resolved(), Some(ERASE_DEL));
        // 锁定后 DEL 按键直通（候选即 DEL）。
        assert_eq!(
            mapped(&mut detector, &[ERASE_DEL], now + Duration::from_secs(1)),
            [ERASE_DEL]
        );
    }

    #[test]
    fn caret_echo_rejects_candidate_and_retries_the_other_byte() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        let retry = detector.observe_output(b"^?", now + Duration::from_millis(20));
        assert_eq!(retry, Some(ERASE_BS));
        // 补发的 BS 被接受（擦除回显）后锁定 BS。
        assert_eq!(
            detector.observe_output(b"\x08 \x08", now + Duration::from_millis(40)),
            None
        );
        assert_eq!(detector.resolved(), Some(ERASE_BS));
        assert_eq!(
            mapped(&mut detector, &[ERASE_DEL], now + Duration::from_secs(1)),
            [ERASE_BS]
        );
    }

    #[test]
    fn caret_echo_split_across_chunks_is_still_rejected() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        assert_eq!(
            detector.observe_output(b"^", now + Duration::from_millis(20)),
            None
        );
        assert_eq!(
            detector.observe_output(b"?", now + Duration::from_millis(30)),
            Some(ERASE_BS)
        );
    }

    #[test]
    fn silence_with_live_echo_retries_then_locks() {
        let mut detector = detector();
        let now = at(0);
        warm_echo(&mut detector, now);
        let press = now + Duration::from_millis(100);
        mapped(&mut detector, &[ERASE_DEL], press);
        // 回显活跃但候选毫无响应 → 到期拒绝并透明补发 BS。
        assert_eq!(detector.poll(press + WINDOW), Some(ERASE_BS));
        // BS 也被沉默以对（设备可能不确认擦除）→ 到期后锁定 BS，不再探测。
        let retry_deadline = press + WINDOW + WINDOW;
        assert_eq!(detector.poll(retry_deadline), None);
        assert_eq!(detector.resolved(), Some(ERASE_BS));
        assert_eq!(
            mapped(&mut detector, &[ERASE_DEL], retry_deadline + WINDOW),
            [ERASE_BS]
        );
    }

    #[test]
    fn silence_without_echo_liveness_stays_unresolved_and_sends_nothing() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        // 密码输入场景：无回显活性，到期不补发、不锁定。
        assert_eq!(detector.poll(now + WINDOW), None);
        assert_eq!(detector.resolved(), None);
        assert_eq!(detector.next_deadline(), None);
        // 下次按键重新探测。
        mapped(&mut detector, &[ERASE_DEL], now + Duration::from_secs(10));
        assert!(detector.next_deadline().is_some());
    }

    #[test]
    fn bel_response_counts_as_acceptance() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        // 空行退格的 BEL：无判定信号，但属相关响应 → 到期接受候选。
        assert_eq!(
            detector.observe_output(b"\x07", now + Duration::from_millis(30)),
            None
        );
        assert_eq!(detector.poll(now + WINDOW), None);
        assert_eq!(detector.resolved(), Some(ERASE_DEL));
    }

    #[test]
    fn csi_redraw_counts_as_acceptance() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        assert_eq!(
            detector.observe_output(b"\x1b[K", now + Duration::from_millis(30)),
            None
        );
        assert_eq!(detector.resolved(), Some(ERASE_DEL));
    }

    #[test]
    fn late_unrelated_output_does_not_resolve_the_probe() {
        let mut detector = detector();
        let now = at(0);
        warm_echo(&mut detector, now);
        let press = now + Duration::from_millis(50);
        mapped(&mut detector, &[ERASE_DEL], press);
        // 迟到（超出相关窗）的日志输出不算按键响应；回显仍活跃 → 沉默拒绝。
        detector.observe_output(b"device log line", press + Duration::from_millis(300));
        assert_eq!(detector.poll(press + WINDOW), Some(ERASE_BS));
    }

    #[test]
    fn keypress_during_probe_follows_the_current_candidate() {
        let mut detector = detector();
        let now = at(0);
        warm_echo(&mut detector, now);
        let press = now + Duration::from_millis(50);
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], press), [ERASE_DEL]);
        // 到期拒绝并补发 BS 后，探测期间的 DEL 按键跟随新候选。
        assert_eq!(detector.poll(press + WINDOW), Some(ERASE_BS));
        assert_eq!(
            mapped(&mut detector, &[ERASE_DEL], press + WINDOW),
            [ERASE_BS]
        );
        // 显式 ^H 按键不受探测影响。
        assert_eq!(
            mapped(&mut detector, &[ERASE_BS], press + WINDOW),
            [ERASE_BS]
        );
    }

    #[test]
    fn explicit_bs_keypress_can_calibrate() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_BS], now);
        assert_eq!(
            detector.observe_output(b"\x08 \x08", now + Duration::from_millis(20)),
            None
        );
        assert_eq!(detector.resolved(), Some(ERASE_BS));
        // DEL 按键按锁定值改写。
        assert_eq!(
            mapped(&mut detector, &[ERASE_DEL], now + Duration::from_secs(1)),
            [ERASE_BS]
        );
    }

    #[test]
    fn resolved_keeps_explicit_bs_keypress_untouched() {
        let mut detector = detector();
        detector.set_resolved(ERASE_DEL);
        assert_eq!(mapped(&mut detector, &[ERASE_BS], at(0)), [ERASE_BS]);
    }

    #[test]
    fn off_mode_passes_through_and_never_probes() {
        let mut detector = detector();
        detector.set_mode(Some("DEL"));
        let now = at(0);
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], now), [ERASE_DEL]);
        assert_eq!(detector.next_deadline(), None);
        assert_eq!(detector.poll(now + WINDOW), None);
        detector.set_mode(Some("bs"));
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], now), [ERASE_DEL]);
        assert_eq!(detector.next_deadline(), None);
    }

    #[test]
    fn mode_switching_preserves_the_learned_byte() {
        let mut detector = detector();
        detector.set_resolved(ERASE_BS);
        detector.set_mode(Some("DEL"));
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], at(0)), [ERASE_DEL]);
        detector.set_mode(Some("AUTO"));
        assert_eq!(mapped(&mut detector, &[ERASE_DEL], at(1)), [ERASE_BS]);
    }

    #[test]
    fn bulk_input_is_never_probed_or_rewritten() {
        let mut detector = detector();
        detector.set_resolved(ERASE_BS);
        // 粘贴/脚本/trzsz 数据里的 0x7f 不是按键，即使已锁定也直通。
        let paste = b"show run\x7f\r\n".to_vec();
        assert_eq!(mapped(&mut detector, &paste, at(0)), paste);
        assert_eq!(detector.next_deadline(), None);
    }

    #[test]
    fn rejected_twice_locks_the_current_candidate() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        assert_eq!(
            detector.observe_output(b"^?", now + Duration::from_millis(10)),
            Some(ERASE_BS)
        );
        // 补发的 BS 也遭脱字符回显拒绝 → 锁定 BS（最后一个候选），停止探测。
        assert_eq!(
            detector.observe_output(b"^H", now + Duration::from_millis(30)),
            None
        );
        assert_eq!(detector.resolved(), Some(ERASE_BS));
        assert_eq!(detector.next_deadline(), None);
    }

    #[test]
    fn caret_echo_of_the_other_character_does_not_reject() {
        let mut detector = detector();
        let now = at(0);
        mapped(&mut detector, &[ERASE_DEL], now);
        // 输出里的 ^H 不是对本候选（DEL ⇒ ^?）的拒绝。
        assert_eq!(
            detector.observe_output(b"^H", now + Duration::from_millis(10)),
            None
        );
        assert!(detector.next_deadline().is_some());
        assert_eq!(detector.resolved(), None);
    }
}
