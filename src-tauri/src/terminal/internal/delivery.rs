use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

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
                CodecState, TerminalWorkingDirectoryEvent, MAX_UNRENDERED_BYTES,
                OUTPUT_FLUSH_MAX_BYTES, OUTPUT_LIVE_FLUSH_MIN_BYTES, OUTPUT_LIVE_FLUSH_WINDOW_MS,
                RENDER_GATE_FAIL_OPEN_MS, RENDER_GATE_RETRY_MS, REPLAY_CACHE_MAX_BYTES,
            },
            osc::extract_working_directories,
            util::RemoteWorkingDirectory,
        },
    },
};

const REPLAY_BATCH_MAX_BYTES: usize = 64 * 1024;
const TRZSZ_TRIGGER_BYTES: &[u8] = b"::TRZSZ:TRANSFER:";

#[derive(Clone)]
struct CachedOutputChunk {
    start_offset: usize,
    end_offset: usize,
    line_break_count: usize,
    encoding: String,
    data: Arc<str>,
    raw_bytes: Arc<[u8]>,
}

impl CachedOutputChunk {
    fn cached_bytes(&self) -> usize {
        self.data.len() + self.raw_bytes.len()
    }
}

/// Live-output backpressure gate. The gate arms on attach and is permanently
/// disabled (fail-open) when no `renderedOffset` report arrives within the
/// grace window, so legacy or broken frontends cannot deadlock a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderGate {
    Inactive,
    Pending(Instant),
    Active,
    Disabled,
}

pub(super) struct SessionDeliveryState {
    pub(super) connection_id: String,
    pub(super) raw_bytes_supported: bool,
    pub(super) active_channel_id: Option<u64>,
    pub(super) output_ready_channel_id: Option<u64>,
    pub(super) replay_channel_id: Option<u64>,
    pub(super) raw_output_channel_id: Option<u64>,
    pub(super) last_input_sequence: Option<u64>,
    base_offset: usize,
    pub(super) delivered_offset: usize,
    pub(super) next_offset: usize,
    retained_line_breaks: usize,
    replay_line_limit: usize,
    live_flush_deadline: Option<Instant>,
    cache: VecDeque<CachedOutputChunk>,
    cache_bytes: usize,
    /// Index of the first cache chunk that may still hold undelivered bytes;
    /// every chunk before it is fully delivered and skipped by emit scans.
    delivery_cursor: usize,
    rendered_offset: usize,
    render_gate: RenderGate,
}

impl SessionDeliveryState {
    pub(super) fn new(
        connection_id: String,
        replay_line_limit: usize,
        raw_bytes_supported: bool,
    ) -> Self {
        Self {
            connection_id,
            raw_bytes_supported,
            active_channel_id: None,
            output_ready_channel_id: None,
            replay_channel_id: None,
            raw_output_channel_id: None,
            last_input_sequence: None,
            base_offset: 0,
            delivered_offset: 0,
            next_offset: 0,
            retained_line_breaks: 0,
            replay_line_limit: replay_line_limit.max(1),
            live_flush_deadline: None,
            cache: VecDeque::new(),
            cache_bytes: 0,
            delivery_cursor: 0,
            rendered_offset: 0,
            render_gate: RenderGate::Inactive,
        }
    }

    pub(super) fn note_channel_activated(&mut self) {
        if self.render_gate != RenderGate::Disabled {
            self.render_gate = RenderGate::Pending(Instant::now());
        }
        self.rendered_offset = self.delivered_offset;
    }

    pub(super) fn record_rendered_offset(&mut self, offset: u64) {
        if self.render_gate == RenderGate::Disabled {
            return;
        }
        self.render_gate = RenderGate::Active;
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        // Reports are monotonic; a stale or rewound frontend must not reopen
        // the gate by dragging the rendered offset backwards.
        if offset > self.rendered_offset {
            self.rendered_offset = offset;
        }
    }

    /// Returns true when live output must wait for the frontend renderer.
    fn render_gate_blocked(&mut self, now: Instant) -> bool {
        if matches!(self.render_gate, RenderGate::Pending(since)
            if now.duration_since(since) >= Duration::from_millis(RENDER_GATE_FAIL_OPEN_MS))
        {
            self.render_gate = RenderGate::Disabled;
            self.rendered_offset = self.next_offset;
        }
        match self.render_gate {
            RenderGate::Inactive | RenderGate::Disabled => false,
            RenderGate::Pending(_) | RenderGate::Active => {
                self.next_offset.saturating_sub(self.rendered_offset) > MAX_UNRENDERED_BYTES
            }
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
        let start_offset = self.next_offset;
        let end_offset = start_offset + data.len();
        self.next_offset = end_offset;
        let line_break_count = count_line_breaks(&data);
        self.retained_line_breaks += line_break_count;
        let raw_bytes = raw_bytes.unwrap_or_default();
        let preserve_raw_bytes = self.raw_bytes_supported
            && (self.raw_output_channel_id.is_some()
                || raw_bytes_contain_trzsz_trigger(&raw_bytes));
        if self.raw_output_channel_id.is_none() && preserve_raw_bytes {
            self.raw_output_channel_id = self.active_channel_id;
        }
        let raw_bytes: Arc<[u8]> = if preserve_raw_bytes {
            Arc::from(raw_bytes)
        } else {
            Arc::from([])
        };
        self.cache_bytes += data.len() + raw_bytes.len();
        self.cache.push_back(CachedOutputChunk {
            start_offset,
            end_offset,
            line_break_count,
            encoding,
            raw_bytes,
            data: Arc::<str>::from(data),
        });
        prune_terminal_replay_cache(self);
        prune_terminal_replay_cache_bytes(self);
    }
}

pub(super) fn flush_terminal_output(
    app: &AppHandle,
    session_id: &str,
    codec: &mut CodecState,
    delivery: &mut SessionDeliveryState,
) -> bool {
    let tail = flush_decoded_backend_bytes_with_raw(codec, delivery.raw_bytes_supported);
    if !tail.data.is_empty()
        && !emit_terminal_data(
            app,
            session_id,
            delivery,
            tail.data,
            tail.encoding,
            Some(tail.raw_bytes),
        )
    {
        return false;
    }
    drain_live_output(app, session_id, delivery)
}

pub(super) fn emit_terminal_data(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
    data: String,
    encoding: String,
    raw_bytes: Option<Vec<u8>>,
) -> bool {
    if data.is_empty() {
        return true;
    }

    cache_terminal_data(app, session_id, delivery, data, encoding, raw_bytes);

    if delivery.output_ready_channel_id.is_some() && delivery.replay_channel_id.is_none() {
        schedule_live_output_flush(delivery);
    }
    true
}

pub(super) fn should_flush_live_output(delivery: &SessionDeliveryState) -> bool {
    if delivery.output_ready_channel_id.is_none() || delivery.replay_channel_id.is_some() {
        return false;
    }
    let pending_bytes = pending_live_output_bytes(delivery);
    pending_bytes >= OUTPUT_LIVE_FLUSH_MIN_BYTES
        || (pending_bytes > 0 && delivery.raw_output_channel_id.is_some())
        || delivery
            .live_flush_deadline
            .is_some_and(|deadline| pending_bytes > 0 && Instant::now() >= deadline)
}

pub(super) fn live_flush_delay(
    delivery: &SessionDeliveryState,
    replay_pending: bool,
) -> Option<Duration> {
    if replay_pending
        || delivery.output_ready_channel_id.is_none()
        || pending_live_output_bytes(delivery) == 0
    {
        return None;
    }
    let deadline = delivery.live_flush_deadline?;
    Some(deadline.saturating_duration_since(Instant::now()))
}

pub(super) fn drain_terminal_replay(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
) -> bool {
    let Some(channel_id) = delivery.replay_channel_id else {
        return true;
    };
    if delivery.output_ready_channel_id != Some(channel_id) {
        return true;
    }
    if delivery.delivered_offset >= delivery.next_offset {
        delivery.replay_channel_id = None;
        return true;
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
        return true;
    }
    if delivery.delivered_offset >= delivery.next_offset {
        delivery.replay_channel_id = None;
    }
    true
}

pub(super) fn drain_live_output(
    app: &AppHandle,
    session_id: &str,
    delivery: &mut SessionDeliveryState,
) -> bool {
    let Some(channel_id) = delivery.output_ready_channel_id else {
        delivery.live_flush_deadline = None;
        return true;
    };
    delivery.live_flush_deadline = None;
    if delivery.render_gate_blocked(Instant::now()) {
        // The frontend renderer is more than MAX_UNRENDERED_BYTES behind;
        // hold the output and retry shortly instead of growing an unbounded
        // renderer backlog.
        delivery.live_flush_deadline =
            Some(Instant::now() + Duration::from_millis(RENDER_GATE_RETRY_MS));
        return true;
    }
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
            return true;
        }
    }
    true
}

fn pending_live_output_bytes(delivery: &SessionDeliveryState) -> usize {
    delivery
        .next_offset
        .saturating_sub(delivery.delivered_offset)
}

fn schedule_live_output_flush(delivery: &mut SessionDeliveryState) {
    if delivery.output_ready_channel_id.is_none() || delivery.replay_channel_id.is_some() {
        return;
    }
    if delivery.raw_output_channel_id.is_some()
        || pending_live_output_bytes(delivery) >= OUTPUT_LIVE_FLUSH_MIN_BYTES
    {
        return;
    }
    if delivery.live_flush_deadline.is_none() {
        delivery.live_flush_deadline =
            Some(Instant::now() + Duration::from_millis(OUTPUT_LIVE_FLUSH_WINDOW_MS));
    }
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

fn count_line_breaks(text: &str) -> usize {
    let mut count = 0;
    let mut index = 0;
    let bytes = text.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                count += 1;
                index += usize::from(matches!(bytes.get(index + 1), Some(b'\n'))) + 1;
            }
            b'\n' => {
                count += 1;
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }
    count
}

fn byte_index_after_nth_line_break(text: &str, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mut seen = 0_usize;
    let mut index = 0_usize;
    let bytes = text.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                seen += 1;
                index += usize::from(matches!(bytes.get(index + 1), Some(b'\n'))) + 1;
                if seen == count {
                    return index;
                }
            }
            b'\n' => {
                seen += 1;
                index += 1;
                if seen == count {
                    return index;
                }
            }
            _ => {
                index += 1;
            }
        }
    }
    text.len()
}

fn prune_terminal_replay_cache(delivery: &mut SessionDeliveryState) {
    let allowed_breaks = delivery.replay_line_limit.saturating_sub(1);
    while delivery.retained_line_breaks > allowed_breaks {
        let Some(mut chunk) = delivery.cache.pop_front() else {
            delivery.base_offset = delivery.next_offset;
            delivery.delivered_offset = delivery.base_offset;
            delivery.retained_line_breaks = 0;
            delivery.delivery_cursor = 0;
            return;
        };
        delivery.delivery_cursor = delivery.delivery_cursor.saturating_sub(1);

        if delivery
            .retained_line_breaks
            .saturating_sub(chunk.line_break_count)
            >= allowed_breaks
        {
            delivery.retained_line_breaks = delivery
                .retained_line_breaks
                .saturating_sub(chunk.line_break_count);
            delivery.cache_bytes = delivery.cache_bytes.saturating_sub(chunk.cached_bytes());
            delivery.base_offset = chunk.end_offset;
            continue;
        }

        let excess_breaks = delivery.retained_line_breaks.saturating_sub(allowed_breaks);
        let trim_index = byte_index_after_nth_line_break(&chunk.data, excess_breaks);
        let remaining = &chunk.data[trim_index..];
        delivery.retained_line_breaks = delivery.retained_line_breaks.saturating_sub(excess_breaks);
        if remaining.is_empty() {
            delivery.cache_bytes = delivery.cache_bytes.saturating_sub(chunk.cached_bytes());
            delivery.base_offset = chunk.end_offset;
            continue;
        }

        let removed_raw_bytes = if chunk.raw_bytes.is_empty() {
            0
        } else if chunk.raw_bytes.len() == chunk.data.len() {
            trim_index
        } else {
            chunk.raw_bytes.len()
        };
        chunk.start_offset += trim_index;
        if !chunk.raw_bytes.is_empty() {
            chunk.raw_bytes = if chunk.raw_bytes.len() == chunk.data.len() {
                Arc::<[u8]>::from(&chunk.raw_bytes[trim_index..])
            } else {
                Arc::<[u8]>::from([])
            };
        }
        chunk.data = Arc::<str>::from(remaining);
        chunk.line_break_count = chunk.line_break_count.saturating_sub(excess_breaks);
        delivery.cache_bytes = delivery
            .cache_bytes
            .saturating_sub(trim_index + removed_raw_bytes);
        delivery.base_offset = chunk.start_offset;
        delivery.cache.push_front(chunk);
        break;
    }

    if delivery.cache.is_empty() {
        delivery.base_offset = delivery.next_offset;
        delivery.delivery_cursor = 0;
    }
    if delivery.delivered_offset < delivery.base_offset {
        delivery.delivered_offset = delivery.base_offset;
    }
}

/// Byte-based hard cap for the replay cache. Runs after the line-based prune;
/// whichever limit retains less output wins. Trimming keeps the newest tail so
/// reattached frontends still see the most recent screenfuls.
fn prune_terminal_replay_cache_bytes(delivery: &mut SessionDeliveryState) {
    while delivery.cache_bytes > REPLAY_CACHE_MAX_BYTES {
        let Some(mut chunk) = delivery.cache.pop_front() else {
            delivery.cache_bytes = 0;
            delivery.delivery_cursor = 0;
            delivery.base_offset = delivery.next_offset;
            delivery.delivered_offset = delivery.base_offset;
            delivery.retained_line_breaks = 0;
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
            delivery.retained_line_breaks = delivery
                .retained_line_breaks
                .saturating_sub(chunk.line_break_count);
            delivery.base_offset = chunk.end_offset;
            continue;
        }

        let removed_breaks = count_line_breaks(&chunk.data[..trim_index]);
        let removed_raw_bytes = if chunk.raw_bytes.is_empty() {
            0
        } else if chunk.raw_bytes.len() == chunk.data.len() {
            trim_index
        } else {
            chunk.raw_bytes.len()
        };
        if !chunk.raw_bytes.is_empty() {
            chunk.raw_bytes = if chunk.raw_bytes.len() == chunk.data.len() {
                Arc::<[u8]>::from(&chunk.raw_bytes[trim_index..])
            } else {
                Arc::<[u8]>::from([])
            };
        }
        chunk.start_offset += trim_index;
        chunk.data = Arc::<str>::from(&chunk.data[trim_index..]);
        chunk.line_break_count = chunk.line_break_count.saturating_sub(removed_breaks);
        delivery.cache_bytes = delivery
            .cache_bytes
            .saturating_sub(trim_index + removed_raw_bytes);
        delivery.retained_line_breaks =
            delivery.retained_line_breaks.saturating_sub(removed_breaks);
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
    let start_offset = delivery.delivered_offset;
    if start_offset >= delivery.next_offset {
        return TerminalOutputDelivery::Delivered;
    }

    delivery.sync_delivery_cursor();
    let mut payload = String::new();
    let mut end_offset = start_offset;
    let mut encoding = "utf-8".to_string();
    let mut raw_payload = Vec::new();
    let raw_output_enabled = delivery.raw_output_channel_id == Some(channel_id);
    let mut exact_raw_payload = raw_output_enabled;

    for chunk in delivery.cache.iter().skip(delivery.delivery_cursor) {
        if chunk.end_offset <= end_offset {
            continue;
        }
        let local_start = end_offset.saturating_sub(chunk.start_offset);
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
        if raw_output_enabled {
            if local_start == 0 && local_end == chunk.data.len() {
                if chunk.raw_bytes.is_empty() {
                    exact_raw_payload = false;
                } else {
                    raw_payload.extend_from_slice(&chunk.raw_bytes);
                }
            } else {
                exact_raw_payload = false;
            }
        }
        end_offset += local_end - local_start;
        encoding = chunk.encoding.clone();
        if payload.len() >= max_bytes {
            break;
        }
    }

    if payload.is_empty() {
        return TerminalOutputDelivery::Delivered;
    }

    let emitted = send_terminal_data_payload(
        app,
        session_id,
        if raw_output_enabled && exact_raw_payload && !raw_payload.is_empty() {
            TerminalSessionChannelPayload::Bytes {
                connection_id: delivery.connection_id.clone(),
                session_id: session_id.to_string(),
                channel_id,
                data_base64: STANDARD_NO_PAD.encode(&raw_payload),
                encoding,
                start_offset,
                end_offset,
            }
        } else if raw_output_enabled {
            TerminalSessionChannelPayload::Bytes {
                connection_id: delivery.connection_id.clone(),
                session_id: session_id.to_string(),
                channel_id,
                data_base64: STANDARD_NO_PAD.encode(payload.as_bytes()),
                encoding: "utf-8".to_string(),
                start_offset,
                end_offset,
            }
        } else {
            TerminalSessionChannelPayload::Text {
                connection_id: delivery.connection_id.clone(),
                session_id: session_id.to_string(),
                channel_id,
                data: payload,
                encoding,
                start_offset,
                end_offset,
            }
        },
    );
    if emitted {
        delivery.delivered_offset = end_offset;
        TerminalOutputDelivery::Delivered
    } else {
        TerminalOutputDelivery::NoSubscriber
    }
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
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
        RenderGate, SessionDeliveryState, MAX_UNRENDERED_BYTES, RENDER_GATE_FAIL_OPEN_MS,
        REPLAY_CACHE_MAX_BYTES,
    };
    use std::time::{Duration, Instant};

    fn delivery() -> SessionDeliveryState {
        SessionDeliveryState::new("connection".to_string(), 9001, false)
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
    fn replay_cache_line_prune_still_applies_below_the_byte_cap() {
        // The line-based limit is `allowed_breaks = replay_line_limit - 1`,
        // so a limit of 3 retains the last 2 newline-terminated lines.
        let mut delivery = SessionDeliveryState::new("connection".to_string(), 3, false);
        delivery.push_cached_output("one\n".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output("two\n".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output("three\n".to_string(), "utf-8".to_string(), None);

        let retained: String = delivery
            .cache
            .iter()
            .map(|chunk| chunk.data.as_ref())
            .collect();
        assert_eq!(retained, "two\nthree\n");
        assert_eq!(delivery.cache_bytes, "two\nthree\n".len());
    }

    #[test]
    fn delivery_cursor_skips_fully_delivered_chunks() {
        let mut delivery = delivery();
        delivery.push_cached_output("aaa".to_string(), "utf-8".to_string(), None);
        delivery.push_cached_output("bbb".to_string(), "utf-8".to_string(), None);
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
    fn rendered_offset_is_monotonic_and_rewind_is_ignored() {
        let mut delivery = delivery();
        delivery.note_channel_activated();

        delivery.record_rendered_offset(10);
        assert_eq!(delivery.rendered_offset, 10);
        delivery.record_rendered_offset(7);
        assert_eq!(delivery.rendered_offset, 10);
        delivery.record_rendered_offset(42);
        assert_eq!(delivery.rendered_offset, 42);
    }

    #[test]
    fn render_gate_blocks_only_beyond_max_unrendered_bytes() {
        let mut delivery = delivery();
        delivery.note_channel_activated();

        delivery.next_offset = MAX_UNRENDERED_BYTES;
        assert!(!delivery.render_gate_blocked(Instant::now()));
        delivery.next_offset = MAX_UNRENDERED_BYTES + 1;
        assert!(delivery.render_gate_blocked(Instant::now()));

        delivery.record_rendered_offset(2);
        assert!(!delivery.render_gate_blocked(Instant::now()));
    }

    #[test]
    fn render_gate_fails_open_without_reports() {
        let mut delivery = delivery();
        delivery.note_channel_activated();
        delivery.next_offset = MAX_UNRENDERED_BYTES + 1;

        let now = Instant::now();
        assert!(delivery.render_gate_blocked(now));
        let later = now + Duration::from_millis(RENDER_GATE_FAIL_OPEN_MS + 1);
        assert!(!delivery.render_gate_blocked(later));
        assert_eq!(delivery.render_gate, RenderGate::Disabled);
        assert_eq!(delivery.rendered_offset, delivery.next_offset);

        // Fail-open is permanent: a late report must not re-arm the gate.
        delivery.record_rendered_offset(u64::MAX);
        assert_eq!(delivery.render_gate, RenderGate::Disabled);
    }
}
