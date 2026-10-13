use std::collections::VecDeque;

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use memchr::memchr;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    state::AppState,
    terminal::{
        api::dto::{TerminalEventEnvelope, TerminalSessionChannelPayload},
        internal::{
            codec::flush_decoded_backend_bytes_with_raw,
            core::{
                CodecState, TerminalWorkingDirectoryEvent, OUTPUT_FLUSH_MAX_BYTES,
                REPLAY_CACHE_MAX_BYTES,
            },
            osc::extract_working_directories,
            util::RemoteWorkingDirectory,
        },
    },
};

const REPLAY_BATCH_MAX_BYTES: usize = 64 * 1024;
const TRZSZ_TRIGGER_BYTES: &[u8] = b"::TRZSZ:TRANSFER:";
/// 尾块合并的尺寸上限：低波特率下输出以每秒数百个几字节小块到达，逐块入缓存
/// 会把分配/记账开销放大数十倍；同编码的纯文本小块直接追加进尾块。超过上限
/// 另起新块，保持裁剪与回放批次的粒度有界。
const CACHE_CHUNK_MERGE_MAX_BYTES: usize = 32 * 1024;

#[derive(Clone)]
struct CachedOutputChunk {
    start_offset: usize,
    end_offset: usize,
    encoding: String,
    data: String,
    raw_bytes: Vec<u8>,
}

impl CachedOutputChunk {
    fn cached_bytes(&self) -> usize {
        self.data.len() + self.raw_bytes.len()
    }

    /// 就地裁掉块首 `trim_index` 字节的文本，返回被移除的 raw bytes 数（用于
    /// cache_bytes 记账）。raw bytes 与文本等长时按比例同步裁剪；不等长说明
    /// 文本经过重编码、字节坐标已无法对应，整体丢弃 raw，避免把错位字节当作
    /// 原始输出交付。
    fn trim_prefix(&mut self, trim_index: usize) -> usize {
        let raw_aligned = !self.raw_bytes.is_empty() && self.raw_bytes.len() == self.data.len();
        let removed_raw_bytes = if self.raw_bytes.is_empty() {
            0
        } else if raw_aligned {
            trim_index
        } else {
            self.raw_bytes.len()
        };
        self.data.drain(..trim_index);
        if raw_aligned {
            self.raw_bytes.drain(..trim_index);
        } else {
            self.raw_bytes.clear();
        }
        self.start_offset += trim_index;
        removed_raw_bytes
    }
}

pub(super) struct SessionDeliveryState {
    pub(super) connection_id: String,
    pub(super) raw_bytes_supported: bool,
    pub(super) active_channel_id: Option<u64>,
    pub(super) output_ready_channel_id: Option<u64>,
    pub(super) replay_channel_id: Option<u64>,
    pub(super) raw_output_channel_id: Option<u64>,
    /// Remembers whether raw output was requested (via SetRawOutput or trzsz
    /// trigger auto-arm) so a re-attached channel re-arms raw delivery; the
    /// frontend does not resend SetRawOutput on re-attach.
    pub(super) raw_output_enabled: bool,
    pub(super) last_input_sequence: Option<u64>,
    base_offset: usize,
    pub(super) delivered_offset: usize,
    pub(super) next_offset: usize,
    cache: VecDeque<CachedOutputChunk>,
    cache_bytes: usize,
    /// Index of the first cache chunk that may still hold undelivered bytes;
    /// every chunk before it is fully delivered and skipped by emit scans.
    delivery_cursor: usize,
}

impl SessionDeliveryState {
    pub(super) fn new(connection_id: String, raw_bytes_supported: bool) -> Self {
        Self {
            connection_id,
            raw_bytes_supported,
            active_channel_id: None,
            output_ready_channel_id: None,
            replay_channel_id: None,
            raw_output_channel_id: None,
            raw_output_enabled: false,
            last_input_sequence: None,
            base_offset: 0,
            delivered_offset: 0,
            next_offset: 0,
            cache: VecDeque::new(),
            cache_bytes: 0,
            delivery_cursor: 0,
        }
    }

    fn sync_delivery_cursor(&mut self) {
        while self
            .cache
            .get(self.delivery_cursor)
            .is_some_and(|chunk| chunk.end_offset <= self.delivered_offset)
        {
            self.delivery_cursor += 1;
        }
    }

    fn push_cached_output(&mut self, data: String, encoding: String, raw_bytes: Option<Vec<u8>>) {
        let raw_bytes = raw_bytes.unwrap_or_default();
        let preserve_raw_bytes = self.raw_bytes_supported
            && (self.raw_output_channel_id.is_some()
                || raw_bytes_contain_trzsz_trigger(&raw_bytes));
        if self.raw_output_channel_id.is_none() && preserve_raw_bytes {
            self.raw_output_channel_id = self.active_channel_id;
            self.raw_output_enabled = true;
        }
        let cached_bytes = data.len()
            + if preserve_raw_bytes {
                raw_bytes.len()
            } else {
                0
            };
        let end_offset = self.next_offset + data.len();

        // 合并条件：双方都不携带 raw bytes（trzsz/raw 模式下的块必须保持字节级
        // 边界）且编码一致。raw 通道一旦武装，之后的块都会保留 raw bytes，自然
        // 落到独立入缓存的分支。
        let merge_into_tail = !preserve_raw_bytes
            && self.cache.back().is_some_and(|tail| {
                tail.raw_bytes.is_empty()
                    && tail.encoding == encoding
                    && tail.data.len() + data.len() <= CACHE_CHUNK_MERGE_MAX_BYTES
            });
        if merge_into_tail {
            let tail_index = self.cache.len() - 1;
            let tail = self.cache.back_mut().expect("tail chunk must exist");
            tail.data.push_str(&data);
            tail.end_offset = end_offset;
            // 尾块若已完全交付，游标可能已越过它；合并后它重新包含未交付字节，
            // 必须把游标退回尾块，否则追加的数据会被 emit 扫描永久跳过。
            self.delivery_cursor = self.delivery_cursor.min(tail_index);
        } else {
            self.cache.push_back(CachedOutputChunk {
                start_offset: self.next_offset,
                end_offset,
                encoding,
                data,
                raw_bytes: if preserve_raw_bytes {
                    raw_bytes
                } else {
                    Vec::new()
                },
            });
        }
        self.next_offset = end_offset;
        self.cache_bytes += cached_bytes;
        prune_terminal_replay_cache_bytes(self);
    }
}

pub(super) fn flush_terminal_output(
    app: &AppHandle,
    session_id: &str,
    codec: &mut CodecState,
    delivery: &mut SessionDeliveryState,
) {
    let tail = flush_decoded_backend_bytes_with_raw(codec, delivery.raw_bytes_supported);
    if !tail.data.is_empty() {
        emit_terminal_data(
            app,
            session_id,
            delivery,
            tail.data,
            tail.encoding,
            Some(tail.raw_bytes),
        );
    }
    drain_live_output(app, session_id, delivery);
}

pub(super) fn emit_terminal_data(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
    data: String,
    encoding: String,
    raw_bytes: Option<Vec<u8>>,
) {
    if data.is_empty() {
        return;
    }

    cache_terminal_data(app, session_id, delivery, data, encoding, raw_bytes);
    // 输出不攒批：数据进缓存后由会话循环在同一回合内立即冲刷。
}

pub(super) fn should_flush_live_output(delivery: &SessionDeliveryState) -> bool {
    delivery.output_ready_channel_id.is_some()
        && delivery.replay_channel_id.is_none()
        && pending_live_output_bytes(delivery) > 0
}

pub(super) fn drain_terminal_replay(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
) {
    let Some(channel_id) = delivery.replay_channel_id else {
        return;
    };
    if delivery.output_ready_channel_id != Some(channel_id) {
        return;
    }
    if delivery.delivered_offset >= delivery.next_offset {
        delivery.replay_channel_id = None;
        return;
    }
    if matches!(
        emit_output_batch(
            app,
            session_id,
            delivery,
            channel_id,
            REPLAY_BATCH_MAX_BYTES,
        ),
        TerminalOutputDelivery::NoSubscriber
    ) {
        return;
    }
    if delivery.delivered_offset >= delivery.next_offset {
        delivery.replay_channel_id = None;
    }
}

pub(super) fn drain_live_output(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
) {
    let Some(channel_id) = delivery.output_ready_channel_id else {
        return;
    };
    while delivery.delivered_offset < delivery.next_offset {
        if matches!(
            emit_output_batch(
                app,
                session_id,
                delivery,
                channel_id,
                OUTPUT_FLUSH_MAX_BYTES,
            ),
            TerminalOutputDelivery::NoSubscriber
        ) {
            return;
        }
    }
}

fn pending_live_output_bytes(delivery: &SessionDeliveryState) -> usize {
    delivery
        .next_offset
        .saturating_sub(delivery.delivered_offset)
}

fn cache_terminal_data(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
    data: String,
    encoding: String,
    raw_bytes: Option<Vec<u8>>,
) {
    for directory in extract_working_directories(&data) {
        emit_working_directory(app, session_id, directory);
    }
    delivery.push_cached_output(data, encoding, raw_bytes);
}

fn raw_bytes_contain_trzsz_trigger(raw_bytes: &[u8]) -> bool {
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

/// 回放缓存的字节硬上限。裁剪保留最新尾部，让重新挂载的前端看到最近的若干屏；
/// 行数裁剪由前端 scrollback（xterm 写入时自行按行数截断）承担，后端不再重复
/// 维护行计数。
fn prune_terminal_replay_cache_bytes(delivery: &mut SessionDeliveryState) {
    while delivery.cache_bytes > REPLAY_CACHE_MAX_BYTES {
        let Some(mut chunk) = delivery.cache.pop_front() else {
            delivery.cache_bytes = 0;
            delivery.delivery_cursor = 0;
            delivery.base_offset = delivery.next_offset;
            delivery.delivered_offset = delivery.base_offset;
            return;
        };
        delivery.delivery_cursor = delivery.delivery_cursor.saturating_sub(1);

        let excess = delivery.cache_bytes - REPLAY_CACHE_MAX_BYTES;
        let chunk_bytes = chunk.cached_bytes();
        let trim_index = previous_char_boundary(&chunk.data, excess.min(chunk.data.len()));
        if chunk_bytes <= excess || trim_index == 0 || trim_index == chunk.data.len() {
            // The whole chunk must go to get below the cap (or the cap cut
            // lands inside a single leading multi-byte character).
            delivery.cache_bytes = delivery.cache_bytes.saturating_sub(chunk_bytes);
            delivery.base_offset = chunk.end_offset;
            continue;
        }

        let removed_raw_bytes = chunk.trim_prefix(trim_index);
        delivery.cache_bytes = delivery
            .cache_bytes
            .saturating_sub(trim_index + removed_raw_bytes);
        delivery.base_offset = chunk.start_offset;
        delivery.cache.push_front(chunk);
    }

    if delivery.cache.is_empty() {
        delivery.base_offset = delivery.next_offset;
        delivery.delivery_cursor = 0;
    }
    if delivery.delivered_offset < delivery.base_offset {
        delivery.delivered_offset = delivery.base_offset;
    }
}

fn emit_output_batch(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
    channel_id: u64,
    max_bytes: usize,
) -> TerminalOutputDelivery {
    let Some(batch) = plan_output_batch(delivery, channel_id, max_bytes) else {
        return TerminalOutputDelivery::Delivered;
    };

    let end_offset = batch.end_offset();
    let payload = match batch {
        OutputBatch::Text {
            data, start_offset, ..
        } => TerminalSessionChannelPayload::Text {
            connection_id: delivery.connection_id.clone(),
            session_id: session_id.to_string(),
            channel_id,
            data,
            start_offset,
            end_offset,
        },
        OutputBatch::RawBytes {
            data_base64,
            encoding,
            start_offset,
            ..
        } => TerminalSessionChannelPayload::Bytes {
            connection_id: delivery.connection_id.clone(),
            session_id: session_id.to_string(),
            channel_id,
            data_base64,
            encoding,
            start_offset,
            end_offset,
        },
    };
    let emitted = send_terminal_data_payload(app, session_id, payload);
    if emitted {
        delivery.delivered_offset = end_offset;
        TerminalOutputDelivery::Delivered
    } else {
        TerminalOutputDelivery::NoSubscriber
    }
}

/// One planned output batch. `RawBytes` always carries the exact bytes the
/// backend received; re-encoded text is never passed off as raw bytes because
/// the frontend pipes `Bytes` payloads straight into binary consumers (trzsz).
enum OutputBatch {
    Text {
        data: String,
        start_offset: usize,
        end_offset: usize,
    },
    RawBytes {
        data_base64: String,
        encoding: String,
        start_offset: usize,
        end_offset: usize,
    },
}

impl OutputBatch {
    fn end_offset(&self) -> usize {
        match self {
            Self::Text { end_offset, .. } | Self::RawBytes { end_offset, .. } => *end_offset,
        }
    }
}

fn plan_output_batch(
    delivery: &mut SessionDeliveryState,
    channel_id: u64,
    max_bytes: usize,
) -> Option<OutputBatch> {
    if delivery.delivered_offset >= delivery.next_offset {
        return None;
    }

    delivery.sync_delivery_cursor();
    // Offsets are byte-based, but a stale cursor can point into a multi-byte
    // character (for example after replay-cache pruning). Drop the partial
    // character and realign the cursor before taking any slices.
    if let Some(chunk) = delivery.cache.get(delivery.delivery_cursor) {
        let local = delivery.delivered_offset.saturating_sub(chunk.start_offset);
        let aligned = next_char_boundary(&chunk.data, local);
        if aligned > local {
            delivery.delivered_offset = chunk.start_offset + aligned;
        }
    }
    let start_offset = delivery.delivered_offset;
    if delivery.raw_output_channel_id == Some(channel_id) {
        plan_raw_output_batch(delivery, start_offset, max_bytes)
    } else {
        plan_text_output_batch(delivery, start_offset, max_bytes)
    }
}

fn plan_text_output_batch(
    delivery: &SessionDeliveryState,
    start_offset: usize,
    max_bytes: usize,
) -> Option<OutputBatch> {
    let mut payload = String::new();
    let mut end_offset = start_offset;

    for chunk in delivery.cache.iter().skip(delivery.delivery_cursor) {
        if chunk.end_offset <= end_offset {
            continue;
        }
        let local_start =
            next_char_boundary(&chunk.data, end_offset.saturating_sub(chunk.start_offset));
        if local_start >= chunk.data.len() {
            continue;
        }
        let remaining = max_bytes.saturating_sub(payload.len());
        if remaining == 0 {
            break;
        }
        let local_end = previous_char_boundary(
            &chunk.data,
            std::cmp::min(chunk.data.len(), local_start + remaining),
        );
        if local_end <= local_start {
            break;
        }
        payload.push_str(&chunk.data[local_start..local_end]);
        end_offset += local_end - local_start;
        if payload.len() >= max_bytes {
            break;
        }
    }

    if payload.is_empty() {
        return None;
    }
    Some(OutputBatch::Text {
        data: payload,
        start_offset,
        end_offset,
    })
}

/// Raw-mode batching. Batch boundaries always align to chunk boundaries so the
/// payload is a concatenation of the exact raw byte slices the backend
/// received; a single chunk larger than `max_bytes` is emitted whole (the cap
/// is a soft batching hint, not worth corrupting a binary stream for).
///
/// A chunk that cannot provide exact raw bytes (partially delivered before raw
/// mode engaged, or raw bytes dropped by replay-cache pruning) is delivered as
/// a `Text` batch instead, and the text run stops before the next raw-capable
/// chunk so transfer bytes never leak into a decoded-text payload.
fn plan_raw_output_batch(
    delivery: &SessionDeliveryState,
    start_offset: usize,
    max_bytes: usize,
) -> Option<OutputBatch> {
    let mut raw_len = 0_usize;
    let mut raw_encoder: Option<StreamingBase64Encoder> = None;
    let mut text: Option<String> = None;
    let mut end_offset = start_offset;
    let mut encoding = "utf-8".to_string();

    for chunk in delivery.cache.iter().skip(delivery.delivery_cursor) {
        if chunk.end_offset <= end_offset {
            continue;
        }
        // `start_offset` is char-boundary aligned and chunk ranges are
        // contiguous, so only the first chunk can be partially consumed.
        let local_start = end_offset.saturating_sub(chunk.start_offset);
        if local_start >= chunk.data.len() {
            continue;
        }
        let exact_raw_available = local_start == 0 && !chunk.raw_bytes.is_empty();
        if exact_raw_available {
            if text.is_some() {
                break;
            }
            if raw_len > 0 && raw_len.saturating_add(chunk.raw_bytes.len()) > max_bytes {
                break;
            }
            raw_encoder
                .get_or_insert_with(|| {
                    StreamingBase64Encoder::new(max_bytes.saturating_add(2) / 3 * 4)
                })
                .push(&chunk.raw_bytes);
            raw_len += chunk.raw_bytes.len();
            end_offset = chunk.end_offset;
            encoding = chunk.encoding.clone();
        } else {
            if raw_len > 0 {
                break;
            }
            let buffer = text.get_or_insert_with(String::new);
            let remaining = max_bytes.saturating_sub(buffer.len());
            if remaining == 0 {
                break;
            }
            let local_end = previous_char_boundary(
                &chunk.data,
                std::cmp::min(chunk.data.len(), local_start + remaining),
            );
            if local_end <= local_start {
                break;
            }
            buffer.push_str(&chunk.data[local_start..local_end]);
            end_offset += local_end - local_start;
        }
    }

    if raw_len > 0 {
        let raw_encoder = raw_encoder?;
        return Some(OutputBatch::RawBytes {
            data_base64: raw_encoder.finish(),
            encoding,
            start_offset,
            end_offset,
        });
    }
    text.filter(|text| !text.is_empty())
        .map(|data| OutputBatch::Text {
            data,
            start_offset,
            end_offset,
        })
}

/// Encodes adjacent raw chunks as one Base64 stream without first coalescing
/// their bytes into a second full-size buffer. The two-byte carry preserves
/// Base64's three-byte grouping across arbitrary cache chunk boundaries.
struct StreamingBase64Encoder {
    output: String,
    carry: [u8; 3],
    carry_len: usize,
}

impl StreamingBase64Encoder {
    fn new(capacity: usize) -> Self {
        Self {
            output: String::with_capacity(capacity),
            carry: [0; 3],
            carry_len: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let mut offset = 0;
        if self.carry_len > 0 {
            let needed = 3 - self.carry_len;
            let copied = needed.min(bytes.len());
            self.carry[self.carry_len..self.carry_len + copied].copy_from_slice(&bytes[..copied]);
            self.carry_len += copied;
            offset += copied;
            if self.carry_len == 3 {
                STANDARD_NO_PAD.encode_string(self.carry, &mut self.output);
                self.carry_len = 0;
            } else {
                return;
            }
        }

        let complete_len = (bytes.len() - offset) / 3 * 3;
        if complete_len > 0 {
            STANDARD_NO_PAD.encode_string(&bytes[offset..offset + complete_len], &mut self.output);
            offset += complete_len;
        }
        let remainder = &bytes[offset..];
        self.carry[..remainder.len()].copy_from_slice(remainder);
        self.carry_len = remainder.len();
    }

    fn finish(mut self) -> String {
        if self.carry_len > 0 {
            STANDARD_NO_PAD.encode_string(&self.carry[..self.carry_len], &mut self.output);
        }
        self.output
    }
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn send_terminal_data_payload(
    app: &AppHandle,
    session_id: &str,
    payload: TerminalSessionChannelPayload,
) -> bool {
    let delivered = app
        .state::<AppState>()
        .send_terminal_output(session_id, payload);
    if delivered == 0 {
        log::trace!(target: "terminal.delivery", "terminal output for session '{session_id}' had no channel subscribers");
    }
    delivered > 0
}

fn emit_working_directory(app: &AppHandle, session_id: &str, directory: RemoteWorkingDirectory) {
    let _ = app.emit(
        "terminal-event",
        TerminalEventEnvelope::new(
            "session.working_directory",
            TerminalWorkingDirectoryEvent {
                session_id: session_id.to_string(),
                path: directory.path,
            },
        ),
    );
}

enum TerminalOutputDelivery {
    Delivered,
    NoSubscriber,
}

#[cfg(test)]
mod tests {
    use super::{
        next_char_boundary, plan_output_batch, previous_char_boundary, should_flush_live_output,
        OutputBatch, SessionDeliveryState, REPLAY_CACHE_MAX_BYTES,
    };
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;

    fn delivery() -> SessionDeliveryState {
        SessionDeliveryState::new("connection".to_string(), false)
    }

    fn raw_delivery() -> SessionDeliveryState {
        let mut delivery = SessionDeliveryState::new("connection".to_string(), true);
        delivery.active_channel_id = Some(7);
        delivery.output_ready_channel_id = Some(7);
        delivery.raw_output_channel_id = Some(7);
        delivery
    }

    #[test]
    fn trickle_output_flushes_immediately_without_coalescing() {
        let mut delivery = delivery();
        delivery.active_channel_id = Some(7);
        delivery.output_ready_channel_id = Some(7);
        // 无待交付字节时不冲刷。
        assert!(!should_flush_live_output(&delivery));

        delivery.push_cached_output("echo".to_string(), "utf-8".to_string(), None);
        assert!(should_flush_live_output(&delivery));

        // 无订阅通道或回放进行中时不冲刷。
        delivery.output_ready_channel_id = None;
        assert!(!should_flush_live_output(&delivery));
        delivery.output_ready_channel_id = Some(7);
        delivery.replay_channel_id = Some(7);
        assert!(!should_flush_live_output(&delivery));
    }

    #[test]
    fn replay_cache_byte_cap_trims_unbroken_output() {
        let mut delivery = delivery();
        let blob = "x".repeat(16 * 1024 * 1024);
        delivery.push_cached_output(blob, "utf-8".to_string(), None);

        assert!(delivery.cache_bytes <= REPLAY_CACHE_MAX_BYTES);
        assert_eq!(
            delivery.base_offset + delivery.cache_bytes,
            delivery.next_offset
        );
        assert_eq!(delivery.delivered_offset, delivery.base_offset);
    }

    #[test]
    fn replay_cache_byte_cap_keeps_the_recent_tail() {
        let mut delivery = delivery();
        let blob = "y".repeat(REPLAY_CACHE_MAX_BYTES + 1024);
        delivery.push_cached_output(blob, "utf-8".to_string(), None);

        assert_eq!(delivery.cache_bytes, REPLAY_CACHE_MAX_BYTES);
        let retained: usize = delivery.cache.iter().map(|chunk| chunk.data.len()).sum();
        assert_eq!(retained, REPLAY_CACHE_MAX_BYTES);
        assert!(delivery
            .cache
            .iter()
            .all(|chunk| chunk.data.bytes().all(|byte| byte == b'y')));
    }

    #[test]
    fn delivery_cursor_skips_fully_delivered_chunks() {
        let mut delivery = delivery();
        // 交替编码阻止尾块合并，保留三个独立块来验证游标语义。
        delivery.push_cached_output("aaa".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output("bbb".to_string(), "gbk".to_string(), None);
        delivery.push_cached_output("ccc".to_string(), "utf-8".to_string(), None);

        delivery.delivered_offset = 6;
        delivery.sync_delivery_cursor();
        assert_eq!(delivery.delivery_cursor, 2);

        delivery.delivered_offset = 7;
        delivery.sync_delivery_cursor();
        assert_eq!(delivery.delivery_cursor, 2);

        delivery.delivered_offset = 9;
        delivery.sync_delivery_cursor();
        assert_eq!(delivery.delivery_cursor, 3);
    }

    #[test]
    fn output_offsets_are_normalized_to_utf8_boundaries() {
        let text = "流量审计";
        assert_eq!(previous_char_boundary(text, 2), 0);
        assert_eq!(next_char_boundary(text, 1), 3);
        assert_eq!(next_char_boundary(text, 3), 3);
    }

    #[test]
    fn raw_batches_align_to_chunk_boundaries_with_exact_bytes() {
        // GBK-like chunks: the decoded text (6 UTF-8 bytes) is longer than the
        // raw byte stream (4 bytes), so a text-byte batch cap must not split
        // the raw payload.
        let mut delivery = raw_delivery();
        delivery.push_cached_output(
            "流量".to_string(),
            "gbk".to_string(),
            Some(vec![0xC1, 0xF7, 0xC1, 0xBF]),
        );
        delivery.push_cached_output(
            "审计".to_string(),
            "gbk".to_string(),
            Some(vec![0xC9, 0xF3, 0xBC, 0xC6]),
        );

        let batch = plan_output_batch(&mut delivery, 7, 4).expect("first batch");
        let end = batch.end_offset();
        match batch {
            OutputBatch::RawBytes {
                data_base64,
                encoding,
                start_offset,
                end_offset,
            } => {
                assert_eq!(
                    STANDARD_NO_PAD.decode(data_base64).expect("valid Base64"),
                    vec![0xC1, 0xF7, 0xC1, 0xBF]
                );
                assert_eq!(encoding, "gbk");
                assert_eq!((start_offset, end_offset), (0, 6));
            }
            _ => panic!("expected a raw bytes batch"),
        }
        delivery.delivered_offset = end;

        let batch = plan_output_batch(&mut delivery, 7, 4).expect("second batch");
        let end = batch.end_offset();
        match batch {
            OutputBatch::RawBytes {
                data_base64,
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(
                    STANDARD_NO_PAD.decode(data_base64).expect("valid Base64"),
                    vec![0xC9, 0xF3, 0xBC, 0xC6]
                );
                assert_eq!((start_offset, end_offset), (6, 12));
            }
            _ => panic!("expected a raw bytes batch"),
        }
        delivery.delivered_offset = end;

        assert!(plan_output_batch(&mut delivery, 7, 4).is_none());
    }

    #[test]
    fn raw_batch_emits_oversized_chunk_whole_instead_of_splitting() {
        let mut delivery = raw_delivery();
        let mut raw = Vec::new();
        for _ in 0..100 {
            raw.extend_from_slice(&[0xC1, 0xF7]);
        }
        delivery.push_cached_output("流".repeat(100), "gbk".to_string(), Some(raw.clone()));

        let batch = plan_output_batch(&mut delivery, 7, 128).expect("batch");
        match batch {
            OutputBatch::RawBytes {
                data_base64,
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(STANDARD_NO_PAD.decode(data_base64).unwrap(), raw);
                assert_eq!((start_offset, end_offset), (0, 300));
            }
            _ => panic!("expected a raw bytes batch"),
        }
    }

    #[test]
    fn raw_mode_never_sends_recoded_text_as_bytes() {
        let mut delivery = raw_delivery();
        // Chunk without raw bytes (cached before raw mode engaged, or raw
        // dropped by replay-cache pruning) followed by a raw-capable chunk.
        delivery.push_cached_output("hello ".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output(
            "流量".to_string(),
            "gbk".to_string(),
            Some(vec![0xC1, 0xF7, 0xC1, 0xBF]),
        );

        let batch = plan_output_batch(&mut delivery, 7, 1024).expect("text batch");
        let end = batch.end_offset();
        match batch {
            OutputBatch::Text {
                data,
                start_offset,
                end_offset,
                ..
            } => {
                // The text run must stop before the raw-capable chunk.
                assert_eq!(data, "hello ");
                assert_eq!((start_offset, end_offset), (0, 6));
            }
            OutputBatch::RawBytes { .. } => panic!("recoded text must never pose as raw bytes"),
        }
        delivery.delivered_offset = end;

        let batch = plan_output_batch(&mut delivery, 7, 1024).expect("raw batch");
        match batch {
            OutputBatch::RawBytes {
                data_base64,
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(
                    STANDARD_NO_PAD.decode(data_base64).expect("valid Base64"),
                    vec![0xC1, 0xF7, 0xC1, 0xBF]
                );
                assert_eq!((start_offset, end_offset), (6, 12));
            }
            _ => panic!("expected a raw bytes batch"),
        }
    }

    #[test]
    fn raw_mode_delivers_partially_consumed_chunk_remainder_as_text() {
        let mut delivery = raw_delivery();
        delivery.push_cached_output(
            "流量审计".to_string(),
            "gbk".to_string(),
            Some(vec![0xC1, 0xF7, 0xC1, 0xBF, 0xC9, 0xF3, 0xBC, 0xC6]),
        );
        // The first half was already delivered before raw mode engaged, so the
        // full-chunk raw bytes would duplicate output; the remainder goes out
        // as text instead.
        delivery.delivered_offset = 6;

        let batch = plan_output_batch(&mut delivery, 7, 1024).expect("batch");
        match batch {
            OutputBatch::Text {
                data,
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(data, "审计");
                assert_eq!((start_offset, end_offset), (6, 12));
            }
            OutputBatch::RawBytes { .. } => panic!("partial chunk must not be resent as raw bytes"),
        }
    }

    #[test]
    fn small_text_chunks_merge_into_the_tail_chunk() {
        let mut delivery = delivery();
        for index in 0..100 {
            delivery.push_cached_output(format!("chunk{index}\n"), "utf-8".to_string(), None);
        }

        assert_eq!(delivery.cache.len(), 1);
        let chunk = &delivery.cache[0];
        assert_eq!(chunk.start_offset, 0);
        assert_eq!(chunk.end_offset, delivery.next_offset);
        assert_eq!(delivery.cache_bytes, chunk.cached_bytes());
        assert!(chunk.data.starts_with("chunk0\n"));
        assert!(chunk.data.ends_with("chunk99\n"));
    }

    #[test]
    fn tail_merge_stops_at_the_chunk_cap() {
        let mut delivery = delivery();
        delivery.push_cached_output(
            "x".repeat(super::CACHE_CHUNK_MERGE_MAX_BYTES),
            "utf-8".to_string(),
            None,
        );
        delivery.push_cached_output("y".to_string(), "utf-8".to_string(), None);

        assert_eq!(delivery.cache.len(), 2);
        assert_eq!(
            delivery.cache[0].data.len(),
            super::CACHE_CHUNK_MERGE_MAX_BYTES
        );
        assert_eq!(delivery.cache[1].data, "y");
        assert_eq!(
            delivery.cache[1].start_offset,
            super::CACHE_CHUNK_MERGE_MAX_BYTES
        );
    }

    #[test]
    fn chunks_with_raw_bytes_or_a_new_encoding_do_not_merge() {
        let mut delivery = raw_delivery();
        delivery.push_cached_output("hello".to_string(), "utf-8".to_string(), None);
        // raw 通道已武装：后续块保留 raw bytes，不得并入纯文本尾块。
        delivery.push_cached_output(
            "流量".to_string(),
            "gbk".to_string(),
            Some(vec![0xC1, 0xF7, 0xC1, 0xBF]),
        );
        delivery.push_cached_output("world".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output("!".to_string(), "ascii".to_string(), None);

        assert_eq!(delivery.cache.len(), 4);
        assert_eq!(delivery.cache[1].raw_bytes.len(), 4);
        assert_eq!(delivery.cache[2].data, "world");
        assert_eq!(delivery.cache[3].data, "!");
        let total: usize = delivery.cache.iter().map(|chunk| chunk.data.len()).sum();
        assert_eq!(
            total,
            "hello".len() + "流量".len() + "world".len() + "!".len()
        );
    }

    #[test]
    fn tail_merge_rewinds_the_delivery_cursor_past_a_fully_delivered_tail() {
        let mut delivery = delivery();
        delivery.push_cached_output("abc".to_string(), "utf-8".to_string(), None);
        // 完整交付一次后游标越过尾块。
        let batch = plan_output_batch(&mut delivery, 7, 1024).expect("first batch");
        delivery.delivered_offset = batch.end_offset();
        delivery.sync_delivery_cursor();
        assert_eq!(delivery.delivery_cursor, 1);

        // 新数据合并进尾块后必须仍然可交付，不能落在游标之后。
        delivery.push_cached_output("def".to_string(), "utf-8".to_string(), None);
        assert_eq!(delivery.cache.len(), 1);
        let batch = plan_output_batch(&mut delivery, 7, 1024).expect("merged bytes deliverable");
        match batch {
            OutputBatch::Text {
                data,
                start_offset,
                end_offset,
            } => {
                assert_eq!(data, "def");
                assert_eq!((start_offset, end_offset), (3, 6));
            }
            OutputBatch::RawBytes { .. } => panic!("expected a text batch"),
        }
    }
}
