use std::{
    collections::{BTreeSet, HashMap},
    sync::OnceLock,
    time::Duration,
};

use font_kit::source::SystemSource;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify, RwLock};

const FONT_ENUM_TIMEOUT: Duration = Duration::from_secs(5);
const FONT_STREAM_DEFAULT_CHUNK_SIZE: usize = 10;
const FONT_STREAM_MAX_CHUNK_SIZE: usize = 256;

static FONT_CACHE: OnceLock<RwLock<FontCache>> = OnceLock::new();
static FONT_CACHE_NOTIFY: OnceLock<Notify> = OnceLock::new();
static FONT_STREAMS: OnceLock<Mutex<HashMap<String, FontStreamSession>>> = OnceLock::new();

#[derive(Default)]
struct FontCache {
    fonts: Option<Vec<String>>,
    loading: bool,
}

struct FontStreamSession {
    fonts: Option<Vec<String>>,
    offset: usize,
    chunk_size: usize,
    emitted: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontStreamStartRequest {
    chunk_size: Option<usize>,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontStreamNextRequest {
    stream_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontStreamCancelRequest {
    stream_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontStreamChunk {
    stream_id: String,
    fonts: Vec<String>,
    done: bool,
    pending: bool,
    error: Option<String>,
}

/// Starts a pull-based font stream.
///
/// When the full font cache is cold, this returns a small fallback first chunk
/// immediately and starts system enumeration in the background. The frontend can
/// request subsequent chunks with `system_fonts_stream_next`, giving UI code
/// natural backpressure and cancellation points.
#[tauri::command]
pub async fn system_fonts_stream_start(request: FontStreamStartRequest) -> FontStreamChunk {
    let chunk_size = normalize_chunk_size(request.chunk_size);
    if request.refresh {
        invalidate_font_cache().await;
    }
    start_font_loading(request.refresh).await;

    let stream_id = crate::ids::new_id();
    if let Some(fonts) = cached_fonts().await {
        return create_cached_stream(stream_id, fonts, chunk_size).await;
    }

    let fonts = first_fallback_chunk(chunk_size);
    let emitted = fonts.iter().cloned().collect();
    streams().lock().await.insert(
        stream_id.clone(),
        FontStreamSession {
            fonts: None,
            offset: 0,
            chunk_size,
            emitted,
        },
    );
    FontStreamChunk {
        stream_id,
        fonts,
        done: false,
        pending: true,
        error: None,
    }
}

#[tauri::command]
pub async fn system_fonts_stream_next(request: FontStreamNextRequest) -> FontStreamChunk {
    let stream_id = request.stream_id;
    if let Some(chunk) = next_cached_stream_chunk(&stream_id).await {
        return chunk;
    }

    let Some(fonts) = await_font_cache().await else {
        cancel_font_stream_by_id(&stream_id).await;
        return FontStreamChunk {
            stream_id,
            fonts: Vec::new(),
            done: true,
            pending: false,
            error: Some("system font enumeration failed".to_string()),
        };
    };

    {
        let mut streams = streams().lock().await;
        let Some(session) = streams.get_mut(&stream_id) else {
            return FontStreamChunk {
                stream_id,
                fonts: Vec::new(),
                done: true,
                pending: false,
                error: None,
            };
        };
        if session.fonts.is_none() {
            session.fonts = Some(fonts);
            session.offset = 0;
        }
    }

    next_cached_stream_chunk(&stream_id)
        .await
        .unwrap_or(FontStreamChunk {
            stream_id,
            fonts: Vec::new(),
            done: true,
            pending: false,
            error: None,
        })
}

#[tauri::command]
pub async fn system_fonts_stream_cancel(request: FontStreamCancelRequest) -> Result<(), String> {
    cancel_font_stream_by_id(&request.stream_id).await;
    Ok(())
}

async fn create_cached_stream(
    stream_id: String,
    fonts: Vec<String>,
    chunk_size: usize,
) -> FontStreamChunk {
    let mut session = FontStreamSession {
        fonts: Some(fonts),
        offset: 0,
        chunk_size,
        emitted: BTreeSet::new(),
    };
    let (fonts, done) = next_chunk(&mut session);
    if !done {
        streams().lock().await.insert(stream_id.clone(), session);
    }
    FontStreamChunk {
        stream_id,
        fonts,
        done,
        pending: false,
        error: None,
    }
}

async fn next_cached_stream_chunk(stream_id: &str) -> Option<FontStreamChunk> {
    let mut streams = streams().lock().await;
    let session = streams.get_mut(stream_id)?;
    session.fonts.as_ref()?;
    let (fonts, done) = next_chunk(session);
    if done {
        streams.remove(stream_id);
    }
    Some(FontStreamChunk {
        stream_id: stream_id.to_string(),
        fonts,
        done,
        pending: false,
        error: None,
    })
}

fn next_chunk(session: &mut FontStreamSession) -> (Vec<String>, bool) {
    let Some(fonts) = session.fonts.as_ref() else {
        return (Vec::new(), false);
    };
    let mut chunk = Vec::new();
    while session.offset < fonts.len() && chunk.len() < session.chunk_size {
        let font = fonts[session.offset].clone();
        session.offset += 1;
        if session.emitted.insert(font.clone()) {
            chunk.push(font);
        }
    }
    (chunk, session.offset >= fonts.len())
}

async fn start_font_loading(refresh: bool) {
    let mut font_cache = cache().write().await;
    if refresh {
        font_cache.fonts = None;
    }
    if font_cache.fonts.is_some() || font_cache.loading {
        return;
    }
    font_cache.loading = true;
    tauri::async_runtime::spawn(async {
        let fonts = load_system_fonts().await.unwrap_or_else(fallback_fonts);
        let mut font_cache = cache().write().await;
        font_cache.fonts = Some(fonts);
        font_cache.loading = false;
        notify().notify_waiters();
    });
}

async fn await_font_cache() -> Option<Vec<String>> {
    loop {
        {
            let cache = cache().read().await;
            if let Some(fonts) = &cache.fonts {
                return Some(fonts.clone());
            }
            if !cache.loading {
                return None;
            }
        }
        notify().notified().await;
    }
}

async fn cached_fonts() -> Option<Vec<String>> {
    cache().read().await.fonts.clone()
}

async fn invalidate_font_cache() {
    let mut cache = cache().write().await;
    cache.fonts = None;
}

async fn cancel_font_stream_by_id(stream_id: &str) {
    streams().lock().await.remove(stream_id);
}

async fn load_system_fonts() -> Option<Vec<String>> {
    let task = tokio::task::spawn_blocking(query_font_families);
    match tokio::time::timeout(FONT_ENUM_TIMEOUT, task).await {
        Ok(Ok(Ok(fonts))) => Some(fonts),
        Ok(Ok(Err(error))) => {
            log::warn!(target: "fonts", "failed to enumerate system fonts: {error}");
            None
        }
        Ok(Err(error)) => {
            log::warn!(target: "fonts", "system font enumeration task failed: {error}");
            None
        }
        Err(_) => {
            log::warn!(
                target: "fonts",
                "system font enumeration exceeded {} seconds; using fallback fonts",
                FONT_ENUM_TIMEOUT.as_secs()
            );
            None
        }
    }
}

fn query_font_families() -> Result<Vec<String>, String> {
    let source = SystemSource::new();
    let families = source
        .all_families()
        .map_err(|error| format!("font-kit system source failed: {error:?}"))?;
    let mut fonts = BTreeSet::new();
    for family in families {
        let normalized = normalize_font_name(&family);
        if !normalized.is_empty() {
            fonts.insert(normalized);
        }
    }
    Ok(fonts.into_iter().collect())
}

fn normalize_chunk_size(chunk_size: Option<usize>) -> usize {
    chunk_size
        .unwrap_or(FONT_STREAM_DEFAULT_CHUNK_SIZE)
        .clamp(1, FONT_STREAM_MAX_CHUNK_SIZE)
}

fn first_fallback_chunk(chunk_size: usize) -> Vec<String> {
    fallback_fonts().into_iter().take(chunk_size).collect()
}

fn cache() -> &'static RwLock<FontCache> {
    FONT_CACHE.get_or_init(|| RwLock::new(FontCache::default()))
}

fn notify() -> &'static Notify {
    FONT_CACHE_NOTIFY.get_or_init(Notify::new)
}

fn streams() -> &'static Mutex<HashMap<String, FontStreamSession>> {
    FONT_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_font_name(raw: &str) -> String {
    raw.trim().trim_matches('"').trim().to_string()
}

fn fallback_fonts() -> Vec<String> {
    ["Consolas", "Cascadia Mono", "Menlo", "Monaco", "monospace"]
        .iter()
        .map(|font| (*font).to_string())
        .collect()
}
