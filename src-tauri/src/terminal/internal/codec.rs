use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, BIG5, EUC_KR, GB18030, GBK, SHIFT_JIS, UTF_8};
use memchr::memchr;

use crate::terminal::internal::core::{
    CodecState, ConnectionReadResult, CODEC_RAW_BUFFER_CAPACITY, DETECTION_ERROR_UNLOCK_THRESHOLD,
    DETECTION_LOCK_CONFIDENCE, DETECTION_SAMPLE_MAX_BYTES, SERIAL_MIN_DETECT_BYTES,
};

/// Confidence reported for a buffer that is valid UTF-8 except for a
/// truncated tail character (stream chunking artifact, not real content).
const UTF8_PREFIX_CONFIDENCE: f32 = 0.86;

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
    decode_backend_bytes_with_raw(bytes, codec, true)
}

pub(super) fn decode_backend_bytes_with_raw(
    bytes: &[u8],
    codec: &mut CodecState,
    include_raw_bytes: bool,
) -> ConnectionReadResult {
    if bytes.is_empty() {
        return empty_decode_result(codec);
    }

    if codec.raw_buffer.is_empty() && codec.pending_tail.is_empty() && bytes.is_ascii() {
        let (encoding, confidence) =
            active_encoding(codec).unwrap_or((UTF_8, default_confidence(codec, UTF_8)));
        return ConnectionReadResult {
            data: String::from_utf8(bytes.to_vec()).expect("ASCII bytes are valid UTF-8"),
            raw_bytes: if include_raw_bytes {
                bytes.to_vec()
            } else {
                Vec::new()
            },
            encoding: encoding_label(encoding),
            confidence,
        };
    }

    codec.raw_buffer.extend_from_slice(bytes);
    decode_buffered_bytes(codec, false, include_raw_bytes)
}

pub(super) fn flush_decoded_backend_bytes(codec: &mut CodecState) -> ConnectionReadResult {
    flush_decoded_backend_bytes_with_raw(codec, true)
}

pub(super) fn flush_decoded_backend_bytes_with_raw(
    codec: &mut CodecState,
    include_raw_bytes: bool,
) -> ConnectionReadResult {
    if codec.raw_buffer.is_empty() && codec.pending_tail.is_empty() {
        return empty_decode_result(codec);
    }

    decode_buffered_bytes(codec, true, include_raw_bytes)
}

fn decode_buffered_bytes(
    codec: &mut CodecState,
    force: bool,
    include_raw_bytes: bool,
) -> ConnectionReadResult {
    let undecoded = take_undecoded_bytes(codec);
    if undecoded.is_empty() {
        return empty_decode_result(codec);
    }

    if let Some(result) =
        decode_complete_utf8_prefix_before_detection(codec, &undecoded, force, include_raw_bytes)
    {
        if result.data.is_empty() && result.raw_bytes.is_empty() {
            restore_undecoded_bytes(codec, undecoded);
        }
        return result;
    }

    if should_wait_for_complete_provisional_sample(codec, &undecoded, force) {
        restore_undecoded_bytes(codec, undecoded);
        return ConnectionReadResult {
            data: String::new(),
            raw_bytes: Vec::new(),
            encoding: encoding_label(UTF_8),
            confidence: default_confidence(codec, UTF_8),
        };
    }

    if let Some(result) =
        decode_utf8_candidate_for_auto_detected_legacy(codec, &undecoded, force, include_raw_bytes)
    {
        return result;
    }

    let (mut encoding, mut confidence) = select_encoding_for_decode(&undecoded, codec, force);
    let mut chunk = decode_complete_chunk(&undecoded, encoding, force);

    if matches!(chunk, Some((_, _, _, true))) && should_recover_from_decode_errors(codec) {
        clear_detected_encoding(codec);
        (encoding, confidence) = select_encoding_for_decode(&undecoded, codec, true);
        chunk = decode_complete_chunk(&undecoded, encoding, force);
    }

    let Some((data, complete_len, had_non_ascii, had_errors)) = chunk else {
        restore_undecoded_bytes(codec, undecoded);
        return ConnectionReadResult {
            data: String::new(),
            raw_bytes: Vec::new(),
            encoding: encoding_label(encoding),
            confidence,
        };
    };

    codec.pending_tail = undecoded[complete_len..].to_vec();
    apply_decode_feedback(
        codec,
        encoding,
        confidence,
        had_non_ascii,
        had_errors,
        complete_len,
    );

    ConnectionReadResult {
        data,
        raw_bytes: if include_raw_bytes {
            undecoded[..complete_len].to_vec()
        } else {
            Vec::new()
        },
        encoding: encoding_label(encoding),
        confidence: if had_errors {
            confidence * 0.65
        } else {
            confidence
        },
    }
}

/// Decodes the longest complete-character prefix of `undecoded`. Returns
/// `(text, consumed_len, had_non_ascii, had_errors)`, or `None` when the
/// buffer ends (or starts) mid-character and nothing can be emitted yet.
fn decode_complete_chunk(
    undecoded: &[u8],
    encoding: &'static Encoding,
    force: bool,
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
    if codec.backend_encoding.is_some() {
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
    if codec.realtime_detection_enabled
        && had_non_ascii
        && should_commit_detected_encoding(decoded_len, confidence)
    {
        codec.detected_encoding = Some(encoding_label(encoding));
        codec.detected_encoding_handle = Some(encoding);
        codec.detected_confidence = confidence.max(codec.detected_confidence);
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
}

fn should_wait_for_complete_provisional_sample(
    codec: &CodecState,
    bytes: &[u8],
    force: bool,
) -> bool {
    !force
        && codec.backend_encoding.is_none()
        && codec.detected_encoding.is_none()
        && codec.realtime_detection_enabled
        && !bytes.is_ascii()
        && std::str::from_utf8(bytes).is_err()
        && has_incomplete_legacy_tail(bytes)
}

/// Splits a buffer whose only UTF-8 flaw is a tail truncated mid-character
/// (a stream chunking artifact). Returns the valid prefix and its byte
/// length; `None` when the buffer is valid UTF-8 or contains genuinely
/// invalid sequences.
fn split_truncated_utf8_prefix(bytes: &[u8]) -> Option<(&str, usize)> {
    let error = std::str::from_utf8(bytes).err()?;
    if error.error_len().is_some() {
        return None;
    }
    let complete_len = error.valid_up_to();
    let prefix = std::str::from_utf8(&bytes[..complete_len]).ok()?;
    Some((prefix, complete_len))
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

fn decode_complete_utf8_prefix_before_detection(
    codec: &mut CodecState,
    bytes: &[u8],
    force: bool,
    include_raw_bytes: bool,
) -> Option<ConnectionReadResult> {
    if force
        || codec.backend_encoding.is_some()
        || codec.detected_encoding.is_some()
        || !codec.realtime_detection_enabled
    {
        return None;
    }

    let (prefix, complete_len) = split_truncated_utf8_prefix(bytes)?;
    if complete_len == 0 {
        return Some(empty_utf8_hold_result());
    }

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

    Some(ConnectionReadResult {
        data: prefix.to_string(),
        raw_bytes: if include_raw_bytes {
            bytes[..complete_len].to_vec()
        } else {
            Vec::new()
        },
        encoding: encoding_label(UTF_8),
        confidence: UTF8_PREFIX_CONFIDENCE,
    })
}

fn decode_utf8_candidate_for_auto_detected_legacy(
    codec: &mut CodecState,
    bytes: &[u8],
    force: bool,
    include_raw_bytes: bool,
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
        return Some(ConnectionReadResult {
            data: text.to_string(),
            raw_bytes: if include_raw_bytes {
                bytes.to_vec()
            } else {
                Vec::new()
            },
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
        raw_bytes: if include_raw_bytes {
            bytes[..complete_len].to_vec()
        } else {
            Vec::new()
        },
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
        64..=127 => 0.84,
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

fn restore_undecoded_bytes(codec: &mut CodecState, bytes: Vec<u8>) {
    codec.raw_buffer = bytes;
    codec.pending_tail.clear();
}

fn active_encoding(codec: &CodecState) -> Option<(&'static Encoding, f32)> {
    if let Some(encoding) = codec.backend_encoding_handle {
        return Some((encoding, 1.0));
    }

    if codec.realtime_detection_enabled {
        if let Some(encoding) = codec.detected_encoding_handle {
            return Some((encoding, codec.detected_confidence.max(0.35)));
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
) -> (&'static Encoding, f32) {
    if let Some((encoding, confidence)) = active_encoding(codec) {
        return (encoding, confidence);
    }

    if bytes.is_ascii() {
        return (UTF_8, 0.98);
    }

    if !codec.realtime_detection_enabled {
        return (UTF_8, default_confidence(codec, UTF_8));
    }

    let should_refresh = codec.detected_confidence < DETECTION_LOCK_CONFIDENCE
        || codec.consecutive_errors >= DETECTION_ERROR_UNLOCK_THRESHOLD;
    if !should_refresh {
        return active_encoding(codec).unwrap_or((UTF_8, default_confidence(codec, UTF_8)));
    }

    let (encoding, confidence) = select_detected_encoding(bytes);
    maybe_commit_detected_encoding(codec, bytes.len(), encoding, confidence, force);
    (encoding, confidence)
}

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
    if codec.detected_encoding_handle == Some(encoding) && confidence <= codec.detected_confidence {
        return;
    }
    if confidence >= codec.detected_confidence
        || codec.detected_confidence < DETECTION_LOCK_CONFIDENCE
    {
        codec.detected_encoding = Some(encoding_label(encoding));
        codec.detected_encoding_handle = Some(encoding);
        codec.detected_confidence = confidence;
    }
}

pub(super) fn select_detected_encoding(bytes: &[u8]) -> (&'static Encoding, f32) {
    let start = bytes.len().saturating_sub(DETECTION_SAMPLE_MAX_BYTES);
    let sample = &bytes[start..];
    if let Ok(text) = std::str::from_utf8(sample) {
        let confidence = utf8_sample_confidence(sample, text);
        return (UTF_8, confidence);
    }

    let candidates = [GB18030, BIG5, SHIFT_JIS, EUC_KR];
    let mut best = (candidates[0], score_decoded_sample(sample, candidates[0]));
    let mut runner_up = 0.0_f32;

    for encoding in candidates.into_iter().skip(1) {
        let score = score_decoded_sample(sample, encoding);
        if score > best.1 {
            runner_up = best.1;
            best = (encoding, score);
        } else if score > runner_up {
            runner_up = score;
        }
    }

    if should_consult_statistical_detector(sample.len(), best.1, runner_up) {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
        detector.feed(sample, true);
        let guessed = detector.guess(None, Utf8Detection::Allow);
        let guessed_score = score_decoded_sample(sample, guessed) + 0.03;
        if guessed_score > best.1 {
            runner_up = best.1;
            best = (guessed, guessed_score);
        } else if guessed_score > runner_up {
            runner_up = guessed_score;
        }
    }

    let confidence = finalize_detection_confidence(sample.len(), best.1, runner_up);
    (best.0, confidence)
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

pub(super) fn score_decoded_sample(bytes: &[u8], encoding: &'static Encoding) -> f32 {
    let (text, _, had_errors) = encoding.decode(bytes);
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
    let decoded = flush_decoded_backend_bytes(&mut codec);

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
    let mut ascii_human = 0_usize;
    let mut structural_chars = 0_usize;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                ' ' | '_' | '-' | '.' | '/' | '@' | '(' | ')' | '[' | ']'
            )
        {
            ascii_human += 1;
        }
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            structural_chars += 1;
        }
    }

    let prompt_evidence = looks_like_serial_prompt(text) || looks_like_login_prompt(&lower);
    let device_evidence = looks_like_network_device_console(&lower);
    let ansi_evidence = decoded.data.contains("\x1b[");
    let newline_evidence = decoded.data.contains('\n') || decoded.data.contains('\r');
    let meaningful_chars = chars.saturating_sub(replacement + bad_control);
    let strong_evidence = prompt_evidence
        || device_evidence
        || ansi_evidence
        || (newline_evidence && meaningful_chars >= SERIAL_MIN_DETECT_BYTES);
    let has_text_structure = strong_evidence || (structural_chars > 0 && ascii_human >= 3);
    let prompt_bonus = if prompt_evidence { 0.20 } else { 0.0 };
    let device_hint_bonus = if device_evidence { 0.16 } else { 0.0 };
    let ansi_bonus = if ansi_evidence { 0.10 } else { 0.0 };
    let newline_bonus = if newline_evidence { 0.07 } else { 0.0 };
    let marker_bonus = ((terminal_markers as f32 / sample.len() as f32).min(0.20)) * 0.25;
    let length_bonus = ((sample.len() as f32 / 48.0).min(1.0)) * 0.07;
    let human_bonus = ((ascii_human as f32 / chars as f32).min(0.70)) * 0.14;
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
    let trimmed = text.trim_end();
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        return true;
    }
    matches!(
        trimmed.chars().next_back(),
        Some('>' | '#' | '$' | ':' | ']' | ')')
    )
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
        flush_decoded_backend_bytes,
    };
    use encoding_rs::Encoding;

    use crate::terminal::internal::core::CodecState;

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
        let decoded = decode_backend_bytes_with_raw(b"serial output\r\n", &mut codec, false);

        assert_eq!(decoded.data, "serial output\r\n");
        assert!(decoded.raw_bytes.is_empty());
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
}
