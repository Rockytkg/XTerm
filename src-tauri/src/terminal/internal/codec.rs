use std::borrow::Cow;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, BIG5, EUC_KR, GB18030, GBK, SHIFT_JIS, UTF_8};
use memchr::memchr;

use crate::terminal::internal::core::{
    CodecState, ConnectionReadResult, CODEC_RAW_BUFFER_CAPACITY, DETECTION_ERROR_UNLOCK_THRESHOLD,
    DETECTION_HISTORY_MAX_BYTES, DETECTION_SAMPLE_MAX_BYTES, SERIAL_MIN_DETECT_BYTES,
};

/// Confidence reported for a buffer that is valid UTF-8 except for a
/// truncated tail character (stream chunking artifact, not real content).
const UTF8_PREFIX_CONFIDENCE: f32 = 0.86;

/// 候选评分分差小于该值视为近平局：GBK/Big5 互解都落在合法 CJK 区块，
/// 启发式评分无法区分，需要统计检测器仲裁。
const DETECTION_TIE_MARGIN: f32 = 0.08;
/// 样本短于该长度时统计检测器自身也不可靠，不做仲裁，维持评分结果。
const DETECTION_ARBITRATION_MIN_BYTES: usize = 8;

const TRZSZ_TRIGGER_BYTES: &[u8] = b"::TRZSZ:TRANSFER:";

/// How a decode pass should populate `ConnectionReadResult.raw_bytes`. The
/// caller derives the mode from the delivery state so raw bytes are only
/// copied when the replay cache would actually keep them; anything else would
/// be copied here and discarded by `push_cached_output` in the same pass.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RawBytesMode {
    /// Transport cannot deliver raw bytes: never copy.
    Disabled,
    /// Raw channel is armed: preserve every delivered byte.
    Armed,
    /// Raw channel idle: preserve the chunk only when it carries the trzsz
    /// trigger, so the transfer protocol can still auto-arm.
    Detect,
}

impl RawBytesMode {
    fn select(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            RawBytesMode::Disabled => Vec::new(),
            RawBytesMode::Armed => bytes.to_vec(),
            RawBytesMode::Detect => {
                if contains_trzsz_trigger(bytes) {
                    bytes.to_vec()
                } else {
                    Vec::new()
                }
            }
        }
    }
}

pub(super) fn contains_trzsz_trigger(raw_bytes: &[u8]) -> bool {
    if raw_bytes.len() < TRZSZ_TRIGGER_BYTES.len() {
        return false;
    }
    let mut offset = 0_usize;
    while let Some(index) = memchr(b':', &raw_bytes[offset..]) {
        let start = offset + index;
        if raw_bytes.len() - start < TRZSZ_TRIGGER_BYTES.len() {
            return false;
        }
        if raw_bytes[start..].starts_with(TRZSZ_TRIGGER_BYTES) {
            return true;
        }
        offset = start + 1;
    }
    false
}

impl CodecState {
    pub(super) fn new(backend_encoding: Option<String>, realtime_detection_enabled: bool) -> Self {
        let backend_encoding_handle = backend_encoding
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| Encoding::for_label(value.as_bytes()));
        let backend_encoding = backend_encoding_handle.map(encoding_label);
        Self {
            backend_encoding,
            backend_encoding_handle,
            detected_encoding: None,
            detected_encoding_handle: None,
            detected_confidence: 0.0,
            consecutive_errors: 0,
            raw_buffer: Vec::with_capacity(CODEC_RAW_BUFFER_CAPACITY),
            pending_tail: Vec::with_capacity(8),
            detection_history: Vec::new(),
            realtime_detection_enabled,
        }
    }

    pub(super) fn set_realtime_detection(&mut self, enabled: bool) {
        self.realtime_detection_enabled = enabled;
        self.consecutive_errors = 0;
        clear_detected_encoding(self);
    }

    pub(super) fn set_backend_encoding(&mut self, encoding: impl Into<String>) {
        let encoding = encoding.into();
        let Some(encoding_handle) = Encoding::for_label(encoding.as_bytes()) else {
            log::debug!(target: "terminal.codec", "codec.backend_encoding.ignored invalid_label={encoding}");
            return;
        };
        self.backend_encoding = Some(encoding_label(encoding_handle));
        self.backend_encoding_handle = Some(encoding_handle);
        self.consecutive_errors = 0;
        clear_detected_encoding(self);
    }

    pub(super) fn clear_backend_encoding(&mut self) {
        self.backend_encoding = None;
        self.backend_encoding_handle = None;
        self.consecutive_errors = 0;
        clear_detected_encoding(self);
    }
}

#[cfg(test)]
fn decode_backend_bytes(bytes: &[u8], codec: &mut CodecState) -> ConnectionReadResult {
    decode_backend_bytes_with_raw(bytes, codec, RawBytesMode::Armed)
}

pub(super) fn decode_backend_bytes_with_raw(
    bytes: &[u8],
    codec: &mut CodecState,
    raw_mode: RawBytesMode,
) -> ConnectionReadResult {
    if bytes.is_empty() {
        return empty_decode_result(codec);
    }

    if codec.raw_buffer.is_empty() && codec.pending_tail.is_empty() {
        // 缓冲为空时直接借用新块解码，只有未消费的尾部才会进入缓冲；
        // UTF-8 预检、ASCII 直发与置信度簿记全部在 decode_chunk 内完成。
        return decode_chunk(codec, bytes, false, raw_mode);
    }

    codec.raw_buffer.extend_from_slice(bytes);
    let combined = take_undecoded_bytes(codec);
    decode_chunk(codec, &combined, false, raw_mode)
}

#[cfg(test)]
fn flush_decoded_backend_bytes(codec: &mut CodecState) -> ConnectionReadResult {
    flush_decoded_backend_bytes_with_raw(codec, RawBytesMode::Armed)
}

pub(super) fn flush_decoded_backend_bytes_with_raw(
    codec: &mut CodecState,
    raw_mode: RawBytesMode,
) -> ConnectionReadResult {
    if codec.raw_buffer.is_empty() && codec.pending_tail.is_empty() {
        return empty_decode_result(codec);
    }

    let combined = take_undecoded_bytes(codec);
    decode_chunk(codec, &combined, true, raw_mode)
}

fn decode_chunk(
    codec: &mut CodecState,
    chunk: &[u8],
    force: bool,
    raw_mode: RawBytesMode,
) -> ConnectionReadResult {
    debug_assert!(!chunk.is_empty());

    // UTF-8 预检每块只跑一次，结果同时驱动直发与"撕裂尾部等待"判定。
    match precheck_utf8(codec, chunk, force, raw_mode) {
        PrecheckUtf8::Delivered(result) => return result,
        PrecheckUtf8::Held => {
            stash_undecoded_bytes(codec, chunk);
            return empty_utf8_hold_result();
        }
        PrecheckUtf8::Ascii | PrecheckUtf8::Skipped => {}
        PrecheckUtf8::Invalid => {
            // 检测未定型时，非法 UTF-8 且带撕裂 legacy 尾部的样本等齐再判，
            // 避免撕裂尾巴污染检测评分。
            if !force
                && codec.backend_encoding.is_none()
                && codec.detected_encoding.is_none()
                && codec.realtime_detection_enabled
                && has_incomplete_legacy_tail(chunk)
            {
                stash_undecoded_bytes(codec, chunk);
                return ConnectionReadResult {
                    data: String::new(),
                    raw_bytes: Vec::new(),
                    encoding: encoding_label(UTF_8),
                    confidence: default_confidence(codec, UTF_8),
                };
            }
        }
    }

    if let Some(result) =
        decode_utf8_candidate_for_auto_detected_legacy(codec, chunk, force, raw_mode)
    {
        return result;
    }

    let ((mut encoding, mut confidence), mut predecoded) =
        select_encoding_for_decode(chunk, codec, force);
    let mut decoded = decode_complete_chunk(chunk, encoding, force, predecoded.take());

    if matches!(decoded, Some((_, _, _, true))) && should_recover_from_decode_errors(codec) {
        clear_detected_encoding(codec);
        ((encoding, confidence), predecoded) = select_encoding_for_decode(chunk, codec, true);
        decoded = decode_complete_chunk(chunk, encoding, force, predecoded.take());
    }

    let Some((data, complete_len, had_non_ascii, had_errors)) = decoded else {
        stash_undecoded_bytes(codec, chunk);
        return ConnectionReadResult {
            data: String::new(),
            raw_bytes: Vec::new(),
            encoding: encoding_label(encoding),
            confidence,
        };
    };

    codec.pending_tail = chunk[complete_len..].to_vec();
    apply_decode_feedback(
        codec,
        encoding,
        confidence,
        had_non_ascii,
        had_errors,
        complete_len,
    );
    record_detection_evidence(codec, &chunk[..complete_len], had_non_ascii, had_errors);

    ConnectionReadResult {
        data,
        raw_bytes: raw_mode.select(&chunk[..complete_len]),
        encoding: encoding_label(encoding),
        confidence: if had_errors {
            confidence * 0.65
        } else {
            confidence
        },
    }
}

/// The winning detection candidate's decode, kept so the delivery pass can
/// reuse it instead of transcoding the same bytes a second time.
struct ScoredDecode {
    text: String,
    had_errors: bool,
    /// Length of the byte sample this text was decoded from.
    sample_len: usize,
}

/// Decodes the longest complete-character prefix of `undecoded`. Returns
/// `(text, consumed_len, had_non_ascii, had_errors)`, or `None` when the
/// buffer ends (or starts) mid-character and nothing can be emitted yet.
///
/// `predecoded` is reused only when it covers exactly `undecoded` (same sample
/// length and no torn tail trimmed off); `encoding.decode` is deterministic,
/// so the reused text is byte-identical to a fresh decode of the same slice.
fn decode_complete_chunk(
    undecoded: &[u8],
    encoding: &'static Encoding,
    force: bool,
    predecoded: Option<ScoredDecode>,
) -> Option<(String, usize, bool, bool)> {
    let complete_len = if force {
        undecoded.len()
    } else {
        complete_prefix_len(undecoded, encoding)
    };
    if complete_len == 0 {
        return None;
    }
    let chunk = &undecoded[..complete_len];
    let had_non_ascii = !chunk.is_ascii();
    if let Some(pre) = predecoded {
        if complete_len == undecoded.len() && pre.sample_len == undecoded.len() {
            return Some((pre.text, complete_len, had_non_ascii, pre.had_errors));
        }
    }
    let (text, _, had_errors) = encoding.decode(chunk);
    Some((text.into_owned(), complete_len, had_non_ascii, had_errors))
}

fn empty_decode_result(codec: &CodecState) -> ConnectionReadResult {
    let (encoding, confidence) =
        active_encoding(codec).unwrap_or((UTF_8, default_confidence(codec, UTF_8)));
    ConnectionReadResult {
        data: String::new(),
        raw_bytes: Vec::new(),
        encoding: encoding_label(encoding),
        confidence,
    }
}

fn apply_decode_feedback(
    codec: &mut CodecState,
    encoding: &'static Encoding,
    confidence: f32,
    had_non_ascii: bool,
    had_errors: bool,
    decoded_len: usize,
) {
    // 纯 ASCII 块在所有候选编码下解码结果一致，对编码判定零贡献：既不重置
    // 也不累计错误计数，更不触发提交。交换机输出中大量 ASCII 行夹在中文行
    // 之间，若重置计数，"连续 3 块错误即解锁重检"在 ASCII 主导的流量下永不
    // 达成（且纯 ASCII 块必然无解码错误，两个分支互斥）。
    if codec.backend_encoding.is_some() || !had_non_ascii {
        return;
    }

    if had_errors {
        codec.consecutive_errors = codec.consecutive_errors.saturating_add(1);
        if should_recover_from_decode_errors(codec) {
            clear_detected_encoding(codec);
        }
        return;
    }

    codec.consecutive_errors = 0;
    if codec.realtime_detection_enabled && should_commit_detected_encoding(decoded_len, confidence)
    {
        codec.detected_encoding = Some(encoding_label(encoding));
        codec.detected_encoding_handle = Some(encoding);
        codec.detected_confidence = confidence.max(codec.detected_confidence);
        codec.detection_history.clear();
    }
}

fn should_recover_from_decode_errors(codec: &CodecState) -> bool {
    codec.backend_encoding.is_none()
        && codec.realtime_detection_enabled
        && codec.consecutive_errors >= DETECTION_ERROR_UNLOCK_THRESHOLD
}

fn clear_detected_encoding(codec: &mut CodecState) {
    codec.detected_encoding = None;
    codec.detected_encoding_handle = None;
    codec.detected_confidence = 0.0;
    codec.detection_history.clear();
}

/// Buffer holds only a truncated tail character: emit nothing yet but keep
/// waiting for the rest of the character.
fn empty_utf8_hold_result() -> ConnectionReadResult {
    ConnectionReadResult {
        data: String::new(),
        raw_bytes: Vec::new(),
        encoding: encoding_label(UTF_8),
        confidence: 0.35,
    }
}

/// Returns the valid prefix when the only UTF-8 error is a truncated tail.
/// Invalid interior bytes remain eligible for legacy-encoding detection.
fn split_truncated_utf8_prefix(bytes: &[u8]) -> Option<(&str, usize)> {
    let error = std::str::from_utf8(bytes).err()?;
    if error.error_len().is_some() {
        return None;
    }
    let complete_len = error.valid_up_to();
    let prefix = std::str::from_utf8(&bytes[..complete_len]).ok()?;
    Some((prefix, complete_len))
}

/// UTF-8 预检结果。每块只跑一次 `from_utf8`，调用方按枚举分流，不再重复扫描。
enum PrecheckUtf8 {
    /// 未做校验（force/固定编码/已锁定/检测关闭）：直接走通用解码路径。
    Skipped,
    /// 合法 UTF-8（或撕裂前缀 + 有效前缀）已交付。
    Delivered(ConnectionReadResult),
    /// 整块只有一个撕裂的 UTF-8 尾部字符：交调用方落缓冲等待。
    Held,
    /// 纯 ASCII：走通用路径（固定置信度），无需等待判定。
    Ascii,
    /// 内部存在非法字节：进入 legacy 检测路径。
    Invalid,
}

fn precheck_utf8(
    codec: &mut CodecState,
    bytes: &[u8],
    force: bool,
    raw_mode: RawBytesMode,
) -> PrecheckUtf8 {
    if force
        || codec.backend_encoding.is_some()
        || codec.detected_encoding.is_some()
        || !codec.realtime_detection_enabled
    {
        return PrecheckUtf8::Skipped;
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => {
            // Keep the existing ASCII fast path and its fixed confidence.
            if text.is_ascii() {
                return PrecheckUtf8::Ascii;
            }
            match valid_utf8_unsettled_result(codec, bytes, text, raw_mode) {
                Some(result) => PrecheckUtf8::Delivered(result),
                None => PrecheckUtf8::Skipped,
            }
        }
        Err(_) => match split_truncated_utf8_prefix(bytes) {
            None => PrecheckUtf8::Invalid,
            Some((_, 0)) => PrecheckUtf8::Held,
            Some((prefix, complete_len)) => {
                codec.raw_buffer.clear();
                codec.pending_tail = bytes[complete_len..].to_vec();
                apply_decode_feedback(
                    codec,
                    UTF_8,
                    UTF8_PREFIX_CONFIDENCE,
                    !prefix.is_ascii(),
                    false,
                    complete_len,
                );

                PrecheckUtf8::Delivered(ConnectionReadResult {
                    data: prefix.to_string(),
                    raw_bytes: raw_mode.select(&bytes[..complete_len]),
                    encoding: encoding_label(UTF_8),
                    confidence: UTF8_PREFIX_CONFIDENCE,
                })
            }
        },
    }
}

/// 检测未定型（无固定/已锁定编码且实时检测开启）时，合法非 ASCII UTF-8 的
/// 直发路径：只做置信度簿记，不触碰缓冲。采样窗口拒绝落在 UTF-8 Continuation
/// 上的切片（返回 None，交回统一解码路径），与改造前行为一致。
fn valid_utf8_unsettled_result(
    codec: &mut CodecState,
    bytes: &[u8],
    text: &str,
    raw_mode: RawBytesMode,
) -> Option<ConnectionReadResult> {
    if codec.backend_encoding.is_some()
        || codec.detected_encoding.is_some()
        || !codec.realtime_detection_enabled
    {
        return None;
    }
    let sample_start = bytes.len().saturating_sub(DETECTION_SAMPLE_MAX_BYTES);
    let sample = &bytes[sample_start..];
    let sample_text = if sample_start == 0 {
        text
    } else {
        std::str::from_utf8(sample).ok()?
    };
    let confidence = utf8_sample_confidence(sample, sample_text);

    codec.raw_buffer.clear();
    codec.pending_tail.clear();
    apply_decode_feedback(codec, UTF_8, confidence, true, false, bytes.len());

    Some(ConnectionReadResult {
        data: text.to_owned(),
        raw_bytes: raw_mode.select(bytes),
        encoding: encoding_label(UTF_8),
        confidence,
    })
}

fn decode_utf8_candidate_for_auto_detected_legacy(
    codec: &mut CodecState,
    bytes: &[u8],
    force: bool,
    raw_mode: RawBytesMode,
) -> Option<ConnectionReadResult> {
    if force
        || codec.backend_encoding.is_some()
        || !codec.realtime_detection_enabled
        || codec
            .detected_encoding_handle
            .is_none_or(|encoding| encoding == UTF_8)
    {
        return None;
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.is_ascii() {
            return None;
        }
        let confidence = utf8_sample_confidence(bytes, text);
        if !should_switch_auto_detected_to_utf8(bytes.len(), confidence) {
            return None;
        }
        codec.raw_buffer.clear();
        codec.pending_tail.clear();
        codec.detected_encoding = Some(encoding_label(UTF_8));
        codec.detected_encoding_handle = Some(UTF_8);
        codec.detected_confidence = confidence;
        codec.consecutive_errors = 0;
        codec.detection_history.clear();
        return Some(ConnectionReadResult {
            data: text.to_string(),
            raw_bytes: raw_mode.select(bytes),
            encoding: encoding_label(UTF_8),
            confidence,
        });
    }

    let (prefix, complete_len) = split_truncated_utf8_prefix(bytes)?;
    codec.raw_buffer.clear();
    codec.pending_tail = bytes[complete_len..].to_vec();
    if complete_len == 0 {
        return Some(empty_utf8_hold_result());
    }

    Some(ConnectionReadResult {
        data: prefix.to_string(),
        raw_bytes: raw_mode.select(&bytes[..complete_len]),
        encoding: encoding_label(UTF_8),
        confidence: UTF8_PREFIX_CONFIDENCE,
    })
}

fn should_switch_auto_detected_to_utf8(sample_len: usize, confidence: f32) -> bool {
    sample_len >= 3 && confidence >= 0.90
}

fn should_commit_detected_encoding(sample_len: usize, confidence: f32) -> bool {
    let threshold = match sample_len {
        0..=15 => 1.0,
        16..=31 => 0.96,
        32..=63 => 0.91,
        // 置信度上限 0.82，阈值须低于上限该分段才可达成；仅高分样本能锁定。
        64..=127 => 0.80,
        128..=255 => 0.76,
        256..=511 => 0.68,
        _ => 0.35,
    };
    confidence >= threshold
}

fn take_undecoded_bytes(codec: &mut CodecState) -> Vec<u8> {
    if codec.pending_tail.is_empty() {
        return std::mem::take(&mut codec.raw_buffer);
    }

    let mut combined = Vec::with_capacity(codec.pending_tail.len() + codec.raw_buffer.len());
    combined.extend_from_slice(&codec.pending_tail);
    combined.extend_from_slice(&codec.raw_buffer);
    codec.pending_tail.clear();
    codec.raw_buffer.clear();
    combined
}

fn stash_undecoded_bytes(codec: &mut CodecState, bytes: &[u8]) {
    // 此刻 raw_buffer 必为空（要么前置条件为空，要么刚被 take_undecoded_bytes
    // 排空），直接存入即可。
    codec.raw_buffer.extend_from_slice(bytes);
}

fn active_encoding(codec: &CodecState) -> Option<(&'static Encoding, f32)> {
    if let Some(encoding) = codec.backend_encoding_handle {
        return Some((encoding, 1.0));
    }

    if codec.realtime_detection_enabled {
        // 提交置信度下限即 0.35（finalize_detection_confidence 的钳制下界），
        // 此处无需再设地板。
        if let Some(encoding) = codec.detected_encoding_handle {
            return Some((encoding, codec.detected_confidence));
        }
    }

    None
}

fn encoding_label(encoding: &'static Encoding) -> String {
    encoding.name().to_ascii_lowercase()
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CodecEncodingKind {
    Utf8,
    Chinese,
    Big5,
    ShiftJis,
    EucKr,
    Other,
}

fn codec_encoding_kind(encoding: &'static Encoding) -> CodecEncodingKind {
    // `Encoding` equality is a pointer comparison against the encoding_rs
    // statics, so this never touches name strings.
    if encoding == UTF_8 {
        CodecEncodingKind::Utf8
    } else if encoding == GBK || encoding == GB18030 {
        CodecEncodingKind::Chinese
    } else if encoding == BIG5 {
        CodecEncodingKind::Big5
    } else if encoding == SHIFT_JIS {
        CodecEncodingKind::ShiftJis
    } else if encoding == EUC_KR {
        CodecEncodingKind::EucKr
    } else {
        CodecEncodingKind::Other
    }
}

fn default_confidence(codec: &CodecState, encoding: &'static Encoding) -> f32 {
    if encoding == UTF_8 {
        if codec.raw_buffer.is_ascii() && codec.pending_tail.is_ascii() {
            0.98
        } else {
            0.35
        }
    } else {
        0.35
    }
}

fn select_encoding_for_decode(
    bytes: &[u8],
    codec: &mut CodecState,
    force: bool,
) -> ((&'static Encoding, f32), Option<ScoredDecode>) {
    if let Some((encoding, confidence)) = active_encoding(codec) {
        return ((encoding, confidence), None);
    }

    if bytes.is_ascii() {
        return ((UTF_8, 0.98), None);
    }

    if !codec.realtime_detection_enabled {
        return ((UTF_8, default_confidence(codec, UTF_8)), None);
    }

    // 不变式：到达此处时检测必然未定型（无固定编码、无已提交检测结果，
    // detected_confidence 恒为 0），每个 legacy 块都重新评分；样本含跨块
    // 积累的历史证据。
    let (encoding, confidence, decode, sample_len) = {
        let sample = detection_sample(codec, bytes);
        let (encoding, confidence, decode) = select_detected_encoding_with_decode(&sample);
        (encoding, confidence, decode, sample.len())
    };
    maybe_commit_detected_encoding(codec, sample_len, encoding, confidence, force);
    ((encoding, confidence), decode)
}

/// 构建检测样本：检测历史尾部 + 当前块，总量不超过 `DETECTION_SAMPLE_MAX_BYTES`，
/// 且始终完整包含当前块。历史为空时零拷贝借用当前块（与改造前逐块检测一致）。
fn detection_sample<'a>(codec: &CodecState, chunk: &'a [u8]) -> Cow<'a, [u8]> {
    if codec.detection_history.is_empty() || chunk.len() >= DETECTION_SAMPLE_MAX_BYTES {
        let window_start = chunk.len().saturating_sub(DETECTION_SAMPLE_MAX_BYTES);
        return Cow::Borrowed(&chunk[window_start..]);
    }

    let history_budget = DETECTION_SAMPLE_MAX_BYTES - chunk.len();
    let history_start = codec.detection_history.len().saturating_sub(history_budget);
    let history = &codec.detection_history[history_start..];
    let mut sample = Vec::with_capacity(history.len() + chunk.len());
    sample.extend_from_slice(history);
    sample.extend_from_slice(chunk);
    Cow::Owned(sample)
}

/// 检测未定型期间，把成功交付的 legacy 字节积累进历史窗口。仅积累"无错误
/// 且含非 ASCII"的解码块：纯 ASCII 行对编码判定无贡献，带替换符的块说明
/// 当前判定可能错误，都不应污染证据。
fn record_detection_evidence(
    codec: &mut CodecState,
    decoded_bytes: &[u8],
    had_non_ascii: bool,
    had_errors: bool,
) {
    if decoded_bytes.is_empty()
        || codec.backend_encoding.is_some()
        || codec.detected_encoding_handle.is_some()
        || !codec.realtime_detection_enabled
        || !had_non_ascii
        || had_errors
    {
        return;
    }

    let overflow = codec.detection_history.len() + decoded_bytes.len();
    if overflow > DETECTION_HISTORY_MAX_BYTES {
        codec
            .detection_history
            .drain(..overflow - DETECTION_HISTORY_MAX_BYTES);
    }
    codec.detection_history.extend_from_slice(decoded_bytes);
}

/// 调用点已保证检测未定型（detected_encoding_handle 为空、confidence 为 0），
/// 达阈值即提交。提交后清空历史证据，避免占用内存或在解锁重检时混入旧证据。
fn maybe_commit_detected_encoding(
    codec: &mut CodecState,
    sample_len: usize,
    encoding: &'static Encoding,
    confidence: f32,
    force: bool,
) {
    if !force && !should_commit_detected_encoding(sample_len, confidence) {
        return;
    }
    codec.detected_encoding = Some(encoding_label(encoding));
    codec.detected_encoding_handle = Some(encoding);
    codec.detected_confidence = confidence;
    codec.detection_history.clear();
}

#[cfg(test)]
pub(super) fn select_detected_encoding(bytes: &[u8]) -> (&'static Encoding, f32) {
    let (encoding, confidence, _) = select_detected_encoding_with_decode(bytes);
    (encoding, confidence)
}

/// Runs candidate scoring and keeps the winning decode so the delivery pass
/// (`decode_complete_chunk`) can reuse it instead of transcoding the sample a
/// second time. Only one extra String is alive at a time (the current best);
/// losing candidates are dropped as they are scored.
fn select_detected_encoding_with_decode(
    bytes: &[u8],
) -> (&'static Encoding, f32, Option<ScoredDecode>) {
    let start = bytes.len().saturating_sub(DETECTION_SAMPLE_MAX_BYTES);
    let sample = &bytes[start..];
    if let Ok(text) = std::str::from_utf8(sample) {
        let confidence = utf8_sample_confidence(sample, text);
        return (UTF_8, confidence, None);
    }

    let candidates = [GB18030, BIG5, SHIFT_JIS, EUC_KR];
    let (score, text, had_errors) = decode_and_score(sample, candidates[0]);
    let mut best = (candidates[0], score);
    let mut best_decode = Some(ScoredDecode {
        text,
        had_errors,
        sample_len: sample.len(),
    });
    let mut runner_up: (&'static Encoding, f32) = (candidates[0], 0.0);

    for encoding in candidates.into_iter().skip(1) {
        let (score, text, had_errors) = decode_and_score(sample, encoding);
        if score > best.1 {
            runner_up = best;
            best = (encoding, score);
            best_decode = Some(ScoredDecode {
                text,
                had_errors,
                sample_len: sample.len(),
            });
        } else if score > runner_up.1 {
            runner_up = (encoding, score);
        }
    }

    if should_consult_statistical_detector(sample.len(), best.1, runner_up.1) {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
        detector.feed(sample, true);
        let guessed = detector.guess(None, Utf8Detection::Allow);
        let (score, text, had_errors) = decode_and_score(sample, guessed);
        let guessed_score = score + 0.03;
        if guessed_score > best.1 {
            runner_up = best;
            best = (guessed, guessed_score);
            best_decode = Some(ScoredDecode {
                text,
                had_errors,
                sample_len: sample.len(),
            });
        } else if guessed_score > runner_up.1 {
            runner_up = (guessed, guessed_score);
        }
    } else {
        maybe_arbitrate_cjk_tie(sample, &mut best, &mut best_decode, runner_up);
    }

    let confidence = finalize_detection_confidence(sample.len(), best.1, runner_up.1);
    (best.0, confidence, best_decode)
}

/// 短样本上的简繁近平局仲裁。GBK 与 Big5 互解都落在合法 CJK 区块、不产生
/// 替换符，启发式评分分差常在浮点噪声量级，逐块重检会掷出不同胜者（同一
/// 设备这行正常、下行乱码的根因）。此时让带语言模型的统计检测器仲裁，但
/// 仅在它明确支持亚军候选时才交换胜者；给出第三种编码时维持原判，避免在
/// 短样本上引入 ISO-2022-JP 之类的新误判面。
fn maybe_arbitrate_cjk_tie(
    sample: &[u8],
    best: &mut (&'static Encoding, f32),
    best_decode: &mut Option<ScoredDecode>,
    runner_up: (&'static Encoding, f32),
) {
    if sample.len() < DETECTION_ARBITRATION_MIN_BYTES
        || best.1 - runner_up.1 >= DETECTION_TIE_MARGIN
        || !is_cjk_confusable_pair(best.0, runner_up.0)
    {
        return;
    }

    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    detector.feed(sample, true);
    if detector.guess(None, Utf8Detection::Allow) != runner_up.0 {
        return;
    }

    let (_, text, had_errors) = decode_and_score(sample, runner_up.0);
    *best = (runner_up.0, runner_up.1);
    *best_decode = Some(ScoredDecode {
        text,
        had_errors,
        sample_len: sample.len(),
    });
}

/// 简（GBK/GB18030）繁（Big5）是设备控制台上唯一一对互解后仍全部落在
/// 合法 CJK 区块的候选：其它组合都会被 kana/hangul 惩罚或替换符拉开分差。
fn is_cjk_confusable_pair(a: &'static Encoding, b: &'static Encoding) -> bool {
    matches!(
        (codec_encoding_kind(a), codec_encoding_kind(b)),
        (CodecEncodingKind::Chinese, CodecEncodingKind::Big5)
            | (CodecEncodingKind::Big5, CodecEncodingKind::Chinese)
    )
}

fn should_consult_statistical_detector(sample_len: usize, best_score: f32, runner_up: f32) -> bool {
    sample_len >= 64 && (best_score < 0.55 || best_score - runner_up < 0.08)
}

fn utf8_sample_confidence(bytes: &[u8], text: &str) -> f32 {
    let mut chars = 0_usize;
    let mut printable = 0_usize;
    let mut cjk = 0_usize;
    for ch in text.chars() {
        chars += 1;
        if is_terminal_char(ch) {
            printable += 1;
        }
        if is_cjk(ch) {
            cjk += 1;
        }
    }

    let chars = chars.max(1) as f32;
    let printable_ratio = printable as f32 / chars;
    let cjk_bonus = (cjk as f32 / chars).min(0.45) * 0.10;
    let utf8_bonus = if bytes.len() >= 32 {
        0.08
    } else if bytes.len() >= 16 {
        0.04
    } else {
        0.0
    };
    (printable_ratio * 0.88 + cjk_bonus + utf8_bonus).clamp(0.55, 0.99)
}

fn finalize_detection_confidence(sample_len: usize, best_score: f32, runner_up: f32) -> f32 {
    let margin_bonus = ((best_score - runner_up).max(0.0) * 0.35).min(0.12);
    let length_cap = match sample_len {
        0..=7 => 0.42,
        8..=15 => 0.50,
        16..=31 => 0.62,
        32..=63 => 0.74,
        64..=127 => 0.82,
        128..=255 => 0.88,
        256..=511 => 0.93,
        _ => 0.99,
    };
    (best_score + margin_bonus).clamp(0.35, length_cap)
}

/// Decodes `bytes` with `encoding` and scores the result. The decoded text is
/// returned alongside the score so the winning candidate's decode can be
/// reused for delivery instead of being dropped and recomputed.
fn decode_and_score(bytes: &[u8], encoding: &'static Encoding) -> (f32, String, bool) {
    let (text, _, had_errors) = encoding.decode(bytes);
    let score = score_decoded_text(bytes, &text, encoding, had_errors);
    (score, text.into_owned(), had_errors)
}

fn score_decoded_text(
    bytes: &[u8],
    text: &str,
    encoding: &'static Encoding,
    had_errors: bool,
) -> f32 {
    let mut chars = 0_usize;
    let mut printable = 0_usize;
    let mut cjk = 0_usize;
    let mut hangul = 0_usize;
    let mut kana = 0_usize;
    let mut replacement = 0_usize;
    let mut terminal = 0_usize;

    for ch in text.chars() {
        chars += 1;
        if ch == '\u{fffd}' {
            replacement += 1;
            continue;
        }
        if is_terminal_char(ch) {
            printable += 1;
        }
        if is_cjk(ch) {
            cjk += 1;
        }
        if is_hangul(ch) {
            hangul += 1;
        }
        if is_kana(ch) {
            kana += 1;
        }
        if is_terminal_marker_char(ch) {
            terminal += 1;
        }
    }

    let chars = chars.max(1) as f32;
    let encoding_kind = codec_encoding_kind(encoding);
    let chinese_bonus_weight = if matches!(
        encoding_kind,
        CodecEncodingKind::Chinese | CodecEncodingKind::Big5
    ) {
        0.82
    } else {
        0.18
    };
    let chinese_bonus = (cjk as f32 / chars).min(0.45) * chinese_bonus_weight;
    let hangul_bonus = if encoding_kind == CodecEncodingKind::EucKr {
        (hangul as f32 / chars).min(0.45) * 0.46
    } else {
        0.0
    };
    let kana_bonus = if encoding_kind == CodecEncodingKind::ShiftJis {
        (kana as f32 / chars).min(0.45) * 0.42
    } else {
        0.0
    };
    let nul_penalty = if memchr(0, bytes).is_some() {
        0.18
    } else {
        0.0
    };
    let hangul_penalty = if encoding_kind == CodecEncodingKind::EucKr {
        0.0
    } else {
        (hangul as f32 / chars).min(0.50) * 0.65
    };
    let kana_penalty = if encoding_kind == CodecEncodingKind::ShiftJis {
        0.0
    } else {
        (kana as f32 / chars).min(0.50) * 0.55
    };
    let error_penalty = if had_errors { 0.22 } else { 0.0 };
    ((printable as f32 / chars) * 0.46
        + chinese_bonus
        + hangul_bonus
        + kana_bonus
        + (terminal as f32 / chars).min(0.25) * 0.16
        - (replacement as f32 / chars) * 0.8
        - hangul_penalty
        - kana_penalty
        - nul_penalty
        - error_penalty)
        .clamp(0.0, 1.0)
}

pub(super) fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
    )
}

fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F)
}

fn is_kana(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF)
}

pub(super) fn complete_prefix_len(bytes: &[u8], encoding: &'static Encoding) -> usize {
    if encoding == UTF_8 {
        return complete_utf8_prefix_len(bytes);
    }

    match codec_encoding_kind(encoding) {
        // GBK double-byte sequences are valid GB18030 sequences, so one
        // width function covers both Chinese encodings.
        CodecEncodingKind::Chinese => complete_legacy_prefix_len(bytes, gb18030_char_width),
        CodecEncodingKind::Big5 => complete_legacy_prefix_len(bytes, big5_char_width),
        CodecEncodingKind::ShiftJis => complete_legacy_prefix_len(bytes, shift_jis_char_width),
        CodecEncodingKind::EucKr => complete_legacy_prefix_len(bytes, euc_kr_char_width),
        _ => bytes.len(),
    }
}

pub(super) fn complete_utf8_prefix_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(error) => {
            if error.error_len().is_none() {
                error.valid_up_to()
            } else {
                bytes.len()
            }
        }
    }
}

/// Length of the longest prefix that ends on a character boundary for a
/// legacy multibyte encoding. A truncated tail character (width 0) stops the
/// scan so the tail stays buffered until the rest of the character arrives.
fn complete_legacy_prefix_len(bytes: &[u8], char_width: fn(&[u8], usize) -> usize) -> usize {
    let mut index = 0;
    while index < bytes.len() {
        let width = char_width(bytes, index);
        if width == 0 {
            return index;
        }
        index += width;
    }

    bytes.len()
}

pub(super) fn has_incomplete_legacy_tail(bytes: &[u8]) -> bool {
    [
        gb18030_char_width,
        big5_char_width,
        shift_jis_char_width,
        euc_kr_char_width,
    ]
    .into_iter()
    .any(|char_width| complete_legacy_prefix_len(bytes, char_width) < bytes.len())
}

pub(super) fn gb18030_char_width(bytes: &[u8], index: usize) -> usize {
    let byte = bytes[index];
    if byte <= 0x7f {
        return 1;
    }
    if !matches!(byte, 0x81..=0xfe) {
        return 1;
    }

    let Some(second) = bytes.get(index + 1).copied() else {
        return 0;
    };
    if matches!(second, 0x40..=0x7e | 0x80..=0xfe) {
        return 2;
    }
    if !matches!(second, 0x30..=0x39) {
        return 1;
    }

    let Some(third) = bytes.get(index + 2).copied() else {
        return 0;
    };
    if !matches!(third, 0x81..=0xfe) {
        return 1;
    }

    let Some(fourth) = bytes.get(index + 3).copied() else {
        return 0;
    };
    if matches!(fourth, 0x30..=0x39) {
        4
    } else {
        1
    }
}

pub(super) fn big5_char_width(bytes: &[u8], index: usize) -> usize {
    let byte = bytes[index];
    if byte <= 0x7f {
        return 1;
    }
    if matches!(byte, 0x81..=0xfe) {
        return trail_width(
            bytes,
            index,
            |trail| matches!(trail, 0x40..=0x7e | 0xa1..=0xfe),
        );
    }
    1
}

pub(super) fn shift_jis_char_width(bytes: &[u8], index: usize) -> usize {
    let byte = bytes[index];
    if byte <= 0x7f || matches!(byte, 0xa1..=0xdf) {
        return 1;
    }
    if matches!(byte, 0x81..=0x9f | 0xe0..=0xfc) {
        return trail_width(
            bytes,
            index,
            |trail| matches!(trail, 0x40..=0x7e | 0x80..=0xfc),
        );
    }
    1
}

pub(super) fn euc_kr_char_width(bytes: &[u8], index: usize) -> usize {
    let byte = bytes[index];
    if byte <= 0x7f {
        return 1;
    }
    if matches!(byte, 0xa1..=0xfe) {
        return trail_width(bytes, index, |trail| matches!(trail, 0xa1..=0xfe));
    }
    1
}

pub(super) fn trail_width<F>(bytes: &[u8], index: usize, valid_trail: F) -> usize
where
    F: Fn(u8) -> bool,
{
    match bytes.get(index + 1).copied() {
        Some(trail) if valid_trail(trail) => 2,
        Some(_) => 1,
        None => 0,
    }
}

pub(super) fn encode_for_backend(text: &str, codec: &CodecState) -> Vec<u8> {
    let encoding = active_encoding(codec)
        .map(|(encoding, _)| encoding)
        .unwrap_or(UTF_8);
    let (bytes, _, _) = encoding.encode(text);
    bytes.into_owned()
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SerialSampleQuality {
    pub(super) confidence: f32,
    pub(super) strong_evidence: bool,
}

pub(super) fn analyze_serial_sample(sample: &[u8], encoding: Option<&str>) -> SerialSampleQuality {
    if sample.is_empty() {
        return SerialSampleQuality {
            confidence: 0.0,
            strong_evidence: false,
        };
    }

    let mut codec = CodecState::new(encoding.map(ToOwned::to_owned), true);
    codec.raw_buffer.extend_from_slice(sample);
    let decoded = flush_decoded_backend_bytes_with_raw(&mut codec, RawBytesMode::Disabled);

    let mut chars = 0_usize;
    let mut printable = 0_usize;
    let mut replacement = 0_usize;
    let mut bad_control = 0_usize;
    for ch in decoded.data.chars() {
        chars += 1;
        if ch == '\u{fffd}' {
            replacement += 1;
        } else if is_terminal_char(ch) {
            printable += 1;
        } else {
            bad_control += 1;
        }
    }
    let chars = chars.max(1);

    let mut ascii_terminal = 0_usize;
    let mut bad_bytes = 0_usize;
    let mut terminal_markers = 0_usize;
    for byte in sample {
        if is_terminal_byte(*byte) {
            ascii_terminal += 1;
        }
        if matches!(*byte, 0x00 | 0xff) || *byte < 0x08 {
            bad_bytes += 1;
        }
        if matches!(
            *byte,
            b'\r' | b'\n' | b'\t' | 0x1b | b'<' | b'>' | b':' | b'$' | b'#'
        ) {
            terminal_markers += 1;
        }
    }

    let text = decoded.data.trim_end();
    let lower = text.to_ascii_lowercase();
    let mut human_chars = 0_usize;
    let mut structural_chars = 0_usize;
    for ch in text.chars() {
        if ch.is_alphanumeric()
            || matches!(
                ch,
                ' ' | '_' | '-' | '.' | '/' | '@' | '(' | ')' | '[' | ']'
            )
        {
            human_chars += 1;
        }
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            structural_chars += 1;
        }
    }

    let prompt_evidence = looks_like_serial_prompt(text) || looks_like_login_prompt(&lower);
    let device_evidence = looks_like_network_device_console(&lower);
    let ansi_evidence = sample.len() >= SERIAL_MIN_DETECT_BYTES && decoded.data.contains("\x1b[");
    let newline_evidence = decoded.data.contains('\n') || decoded.data.contains('\r');
    let meaningful_chars = chars.saturating_sub(replacement + bad_control);
    let printable_ratio = printable as f32 / chars as f32;
    let strong_evidence = prompt_evidence
        || device_evidence
        || ansi_evidence
        || (newline_evidence
            && meaningful_chars >= SERIAL_MIN_DETECT_BYTES * 2
            && printable_ratio >= 0.72
            && human_chars >= 4);
    let has_text_structure = strong_evidence || (structural_chars >= 2 && human_chars >= 4);
    let prompt_bonus = if prompt_evidence { 0.20 } else { 0.0 };
    let device_hint_bonus = if device_evidence { 0.16 } else { 0.0 };
    let ansi_bonus = if ansi_evidence { 0.10 } else { 0.0 };
    let newline_bonus = if newline_evidence { 0.07 } else { 0.0 };
    let marker_bonus = ((terminal_markers as f32 / sample.len() as f32).min(0.20)) * 0.25;
    let length_bonus = ((sample.len() as f32 / 48.0).min(1.0)) * 0.07;
    let human_bonus = ((human_chars as f32 / chars as f32).min(0.70)) * 0.14;
    let mut score = (printable as f32 / chars as f32) * 0.34
        + (ascii_terminal as f32 / sample.len() as f32) * 0.22
        + decoded.confidence * 0.10
        + prompt_bonus
        + device_hint_bonus
        + ansi_bonus
        + newline_bonus
        + marker_bonus
        + length_bonus
        + human_bonus
        - (replacement as f32 / chars as f32) * 0.60
        - (bad_control as f32 / chars as f32) * 0.55
        - (bad_bytes as f32 / sample.len() as f32) * 0.55;

    if sample.len() < SERIAL_MIN_DETECT_BYTES && !prompt_evidence {
        score = score.min(0.35);
    } else if !has_text_structure && sample.len() < 32 {
        // A short run of printable bytes is common at a wrong baud. Require
        // prompt/line/ANSI structure or a longer independent text sample.
        score = score.min(0.48);
    }

    SerialSampleQuality {
        confidence: score.clamp(0.0, 1.0),
        strong_evidence,
    }
}

fn is_terminal_char(ch: char) -> bool {
    !ch.is_control() || is_allowed_terminal_control(ch)
}

/// Structural characters that appear in real console output (prompts, line
/// breaks, ANSI escapes). Shared by both scoring paths so they weigh the same
/// evidence.
fn is_terminal_marker_char(ch: char) -> bool {
    matches!(
        ch,
        '\r' | '\n' | '\t' | '\x1b' | '<' | '>' | ':' | '$' | '#'
    )
}

fn is_allowed_terminal_control(ch: char) -> bool {
    matches!(ch, '\r' | '\n' | '\t' | '\x1b' | '\x08' | '\x07')
}

fn is_terminal_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'\r' | b'\n' | b'\t' | 0x1b | 0x08 | 0x07 | 0x20..=0x7e
    )
}

fn looks_like_serial_prompt(text: &str) -> bool {
    let line = text.lines().next_back().unwrap_or(text).trim_end();
    if line.len() < 2 {
        return false;
    }
    let body = line.trim_end_matches(['>', '#', '$', ':', ']', ')']);
    let body_chars = body.chars().filter(|ch| ch.is_alphanumeric()).count();
    if line.starts_with('<') && line.ends_with('>') {
        return body_chars >= 2;
    }
    match line.chars().next_back() {
        Some('>' | '#' | '$') => body_chars >= 2,
        Some(']' | ')') => body_chars >= 2 && (line.contains('[') || line.contains('(')),
        Some(':') => body_chars >= 3,
        _ => false,
    }
}

fn looks_like_login_prompt(lower: &str) -> bool {
    lower.contains("login:")
        || lower.contains("username:")
        || lower.contains("password:")
        || lower.contains("press return")
        || lower.contains("press enter")
}

fn looks_like_network_device_console(lower: &str) -> bool {
    // "login:" style prompts are covered by looks_like_login_prompt; keeping a
    // bare "login" here would double-count the same evidence.
    [
        "console",
        "user access verification",
        "escape character",
        "router",
        "switch",
        "firewall",
        "ap>",
        "ap#",
        "junos",
        "cisco",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_serial_sample, decode_backend_bytes, decode_backend_bytes_with_raw,
        flush_decoded_backend_bytes, select_detected_encoding, RawBytesMode,
    };
    use encoding_rs::Encoding;

    use crate::terminal::internal::core::{CodecState, DETECTION_SAMPLE_MAX_BYTES};

    #[test]
    fn gb18030_tail_is_preserved_across_chunks() {
        let mut codec = CodecState::new(None, true);
        let text = "你好";
        let (encoded, _, _) = Encoding::for_label(b"gb18030")
            .expect("gb18030")
            .encode(text);
        let bytes = encoded.as_ref();

        let first = decode_backend_bytes(&bytes[..3], &mut codec);
        assert!(first.data.is_empty());
        assert!(first.raw_bytes.is_empty());
        assert_eq!(codec.raw_buffer, bytes[..3]);
        assert!(codec.pending_tail.is_empty());

        let second = decode_backend_bytes(&bytes[3..], &mut codec);
        assert_eq!(second.data, text);
        assert_eq!(second.raw_bytes, bytes);
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());

        let flushed = flush_decoded_backend_bytes(&mut codec);
        assert!(flushed.data.is_empty());
        assert!(flushed.raw_bytes.is_empty());
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());
    }

    #[test]
    fn first_small_non_ascii_chunk_is_decoded_immediately() {
        let mut codec = CodecState::new(None, true);
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文");

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        assert_eq!(decoded.data, "中文");
        assert_eq!(decoded.raw_bytes, encoded.as_ref());
        assert!(codec.raw_buffer.is_empty());
        assert!(codec.pending_tail.is_empty());
    }

    #[test]
    fn small_non_ascii_chunk_does_not_lock_detected_encoding() {
        let mut codec = CodecState::new(None, true);
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文");

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        assert_eq!(decoded.data, "中文");
        assert!(codec.detected_encoding.is_none());
        assert_eq!(codec.detected_confidence, 0.0);
    }

    #[test]
    fn medium_sample_can_lock_detected_encoding_without_large_buffer_wait() {
        let mut codec = CodecState::new(None, true);
        let sample = "中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示:";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(sample);

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        assert_eq!(decoded.data, sample);
        let detected = codec
            .detected_encoding
            .as_deref()
            .expect("detected encoding should be locked");
        assert!(matches!(detected, "gbk" | "gb18030"));
        assert!(codec.detected_confidence >= 0.35);
    }

    #[test]
    fn split_utf8_chinese_tail_is_not_decoded_as_legacy_encoding() {
        let mut codec = CodecState::new(None, true);
        let sample = "  inspection  Arp检测\r\n";
        let split = sample.find("检").expect("chinese marker") + 2;

        let first = decode_backend_bytes(&sample.as_bytes()[..split], &mut codec);
        assert_eq!(first.data, "  inspection  Arp");
        assert_eq!(codec.pending_tail, sample.as_bytes()[split - 2..split]);

        let second = decode_backend_bytes(&sample.as_bytes()[split..], &mut codec);
        assert_eq!(second.data, "检测\r\n");
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());
    }

    #[test]
    fn byte_fragmented_utf8_chinese_remains_utf8() {
        let mut codec = CodecState::new(None, true);
        let sample = "  inspection  Arp检测\r\n";
        let mut decoded = String::new();

        for byte in sample.as_bytes() {
            decoded.push_str(&decode_backend_bytes(&[*byte], &mut codec).data);
        }
        decoded.push_str(&flush_decoded_backend_bytes(&mut codec).data);

        assert_eq!(decoded, sample);
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());
    }

    #[test]
    fn detection_winning_decode_is_reused_for_delivery() {
        // The detection pass scores each candidate with a full decode; the
        // winning text must be byte-identical to a direct encoding_rs decode,
        // which is what the delivery path would produce without reuse.
        let mut codec = CodecState::new(None, true);
        let sample = "中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示:";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(sample);

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        let (expected, _, _) = encoding_rs::GBK.decode(encoded.as_ref());
        assert_eq!(decoded.data, expected.as_ref());
    }

    #[test]
    fn oversized_sample_falls_back_to_fresh_decode() {
        // Chunks larger than the detection window score only the tail window,
        // so the delivery pass must decode the full chunk itself.
        let mut codec = CodecState::new(None, true);
        let sample = "状态正常接口已连接".repeat(600);
        let (encoded, _, _) = Encoding::for_label(b"gb18030")
            .expect("gb18030")
            .encode(&sample);
        assert!(encoded.len() > DETECTION_SAMPLE_MAX_BYTES);

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        let (expected, _, _) = encoding_rs::GB18030.decode(encoded.as_ref());
        assert_eq!(decoded.data, expected.as_ref());
    }

    #[test]
    fn valid_utf8_is_decoded_without_legacy_candidate_scoring() {
        let mut codec = CodecState::new(None, true);
        let sample = "系统状态: 正常，接口状态: 已连接\r\n".repeat(12);
        let bytes = sample.as_bytes();
        let (_, expected_confidence) = select_detected_encoding(bytes);

        let decoded = decode_backend_bytes(bytes, &mut codec);

        assert_eq!(decoded.data, sample);
        assert_eq!(decoded.raw_bytes, bytes);
        assert_eq!(decoded.encoding, "utf-8");
        assert_eq!(decoded.confidence, expected_confidence);
        assert_eq!(codec.detected_encoding.as_deref(), Some("utf-8"));
        assert!(codec.raw_buffer.is_empty());
        assert!(codec.pending_tail.is_empty());
    }

    #[test]
    fn valid_utf8_sample_window_preserves_detection_confidence() {
        let mut codec = CodecState::new(None, true);
        // The ASCII suffix makes the 4096-byte detector window start on a
        // character boundary, exercising the same bounded sample as before.
        let sample = format!("{}x", "中文".repeat(850));
        let bytes = sample.as_bytes();
        let (_, expected_confidence) = select_detected_encoding(bytes);

        let decoded = decode_backend_bytes(bytes, &mut codec);

        assert_eq!(decoded.data, sample);
        assert_eq!(decoded.confidence, expected_confidence);
        assert_eq!(decoded.encoding, "utf-8");
    }

    #[test]
    fn gb18030_network_help_text_prefers_chinese_over_korean() {
        let mut codec = CodecState::new(None, true);
        let sample = "  inspection  Arp检测\r\n";
        let (encoded, _, _) = Encoding::for_label(b"gb18030")
            .expect("gb18030")
            .encode(sample);

        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        assert_eq!(decoded.data, sample);
        assert_ne!(decoded.data, "  inspection  Arp쇱꿎\r\n");
        assert!(matches!(decoded.encoding.as_str(), "gbk" | "gb18030"));
    }

    #[test]
    fn strong_utf8_sample_replaces_auto_detected_legacy_encoding() {
        let mut codec = CodecState::new(None, true);
        let legacy_sample = "中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示:";
        let (legacy_bytes, _, _) = Encoding::for_label(b"gbk")
            .expect("gbk")
            .encode(legacy_sample);
        let legacy = decode_backend_bytes(legacy_bytes.as_ref(), &mut codec);
        assert_eq!(legacy.data, legacy_sample);
        assert!(matches!(
            codec.detected_encoding.as_deref(),
            Some("gbk" | "gb18030")
        ));

        let utf8_sample = "系统状态: 正常\r\n接口状态: 已连接\r\n";
        let decoded = decode_backend_bytes(utf8_sample.as_bytes(), &mut codec);

        assert_eq!(decoded.data, utf8_sample);
        assert_eq!(decoded.encoding, "utf-8");
        assert_eq!(codec.detected_encoding.as_deref(), Some("utf-8"));
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());
    }

    #[test]
    fn split_utf8_after_auto_detected_legacy_is_not_consumed_as_legacy() {
        let mut codec = CodecState::new(None, true);
        let legacy_sample = "中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示:";
        let (legacy_bytes, _, _) = Encoding::for_label(b"gbk")
            .expect("gbk")
            .encode(legacy_sample);
        let legacy = decode_backend_bytes(legacy_bytes.as_ref(), &mut codec);
        assert_eq!(legacy.data, legacy_sample);
        assert!(matches!(
            codec.detected_encoding.as_deref(),
            Some("gbk" | "gb18030")
        ));

        let utf8_sample = "检测\r\n";
        let first = decode_backend_bytes(&utf8_sample.as_bytes()[..2], &mut codec);
        assert!(first.data.is_empty());
        assert_eq!(codec.pending_tail, utf8_sample.as_bytes()[..2]);

        let second = decode_backend_bytes(&utf8_sample.as_bytes()[2..], &mut codec);
        assert_eq!(second.data, utf8_sample);
        assert_eq!(second.encoding, "utf-8");
        assert_eq!(codec.detected_encoding.as_deref(), Some("utf-8"));
        assert!(codec.pending_tail.is_empty());
        assert!(codec.raw_buffer.is_empty());
    }

    #[test]
    fn enabling_detection_preserves_buffered_bytes() {
        let mut codec = CodecState::new(None, false);
        let sample = [0xe4, 0xbd];
        let pending = decode_backend_bytes(&sample, &mut codec);
        assert!(pending.data.is_empty());
        assert_eq!(codec.raw_buffer, sample);

        codec.set_realtime_detection(true);
        assert_eq!(codec.raw_buffer, sample);
        assert!(codec.pending_tail.is_empty());
    }

    #[test]
    fn serial_sample_scoring_accepts_real_prompts() {
        let quality = analyze_serial_sample(b"router#\r\n", None);

        assert!(quality.strong_evidence);
        assert!(quality.confidence >= 0.52);
    }

    #[test]
    fn serial_sample_scoring_rejects_short_control_noise() {
        let quality = analyze_serial_sample(&[0x01, 0x03, 0x05, b'\n'], None);

        assert!(!quality.strong_evidence);
        assert!(quality.confidence < 0.52);
    }

    #[test]
    fn disabling_detection_clears_detected_encoding_and_stops_fallback_detection() {
        let mut codec = CodecState::new(None, true);
        codec.detected_encoding = Some("gbk".to_string());
        codec.detected_confidence = 0.97;

        codec.set_realtime_detection(false);

        assert!(codec.detected_encoding.is_none());
        assert_eq!(codec.detected_confidence, 0.0);

        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文\r\n");
        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
        assert_eq!(decoded.encoding, "utf-8");
        assert_ne!(decoded.data, "中文\r\n");
        assert!(codec.detected_encoding.is_none());
    }

    #[test]
    fn backend_encoding_overrides_disabled_realtime_detection() {
        let mut codec = CodecState::new(None, false);
        codec.set_backend_encoding("gbk");

        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文\r\n");
        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
        assert_eq!(decoded.data, "中文\r\n");
        assert_eq!(decoded.encoding, "gbk");
        assert_eq!(decoded.confidence, 1.0);
    }

    #[test]
    fn clearing_backend_encoding_restores_realtime_detection() {
        let mut codec = CodecState::new(Some("gbk".to_string()), false);
        codec.clear_backend_encoding();
        codec.set_realtime_detection(true);

        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文\r\n");
        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);

        assert_eq!(decoded.data, "中文\r\n");
        assert_ne!(decoded.encoding, "utf-8");
        assert!(codec.backend_encoding.is_none());
        assert!(codec.realtime_detection_enabled);
    }

    #[test]
    fn manual_encoding_is_used_for_telnet_compatible_chunks() {
        let mut codec = CodecState::new(Some("gbk".to_string()), true);
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode("中文\r\n");
        let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
        assert_eq!(decoded.data, "中文\r\n");
        assert_eq!(decoded.raw_bytes, encoded.as_ref());
        assert_eq!(decoded.encoding, "gbk");
    }

    #[test]
    fn raw_bytes_can_be_omitted_for_protocols_without_raw_output() {
        let mut codec = CodecState::new(None, true);
        let decoded =
            decode_backend_bytes_with_raw(b"serial output\r\n", &mut codec, RawBytesMode::Disabled);

        assert_eq!(decoded.data, "serial output\r\n");
        assert!(decoded.raw_bytes.is_empty());
    }

    #[test]
    fn detect_mode_only_keeps_raw_bytes_for_trzsz_trigger_chunks() {
        let mut codec = CodecState::new(None, true);
        let plain =
            decode_backend_bytes_with_raw(b"plain output\r\n", &mut codec, RawBytesMode::Detect);
        assert_eq!(plain.data, "plain output\r\n");
        assert!(plain.raw_bytes.is_empty());

        let trigger = b"before ::TRZSZ:TRANSFER:xyz\r\n";
        let triggered = decode_backend_bytes_with_raw(trigger, &mut codec, RawBytesMode::Detect);
        assert_eq!(triggered.raw_bytes, trigger);

        let mut armed = CodecState::new(None, true);
        let kept =
            decode_backend_bytes_with_raw(b"plain again\r\n", &mut armed, RawBytesMode::Armed);
        assert_eq!(kept.raw_bytes, b"plain again\r\n");
    }

    #[test]
    fn short_printable_noise_is_not_treated_as_reliable_serial_text() {
        let quality = analyze_serial_sample(b"abcd1234", None);

        assert!(!quality.strong_evidence);
        assert!(quality.confidence < 0.52);
    }

    #[test]
    fn long_unstructured_printable_run_still_lacks_confirmation_evidence() {
        let quality = analyze_serial_sample(
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            None,
        );

        assert!(!quality.strong_evidence);
    }

    #[test]
    fn prompt_and_line_structure_are_strong_serial_evidence() {
        let prompt = analyze_serial_sample(b"Router#\r\n", None);
        let ansi = analyze_serial_sample(b"\x1b[32mready\x1b[0m\r\n", None);

        assert!(prompt.strong_evidence);
        assert!(ansi.strong_evidence);
        assert!(prompt.confidence >= 0.52);
        assert!(ansi.confidence >= 0.52);
    }

    #[test]
    fn encoded_console_text_scores_above_binary_line_noise() {
        let (encoded, _, _) = Encoding::for_label(b"gbk")
            .expect("gbk")
            .encode("设备启动完成\r\n登录:");
        let text_score = analyze_serial_sample(encoded.as_ref(), Some("gbk")).confidence;
        let noise_score = analyze_serial_sample(
            &[0x00, 0xff, 0x01, 0xfe, 0x00, 0x7f, 0x03, 0xff],
            Some("gbk"),
        )
        .confidence;

        assert!(text_score >= 0.52);
        assert!(text_score > noise_score);
    }

    #[test]
    fn repeated_short_gbk_lines_lock_via_accumulated_evidence() {
        // 交换机典型流量：ASCII CLI 之间夹短促中文行（描述/日志）。单行
        // 14 字节远低于任何可提交分段，改造前每行独立检测、永不锁定；
        // 跨块积累后样本量进入可提交分段，应锁定且每行都解码正确。
        let mut codec = CodecState::new(None, true);
        let line = "接口状态正常\r\n";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(line);
        let mut locked_at = None;

        for index in 0..12 {
            let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
            assert_eq!(decoded.data, line);
            if locked_at.is_none() && codec.detected_encoding.is_some() {
                locked_at = Some(index);
            }
        }

        assert!(locked_at.is_some(), "accumulated evidence should lock");
        assert!(matches!(
            codec.detected_encoding.as_deref(),
            Some("gbk" | "gb18030")
        ));
        assert!(codec.detection_history.is_empty());
    }

    #[test]
    fn accumulated_history_is_never_redelivered() {
        // 历史只参与检测评分，不得混入交付文本。
        let mut codec = CodecState::new(None, true);
        let line = "端口描述办公网\r\n";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(line);
        let mut delivered = String::new();

        for _ in 0..6 {
            delivered.push_str(&decode_backend_bytes(encoded.as_ref(), &mut codec).data);
        }

        assert_eq!(delivered, line.repeat(6));
    }

    #[test]
    fn ascii_chunks_do_not_erase_accumulated_evidence() {
        // 交换机输出中中文行之间隔着大量纯 ASCII 行；ASCII 对编码判定无贡献，
        // 不得清空已积累的中文证据。
        let mut codec = CodecState::new(None, true);
        let chinese = "接口状态正常\r\n";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(chinese);

        for index in 0..10 {
            let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
            assert_eq!(decoded.data, chinese);
            let ascii = decode_backend_bytes(b"  GigabitEthernet0/0/1 up\r\n", &mut codec);
            assert_eq!(ascii.data, "  GigabitEthernet0/0/1 up\r\n");
            if codec.detected_encoding.is_some() {
                assert!(
                    index >= 2,
                    "lock should come from accumulated evidence, not the first line"
                );
                break;
            }
            assert!(!codec.detection_history.is_empty());
        }

        assert!(matches!(
            codec.detected_encoding.as_deref(),
            Some("gbk" | "gb18030")
        ));
    }

    #[test]
    fn short_big5_traditional_sample_is_arbitrated_to_big5() {
        // 繁体 Big5 短样本按 GB18030 解码同样落在合法 CJK 区块，评分近平局；
        // 统计仲裁应把胜者判给 Big5。
        let sample = "繁體中文資訊網路".repeat(2);
        let (encoded, _, _) = Encoding::for_label(b"big5").expect("big5").encode(&sample);

        let (encoding, _) = select_detected_encoding(encoded.as_ref());

        assert_eq!(encoding, encoding_rs::BIG5);
    }

    #[test]
    fn locked_encoding_keeps_history_cleared() {
        let mut codec = CodecState::new(None, true);
        let line = "接口状态正常\r\n";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(line);

        for _ in 0..12 {
            decode_backend_bytes(encoded.as_ref(), &mut codec);
            if codec.detected_encoding.is_some() {
                break;
            }
        }
        assert!(codec.detected_encoding.is_some());

        for _ in 0..3 {
            let decoded = decode_backend_bytes(encoded.as_ref(), &mut codec);
            assert_eq!(decoded.data, line);
            assert!(codec.detection_history.is_empty());
        }
    }

    #[test]
    fn ascii_chunks_do_not_reset_consecutive_decode_errors() {
        // 交换机流量常态：中文行之间夹大量 ASCII 行。错误恢复要求"连续 3 个
        // 带错块"才解锁重检，ASCII 块对编码判定零贡献，不得打断计数。
        let mut codec = CodecState::new(None, true);
        let sample = "中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示: 中文命令提示> 中文命令提示# 中文命令提示$ 中文命令提示:";
        let (encoded, _, _) = Encoding::for_label(b"gbk").expect("gbk").encode(sample);
        decode_backend_bytes(encoded.as_ref(), &mut codec);
        assert!(codec.detected_encoding.is_some());

        // 0xFF 在所有候选编码下都无法解码（产生替换符），即"锁定编码错误"
        // 的代理；且不是任何编码的前导字节，不会形成撕裂尾部粘住下一块。
        decode_backend_bytes(&[0xff, 0xff], &mut codec);
        assert_eq!(codec.consecutive_errors, 1);

        let ascii = decode_backend_bytes(b"display interface brief\r\n", &mut codec);
        assert_eq!(ascii.data, "display interface brief\r\n");
        assert_eq!(codec.consecutive_errors, 1);

        decode_backend_bytes(&[0xff, 0xff], &mut codec);
        decode_backend_bytes(b"up up\r\n", &mut codec);
        assert_eq!(codec.consecutive_errors, 2);

        // 第三个带错块达成阈值：已提交编码被清除（解锁），下一块起重新检测。
        decode_backend_bytes(&[0xff, 0xff], &mut codec);
        assert_eq!(codec.consecutive_errors, 3);
        assert!(
            codec.detected_encoding.is_none(),
            "3 consecutive error chunks should unlock the committed encoding"
        );
    }
}
