use russh_sftp::protocol::FileType;
use russh_sftp::protocol::OpenFlags;
use std::{
    collections::HashMap,
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

use parking_lot::Mutex;
use russh_sftp::client::SftpSession as RusshSftpSession;
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
    sync::Semaphore,
};

use crate::{
    state::AppState,
    terminal::{
        events::{emit_sftp_transfer_progress, emit_sftp_transfer_status},
        internal::{
            core::{
                SftpEntry, SftpFileStatResult, SftpTransferItem, SftpTransferRequest,
                SFTP_TRANSFER_BUFFER_BYTES,
            },
            ssh_aux::get_or_create_sftp_session,
        },
    },
};

pub(super) const SFTP_EDIT_MAX_BYTES: u64 = 5 * 1024 * 1024;
const SFTP_TRANSFER_MAX_CONCURRENT: usize = 3;
const SFTP_TRANSFER_MAX_ATTEMPTS: usize = 3;
const SFTP_TRANSFER_IO_TIMEOUT: Duration = Duration::from_secs(30);

static SFTP_TRANSFER_STORE: OnceLock<Mutex<SftpTransferStore>> = OnceLock::new();
static SFTP_TRANSFER_SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn sftp_transfer_store() -> &'static Mutex<SftpTransferStore> {
    SFTP_TRANSFER_STORE.get_or_init(|| Mutex::new(SftpTransferStore::default()))
}

fn sftp_transfer_slots() -> Arc<Semaphore> {
    SFTP_TRANSFER_SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(SFTP_TRANSFER_MAX_CONCURRENT)))
        .clone()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SftpTransferRuntimeState {
    Running,
    PauseRequested,
    CancelRequested,
}

struct SftpTransferControl {
    state: tokio::sync::Mutex<SftpTransferRuntimeState>,
}

impl SftpTransferControl {
    fn new() -> Self {
        Self {
            state: tokio::sync::Mutex::new(SftpTransferRuntimeState::Running),
        }
    }

    async fn request_pause(&self) {
        let mut state = self.state.lock().await;
        if matches!(*state, SftpTransferRuntimeState::Running) {
            *state = SftpTransferRuntimeState::PauseRequested;
        }
    }

    async fn request_cancel(&self) {
        *self.state.lock().await = SftpTransferRuntimeState::CancelRequested;
    }

    async fn checkpoint(&self) -> Result<(), SftpTransferStop> {
        match *self.state.lock().await {
            SftpTransferRuntimeState::PauseRequested => Err(SftpTransferStop::Paused),
            SftpTransferRuntimeState::CancelRequested => Err(SftpTransferStop::Canceled),
            SftpTransferRuntimeState::Running => Ok(()),
        }
    }
}

struct SftpTransferRegistryGuard {
    transfer_id: String,
}

impl Drop for SftpTransferRegistryGuard {
    fn drop(&mut self) {
        sftp_transfer_store().lock().mark_stopped(&self.transfer_id);
    }
}

enum SftpTransferStop {
    Paused,
    Canceled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SftpTransferDirection {
    Upload,
    Download,
}

impl SftpTransferDirection {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "upload" => Ok(Self::Upload),
            "download" => Ok(Self::Download),
            value => Err(format!("unsupported SFTP transfer direction '{value}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SftpUploadConflictAction {
    Create,
    Overwrite,
    Resume,
}

impl SftpUploadConflictAction {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("create") {
            "create" => Ok(Self::Create),
            "overwrite" => Ok(Self::Overwrite),
            "resume" => Ok(Self::Resume),
            value => Err(format!("unsupported SFTP upload conflict action '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SftpNameConflictAction {
    Create,
    Overwrite,
}

impl SftpNameConflictAction {
    pub(super) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("create") {
            "create" => Ok(Self::Create),
            "overwrite" => Ok(Self::Overwrite),
            value => Err(format!("unsupported SFTP rename conflict action '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SftpTransferStatus {
    Queued,
    Running,
    Paused,
}

impl SftpTransferStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }
}

#[derive(Clone, Debug)]
struct SftpTransferRecord {
    transfer_id: String,
    connection_id: String,
    session_id: String,
    direction: SftpTransferDirection,
    name: String,
    local_path: String,
    remote_path: String,
    upload_conflict_action: SftpUploadConflictAction,
    transferred: u64,
    total: u64,
    status: SftpTransferStatus,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct SftpTransferFile {
    local_path: PathBuf,
    remote_path: String,
    size: u64,
}

#[derive(Clone, Debug)]
struct SftpLocalDirectoryEntry {
    name: String,
    path: PathBuf,
    size: u64,
    is_dir: bool,
}

#[derive(Clone, Debug, Default)]
struct SftpRemoteTree {
    directories: Vec<PathBuf>,
    files: Vec<SftpTransferFile>,
}

enum SftpRemoteDeleteEntry {
    Directory(String),
    File(String),
}

struct SftpTransferAttemptError {
    record: SftpTransferRecord,
    error: String,
}

struct SftpTransferEntry {
    record: SftpTransferRecord,
    control: Option<Arc<SftpTransferControl>>,
}

#[derive(Default)]
struct SftpTransferStore {
    entries: HashMap<String, SftpTransferEntry>,
}

impl SftpTransferStore {
    fn start(&mut self, record: SftpTransferRecord) -> Result<Arc<SftpTransferControl>, String> {
        let transfer_id = record.transfer_id.clone();
        if self
            .entries
            .get(&transfer_id)
            .and_then(|entry| entry.control.as_ref())
            .is_some()
        {
            return Err(format!("SFTP transfer '{transfer_id}' already exists"));
        }
        let control = Arc::new(SftpTransferControl::new());
        self.entries.insert(
            transfer_id,
            SftpTransferEntry {
                record,
                control: Some(control.clone()),
            },
        );
        Ok(control)
    }

    fn control(&self, transfer_id: &str) -> Result<Arc<SftpTransferControl>, String> {
        self.entries
            .get(transfer_id)
            .and_then(|entry| entry.control.as_ref())
            .cloned()
            .ok_or_else(|| format!("SFTP transfer '{transfer_id}' is not running"))
    }

    fn record(&self, transfer_id: &str) -> Result<SftpTransferRecord, String> {
        self.entries
            .get(transfer_id)
            .map(|entry| entry.record.clone())
            .ok_or_else(|| format!("SFTP transfer '{transfer_id}' was not found"))
    }

    fn update(&mut self, record: &SftpTransferRecord) {
        if let Some(entry) = self.entries.get_mut(&record.transfer_id) {
            entry.record = record.clone();
        }
    }

    fn update_progress(&mut self, transfer_id: &str, transferred: u64) {
        if let Some(entry) = self.entries.get_mut(transfer_id) {
            entry.record.transferred = transferred;
        }
    }

    fn remove(&mut self, transfer_id: &str) -> Result<SftpTransferRecord, String> {
        self.entries
            .remove(transfer_id)
            .map(|entry| entry.record)
            .ok_or_else(|| format!("SFTP transfer '{transfer_id}' was not found"))
    }

    fn remove_if_exists(&mut self, transfer_id: &str) {
        self.entries.remove(transfer_id);
    }

    fn mark_stopped(&mut self, transfer_id: &str) {
        if let Some(entry) = self.entries.get_mut(transfer_id) {
            entry.control = None;
            if matches!(
                entry.record.status,
                SftpTransferStatus::Queued | SftpTransferStatus::Running
            ) {
                entry.record.status = SftpTransferStatus::Paused;
            }
        }
    }

    fn list(&self, connection_id: &str, session_id: &str) -> Vec<SftpTransferItem> {
        let mut items: Vec<_> = self
            .entries
            .values()
            .filter(|entry| {
                entry.record.connection_id == connection_id && entry.record.session_id == session_id
            })
            .map(|entry| entry.record.to_item(entry.control.is_some()))
            .collect();
        items.sort_by(|a, b| b.transfer_id.cmp(&a.transfer_id));
        items
    }

    fn controls_for_session(&self, session_id: &str) -> Vec<Arc<SftpTransferControl>> {
        self.entries
            .values()
            .filter(|entry| entry.record.session_id == session_id)
            .filter_map(|entry| entry.control.clone())
            .collect()
    }
}

impl SftpTransferRecord {
    fn list_status(&self, is_running: bool) -> &'static str {
        if is_running && self.status != SftpTransferStatus::Paused {
            SftpTransferStatus::Running.as_str()
        } else if !is_running
            && matches!(
                self.status,
                SftpTransferStatus::Running | SftpTransferStatus::Queued
            )
        {
            SftpTransferStatus::Paused.as_str()
        } else {
            self.status.as_str()
        }
    }

    fn to_item(&self, is_running: bool) -> SftpTransferItem {
        SftpTransferItem {
            transfer_id: self.transfer_id.clone(),
            connection_id: self.connection_id.clone(),
            session_id: self.session_id.clone(),
            direction: self.direction.as_str().to_string(),
            name: self.name.clone(),
            transferred: self.transferred,
            total: self.total,
            status: self.list_status(is_running).to_string(),
            error: self.error.clone(),
        }
    }
}

pub(super) struct TransferProgress {
    app: AppHandle,
    transfer_id: String,
    transferred: u64,
    total: u64,
    last_emit: Instant,
}

impl TransferProgress {
    fn new_with_offset(app: AppHandle, transfer_id: String, total: u64, transferred: u64) -> Self {
        let progress = Self {
            app,
            transfer_id,
            transferred,
            total,
            last_emit: Instant::now(),
        };
        emit_sftp_transfer_progress(
            &progress.app,
            &progress.transfer_id,
            transferred,
            total,
            false,
            None,
        );
        progress
    }

    pub(super) fn add(&mut self, bytes: u64) -> bool {
        self.transferred = self.transferred.saturating_add(bytes);
        self.emit_if_due()
    }

    fn set_total(&mut self, total: u64) -> bool {
        self.total = total;
        self.emit_if_due()
    }

    fn set_transferred(&mut self, transferred: u64) -> bool {
        self.transferred = transferred;
        self.emit_if_due()
    }

    pub(super) fn finish(&self) {
        let total = self.total.max(self.transferred);
        emit_sftp_transfer_progress(&self.app, &self.transfer_id, total, total, true, None);
    }

    fn transferred(&self) -> u64 {
        self.transferred
    }

    fn emit_if_due(&mut self) -> bool {
        if self.last_emit.elapsed() < Duration::from_millis(80) && self.transferred < self.total {
            return false;
        }
        emit_sftp_transfer_progress(
            &self.app,
            &self.transfer_id,
            self.transferred,
            self.total,
            false,
            None,
        );
        self.last_emit = Instant::now();
        true
    }
}

pub(super) async fn start_sftp_transfer_task(
    app: AppHandle,
    _state: &AppState,
    request: SftpTransferRequest,
) -> Result<String, String> {
    let record = prepare_sftp_transfer_record(&request).await?;
    let transfer_id = record.transfer_id.clone();
    spawn_registered_sftp_transfer(app, record)?;
    Ok(transfer_id)
}

pub(super) async fn pause_sftp_transfer(transfer_id: &str) -> Result<(), String> {
    let control = sftp_transfer_store().lock().control(transfer_id)?;
    control.request_pause().await;
    Ok(())
}

pub(super) async fn resume_sftp_transfer_task(
    app: AppHandle,
    _state: &AppState,
    transfer_id: &str,
) -> Result<(), String> {
    if sftp_transfer_store().lock().control(transfer_id).is_ok() {
        return Ok(());
    }
    let mut record = sftp_transfer_store().lock().record(transfer_id)?;
    record.status = SftpTransferStatus::Running;
    record.error = None;
    spawn_registered_sftp_transfer(app, record)?;
    Ok(())
}

fn spawn_registered_sftp_transfer(
    app: AppHandle,
    record: SftpTransferRecord,
) -> Result<(), String> {
    let control = sftp_transfer_store().lock().start(record.clone())?;
    let app_for_task = app.clone();
    tokio::spawn(async move {
        let state = app_for_task.state::<AppState>();
        run_sftp_transfer_task(app_for_task.clone(), state.inner(), record, control).await;
    });
    Ok(())
}

pub(super) async fn cancel_sftp_transfer_task(
    app: AppHandle,
    _state: &AppState,
    transfer_id: &str,
) -> Result<(), String> {
    let control = sftp_transfer_store().lock().control(transfer_id).ok();
    if let Some(control) = control {
        control.request_cancel().await;
        return Ok(());
    }
    let record = sftp_transfer_store().lock().remove(transfer_id)?;
    emit_sftp_transfer_status(
        &app,
        &record.transfer_id,
        record.transferred,
        record.total,
        true,
        "canceled",
        Some("canceled".to_string()),
    );
    Ok(())
}

pub(crate) fn cancel_sftp_transfers_for_session(session_id: &str) {
    let controls = sftp_transfer_store()
        .lock()
        .controls_for_session(session_id);
    if controls.is_empty() {
        return;
    }
    let session_id = session_id.to_string();
    tokio::spawn(async move {
        for control in controls {
            control.request_cancel().await;
        }
        log::debug!(target: "terminal.sftp", "requested cancellation for SFTP transfers on '{session_id}'");
    });
}

pub(super) async fn list_sftp_transfer_tasks(
    _state: &AppState,
    connection_id: &str,
    session_id: &str,
) -> Result<Vec<SftpTransferItem>, String> {
    Ok(sftp_transfer_store().lock().list(connection_id, session_id))
}

async fn prepare_sftp_transfer_record(
    request: &SftpTransferRequest,
) -> Result<SftpTransferRecord, String> {
    let direction = SftpTransferDirection::parse(&request.direction)?;
    let upload_conflict_action =
        SftpUploadConflictAction::parse(request.upload_conflict_action.as_deref())?;
    let local_path = expand_local_path(&request.local_path)?;
    let name = match direction {
        SftpTransferDirection::Upload => request
            .remote_name
            .as_deref()
            .map(validate_remote_child_name)
            .transpose()?
            .unwrap_or_else(|| local_file_name(&local_path)),
        SftpTransferDirection::Download => local_file_name(&local_path),
    };
    let remote_path = match direction {
        SftpTransferDirection::Upload => resolve_remote_child_path(
            request
                .remote_parent_path
                .as_deref()
                .ok_or_else(|| "remote parent path is required".to_string())?,
            &name,
        )?,
        SftpTransferDirection::Download => normalize_remote_path(
            request
                .remote_path
                .as_deref()
                .ok_or_else(|| "remote path is required".to_string())?,
        ),
    };

    let total = match direction {
        SftpTransferDirection::Upload => {
            let metadata = tokio::fs::metadata(&local_path).await.map_err(|error| {
                format!(
                    "failed to read local metadata '{}': {error}",
                    local_path.display()
                )
            })?;
            if metadata.is_file() {
                metadata.len()
            } else if metadata.is_dir() {
                0
            } else {
                return Err(format!(
                    "local upload path '{}' is not a regular file or directory",
                    local_path.display()
                ));
            }
        }
        SftpTransferDirection::Download => 0,
    };

    Ok(SftpTransferRecord {
        transfer_id: request
            .transfer_id
            .clone()
            .unwrap_or_else(crate::ids::new_id),
        connection_id: request.connection_id.clone(),
        session_id: request.session_id.clone(),
        direction,
        name,
        local_path: local_path.to_string_lossy().to_string(),
        remote_path,
        upload_conflict_action,
        transferred: 0,
        total,
        status: SftpTransferStatus::Queued,
        error: None,
    })
}

async fn run_sftp_transfer_task(
    app: AppHandle,
    state: &AppState,
    mut record: SftpTransferRecord,
    control: Arc<SftpTransferControl>,
) {
    let transfer_id = record.transfer_id.clone();
    let _registry_guard = SftpTransferRegistryGuard {
        transfer_id: transfer_id.clone(),
    };
    let result = async {
        let _permit = sftp_transfer_slots()
            .acquire_owned()
            .await
            .map_err(|error| format!("failed to acquire SFTP transfer slot: {error}"))?;
        control.checkpoint().await.map_err(stop_to_string)?;
        record.status = SftpTransferStatus::Running;
        record.error = None;
        sftp_transfer_store().lock().update(&record);

        let mut attempt = 0_usize;
        loop {
            attempt = attempt.saturating_add(1);
            let sftp_session =
                get_or_create_sftp_session(state, &record.connection_id, &record.session_id)
                    .await?;
            let attempt_result = match run_sftp_transfer_attempt(
                sftp_session,
                app.clone(),
                record.clone(),
                control.clone(),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => Err(SftpTransferAttemptError {
                    record: record.clone(),
                    error,
                }),
            };
            match attempt_result {
                Ok(updated_record) => {
                    record = updated_record;
                    break Ok(());
                }
                Err(failed) => {
                    record = failed.record;
                    let error = failed.error;
                    if error == "paused" || error == "canceled" {
                        break Err(error);
                    }
                    let retryable = is_retryable_sftp_transfer_error(&error);
                    if retryable {
                        state.remove_sftp_session(&record.session_id);
                    }
                    if attempt < SFTP_TRANSFER_MAX_ATTEMPTS && retryable {
                        record.error = Some(format!("retrying after transfer error: {error}"));
                        sftp_transfer_store().lock().update(&record);
                        tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
                        control.checkpoint().await.map_err(stop_to_string)?;
                        continue;
                    }
                    break Err(error);
                }
            }
        }
    }
    .await;

    match result {
        Ok(()) => {
            record.transferred = record.total.max(record.transferred);
            record.error = None;
            sftp_transfer_store()
                .lock()
                .remove_if_exists(&record.transfer_id);
            emit_sftp_transfer_progress(
                &app,
                &record.transfer_id,
                record.transferred,
                record.total,
                true,
                None,
            );
        }
        Err(error) if error == "paused" => {
            record.status = SftpTransferStatus::Paused;
            record.error = None;
            sftp_transfer_store().lock().update(&record);
            emit_sftp_transfer_status(
                &app,
                &record.transfer_id,
                record.transferred,
                record.total,
                false,
                "paused",
                None,
            );
        }
        Err(error) if error == "canceled" => {
            sftp_transfer_store()
                .lock()
                .remove_if_exists(&record.transfer_id);
            emit_sftp_transfer_status(
                &app,
                &record.transfer_id,
                record.transferred,
                record.total,
                true,
                "canceled",
                Some("canceled".to_string()),
            );
        }
        Err(error) => {
            record.error = Some(error.clone());
            sftp_transfer_store()
                .lock()
                .remove_if_exists(&record.transfer_id);
            emit_sftp_transfer_progress(
                &app,
                &record.transfer_id,
                record.transferred,
                record.total,
                true,
                Some(error),
            );
        }
    }
}

async fn run_sftp_transfer_attempt(
    sftp_session: crate::state::SftpSession,
    app: AppHandle,
    record: SftpTransferRecord,
    control: Arc<SftpTransferControl>,
) -> Result<Result<SftpTransferRecord, SftpTransferAttemptError>, String> {
    sftp_session
        .run(move |sftp| {
            Box::pin(async move {
                let mut attempt_record = record;
                let result = match attempt_record.direction {
                    SftpTransferDirection::Upload => {
                        upload_resumable_path(sftp, &app, &mut attempt_record, &control).await
                    }
                    SftpTransferDirection::Download => {
                        download_resumable_path(sftp, &app, &mut attempt_record, &control).await
                    }
                };
                match result {
                    Ok(()) => Ok(Ok(attempt_record)),
                    Err(error) => Ok(Err(SftpTransferAttemptError {
                        record: attempt_record,
                        error,
                    })),
                }
            })
        })
        .await
}

async fn upload_resumable_path(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
) -> Result<(), String> {
    let local_path = PathBuf::from(&record.local_path);
    let metadata = tokio::fs::metadata(&local_path).await.map_err(|error| {
        format!(
            "failed to read local metadata '{}': {error}",
            local_path.display()
        )
    })?;
    if metadata.is_dir() {
        return upload_resumable_directory(sftp, app, record, control, &local_path).await;
    }
    if !metadata.is_file() {
        return Err(format!(
            "local upload path '{}' is not a regular file or directory",
            local_path.display()
        ));
    }
    upload_resumable_file(sftp, app, record, control, &local_path, metadata.len()).await
}

async fn upload_resumable_file(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    local_path: &Path,
    local_len: u64,
) -> Result<(), String> {
    record.total = local_len;
    record.transferred = record.transferred.min(local_len);
    if record.transferred == 0 {
        resolve_remote_upload_target(sftp, record, local_len).await?;
    }
    sftp_transfer_store().lock().update(record);

    let mut progress = TransferProgress::new_with_offset(
        app.clone(),
        record.transfer_id.clone(),
        record.total,
        record.transferred,
    );
    if record.transferred >= record.total {
        progress.finish();
        return Ok(());
    }

    let mut input = tokio::fs::File::open(local_path).await.map_err(|error| {
        format!(
            "failed to open local file '{}': {error}",
            local_path.display()
        )
    })?;
    input
        .seek(SeekFrom::Start(record.transferred))
        .await
        .map_err(|error| {
            format!(
                "failed to seek local file '{}': {error}",
                local_path.display()
            )
        })?;
    let open_flags = if record.transferred == 0
        && record.upload_conflict_action == SftpUploadConflictAction::Overwrite
    {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    } else if record.transferred == 0 {
        OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::WRITE
    };
    let mut output = sftp
        .open_with_flags(record.remote_path.clone(), open_flags)
        .await
        .map_err(|error| {
            format!(
                "failed to open remote file '{}' for transfer: {error}",
                record.remote_path
            )
        })?;
    output
        .seek(SeekFrom::Start(record.transferred))
        .await
        .map_err(|error| {
            format!(
                "failed to seek remote file '{}': {error}",
                record.remote_path
            )
        })?;

    copy_stream_with_resume(&mut input, &mut output, &mut progress, record, control).await?;
    output.shutdown().await.map_err(|error| {
        format!(
            "failed to close remote file '{}': {error}",
            record.remote_path
        )
    })?;
    progress.finish();
    Ok(())
}

async fn upload_resumable_directory(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    local_root: &Path,
) -> Result<(), String> {
    prepare_remote_directory_upload_target(sftp, record).await?;
    record.total = record.total.max(record.transferred);
    sftp_transfer_store().lock().update(record);

    let mut progress = TransferProgress::new_with_offset(
        app.clone(),
        record.transfer_id.clone(),
        record.total,
        record.transferred,
    );
    upload_directory_streaming(sftp, record, control, &mut progress, local_root).await?;

    progress.finish();
    Ok(())
}

async fn upload_directory_streaming(
    sftp: &RusshSftpSession,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    progress: &mut TransferProgress,
    local_root: &Path,
) -> Result<(), String> {
    let mut pending = vec![(local_root.to_path_buf(), record.remote_path.clone())];
    let mut completed_before_file = 0_u64;
    let mut discovered_total = 0_u64;

    while let Some((local_directory, remote_directory)) = pending.pop() {
        control.checkpoint().await.map_err(stop_to_string)?;
        ensure_remote_dir(sftp, &remote_directory).await?;
        let mut entries = read_sorted_local_directory(&local_directory).await?;
        entries.reverse();

        for entry in entries {
            control.checkpoint().await.map_err(stop_to_string)?;
            let remote_path = join_remote_path(&remote_directory, &entry.name);
            if entry.is_dir {
                pending.push((entry.path, remote_path));
                continue;
            }

            let file = SftpTransferFile {
                local_path: entry.path,
                remote_path,
                size: entry.size,
            };
            discovered_total = discovered_total.saturating_add(file.size);
            update_streaming_total(record, progress, discovered_total);

            let next_completed = completed_before_file.saturating_add(file.size);
            if record.transferred >= next_completed {
                completed_before_file = next_completed;
                continue;
            }

            let file_offset =
                upload_file_resume_offset(sftp, record, progress, &file, completed_before_file)
                    .await?;
            if record.transferred >= next_completed {
                completed_before_file = next_completed;
                continue;
            }

            copy_local_file_to_remote(
                sftp,
                &file.local_path,
                &file.remote_path,
                file_offset,
                progress,
                record,
                control,
            )
            .await?;
            completed_before_file = next_completed;
        }
    }

    record.total = discovered_total;
    record.transferred = record.transferred.min(record.total);
    progress.set_total(record.total);
    sftp_transfer_store().lock().update(record);
    Ok(())
}

async fn download_resumable_path(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
) -> Result<(), String> {
    let local_path = PathBuf::from(&record.local_path);
    let metadata = remote_metadata(sftp, &record.remote_path).await?;
    if metadata.file_type().is_dir() {
        return download_resumable_directory(sftp, app, record, control, &local_path).await;
    }
    download_resumable_file(sftp, app, record, control, &local_path, metadata.len()).await
}

async fn download_resumable_file(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    local_path: &Path,
    remote_len: u64,
) -> Result<(), String> {
    ensure_local_parent_dir(local_path).await?;
    record.total = remote_len;
    record.transferred = record.transferred.min(record.total);
    sftp_transfer_store().lock().update(record);

    let mut progress = TransferProgress::new_with_offset(
        app.clone(),
        record.transfer_id.clone(),
        record.total,
        record.transferred,
    );
    let mut input = sftp
        .open(record.remote_path.clone())
        .await
        .map_err(|error| {
            format!(
                "failed to open remote file '{}': {error}",
                record.remote_path
            )
        })?;
    input
        .seek(SeekFrom::Start(record.transferred))
        .await
        .map_err(|error| {
            format!(
                "failed to seek remote file '{}': {error}",
                record.remote_path
            )
        })?;
    let mut output = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(record.transferred == 0)
        .write(true)
        .open(&local_path)
        .await
        .map_err(|error| {
            format!(
                "failed to open local file '{}': {error}",
                local_path.display()
            )
        })?;
    output.set_len(record.transferred).await.map_err(|error| {
        format!(
            "failed to resize local file '{}' for resume: {error}",
            local_path.display()
        )
    })?;
    output
        .seek(SeekFrom::Start(record.transferred))
        .await
        .map_err(|error| {
            format!(
                "failed to seek local file '{}': {error}",
                local_path.display()
            )
        })?;

    copy_stream_with_resume(&mut input, &mut output, &mut progress, record, control).await?;
    output.shutdown().await.map_err(|error| {
        format!(
            "failed to close local file '{}': {error}",
            local_path.display()
        )
    })?;
    progress.finish();
    Ok(())
}

async fn download_resumable_directory(
    sftp: &RusshSftpSession,
    app: &AppHandle,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    local_root: &Path,
) -> Result<(), String> {
    tokio::fs::create_dir_all(local_root)
        .await
        .map_err(|error| {
            format!(
                "failed to create local directory '{}': {error}",
                local_root.display()
            )
        })?;
    let tree = collect_remote_tree(sftp, &record.remote_path, local_root).await?;
    record.total = tree.files.iter().map(|file| file.size).sum();
    record.transferred = record.transferred.min(record.total);
    sftp_transfer_store().lock().update(record);

    let mut progress = TransferProgress::new_with_offset(
        app.clone(),
        record.transfer_id.clone(),
        record.total,
        record.transferred,
    );
    for directory in &tree.directories {
        control.checkpoint().await.map_err(stop_to_string)?;
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|error| {
                format!(
                    "failed to create local directory '{}': {error}",
                    directory.display()
                )
            })?;
    }
    if record.total == 0 {
        progress.finish();
        return Ok(());
    }

    download_planned_files(sftp, record, control, &mut progress, tree.files).await?;

    progress.finish();
    Ok(())
}

async fn download_planned_files(
    sftp: &RusshSftpSession,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
    progress: &mut TransferProgress,
    files: Vec<SftpTransferFile>,
) -> Result<(), String> {
    let mut completed_before_file = 0_u64;
    for file in files {
        control.checkpoint().await.map_err(stop_to_string)?;
        let next_completed = completed_before_file.saturating_add(file.size);
        if record.transferred >= next_completed {
            completed_before_file = next_completed;
            continue;
        }
        let file_offset = record.transferred.saturating_sub(completed_before_file);
        copy_remote_file_to_local(
            sftp,
            &file.remote_path,
            &file.local_path,
            file_offset,
            progress,
            record,
            control,
        )
        .await?;
        completed_before_file = next_completed;
    }
    Ok(())
}

async fn copy_local_file_to_remote(
    sftp: &RusshSftpSession,
    local_path: &Path,
    remote_path: &str,
    offset: u64,
    progress: &mut TransferProgress,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
) -> Result<(), String> {
    let mut input = tokio::fs::File::open(local_path).await.map_err(|error| {
        format!(
            "failed to open local file '{}': {error}",
            local_path.display()
        )
    })?;
    input.seek(SeekFrom::Start(offset)).await.map_err(|error| {
        format!(
            "failed to seek local file '{}': {error}",
            local_path.display()
        )
    })?;
    let open_flags = if offset == 0 {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::WRITE
    };
    let mut output = sftp
        .open_with_flags(remote_path.to_string(), open_flags)
        .await
        .map_err(|error| format!("failed to open remote file '{remote_path}': {error}"))?;
    output
        .seek(SeekFrom::Start(offset))
        .await
        .map_err(|error| format!("failed to seek remote file '{remote_path}': {error}"))?;

    copy_stream_with_resume(&mut input, &mut output, progress, record, control).await?;
    output
        .shutdown()
        .await
        .map_err(|error| format!("failed to close remote file '{remote_path}': {error}"))
}

async fn copy_remote_file_to_local(
    sftp: &RusshSftpSession,
    remote_path: &str,
    local_path: &Path,
    offset: u64,
    progress: &mut TransferProgress,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
) -> Result<(), String> {
    ensure_local_parent_dir(local_path).await?;
    let mut input = sftp
        .open(remote_path.to_string())
        .await
        .map_err(|error| format!("failed to open remote file '{remote_path}': {error}"))?;
    input
        .seek(SeekFrom::Start(offset))
        .await
        .map_err(|error| format!("failed to seek remote file '{remote_path}': {error}"))?;
    let mut output = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(offset == 0)
        .write(true)
        .open(local_path)
        .await
        .map_err(|error| {
            format!(
                "failed to open local file '{}': {error}",
                local_path.display()
            )
        })?;
    output.set_len(offset).await.map_err(|error| {
        format!(
            "failed to resize local file '{}' for resume: {error}",
            local_path.display()
        )
    })?;
    output
        .seek(SeekFrom::Start(offset))
        .await
        .map_err(|error| {
            format!(
                "failed to seek local file '{}': {error}",
                local_path.display()
            )
        })?;

    copy_stream_with_resume(&mut input, &mut output, progress, record, control).await?;
    output.shutdown().await.map_err(|error| {
        format!(
            "failed to close local file '{}': {error}",
            local_path.display()
        )
    })
}

async fn copy_stream_with_resume<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &mut TransferProgress,
    record: &mut SftpTransferRecord,
    control: &SftpTransferControl,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; SFTP_TRANSFER_BUFFER_BYTES];
    loop {
        control.checkpoint().await.map_err(stop_to_string)?;
        let size = tokio::time::timeout(SFTP_TRANSFER_IO_TIMEOUT, reader.read(&mut buffer))
            .await
            .map_err(|_| {
                format!(
                    "timed out reading the SFTP transfer stream after {} seconds",
                    SFTP_TRANSFER_IO_TIMEOUT.as_secs()
                )
            })?
            .map_err(|error| format!("failed to read transfer stream: {error}"))?;
        if size == 0 {
            break;
        }
        tokio::time::timeout(SFTP_TRANSFER_IO_TIMEOUT, writer.write_all(&buffer[..size]))
            .await
            .map_err(|_| {
                format!(
                    "timed out writing the SFTP transfer stream after {} seconds",
                    SFTP_TRANSFER_IO_TIMEOUT.as_secs()
                )
            })?
            .map_err(|error| format!("failed to write transfer stream: {error}"))?;
        let emitted = progress.add(size as u64);
        record.transferred = progress.transferred();
        if emitted {
            sftp_transfer_store()
                .lock()
                .update_progress(&record.transfer_id, record.transferred);
        }
    }
    tokio::time::timeout(SFTP_TRANSFER_IO_TIMEOUT, writer.flush())
        .await
        .map_err(|_| {
            format!(
                "timed out flushing the SFTP transfer stream after {} seconds",
                SFTP_TRANSFER_IO_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| format!("failed to flush transfer stream: {error}"))
}

fn stop_to_string(stop: SftpTransferStop) -> String {
    match stop {
        SftpTransferStop::Paused => "paused".to_string(),
        SftpTransferStop::Canceled => "canceled".to_string(),
    }
}

fn is_retryable_sftp_transfer_error(error: &str) -> bool {
    !error.starts_with("unsupported SFTP transfer direction")
        && !error.starts_with("unsupported SFTP upload conflict action")
        && !error.contains("is required")
        && !error.starts_with("remote upload target is a directory")
        && !error.starts_with("remote upload target is larger than local file")
        && !error.starts_with("remote upload target already exists")
}

async fn resolve_remote_upload_target(
    sftp: &RusshSftpSession,
    record: &mut SftpTransferRecord,
    local_len: u64,
) -> Result<(), String> {
    let exists = sftp
        .try_exists(record.remote_path.clone())
        .await
        .map_err(|error| {
            format!(
                "failed to check remote upload target '{}': {error}",
                record.remote_path
            )
        })?;
    if !exists {
        return Ok(());
    }

    let metadata = sftp
        .metadata(record.remote_path.clone())
        .await
        .map_err(|error| {
            format!(
                "failed to stat remote upload target '{}': {error}",
                record.remote_path
            )
        })?;
    if metadata.file_type().is_dir() {
        return Err(format!(
            "remote upload target is a directory; choose that directory as the upload destination instead: {}",
            record.remote_path
        ));
    }

    match record.upload_conflict_action {
        SftpUploadConflictAction::Overwrite => Ok(()),
        SftpUploadConflictAction::Resume => {
            let remote_len = metadata.len();
            if remote_len > local_len {
                return Err(format!(
                    "remote upload target is larger than local file; cannot resume upload: {}",
                    record.remote_path
                ));
            }
            record.transferred = remote_len;
            Ok(())
        }
        SftpUploadConflictAction::Create => Err(format!(
            "remote upload target already exists; choose overwrite or resume before uploading: {}",
            record.remote_path
        )),
    }
}

async fn read_sorted_local_directory(
    directory: &Path,
) -> Result<Vec<SftpLocalDirectoryEntry>, String> {
    let mut output = Vec::new();
    let mut entries = tokio::fs::read_dir(directory).await.map_err(|error| {
        format!(
            "failed to read local directory '{}': {error}",
            directory.display()
        )
    })?;
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        format!(
            "failed to read local directory '{}': {error}",
            directory.display()
        )
    })? {
        let path = entry.path();
        let metadata = entry.metadata().await.map_err(|error| {
            format!(
                "failed to read local metadata '{}': {error}",
                path.display()
            )
        })?;
        if !metadata.is_dir() && !metadata.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        output.push(SftpLocalDirectoryEntry {
            name,
            path,
            size: metadata.len(),
            is_dir: metadata.is_dir(),
        });
    }
    output.sort_by(|a, b| {
        a.is_dir
            .cmp(&b.is_dir)
            .reverse()
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(output)
}

async fn collect_remote_tree(
    sftp: &RusshSftpSession,
    remote_root: &str,
    local_root: &Path,
) -> Result<SftpRemoteTree, String> {
    let mut tree = SftpRemoteTree::default();
    let mut pending = vec![(remote_root.to_string(), local_root.to_path_buf())];
    while let Some((remote_directory, local_directory)) = pending.pop() {
        let entries = sftp
            .read_dir(remote_directory.clone())
            .await
            .map_err(|error| {
                format!("failed to read remote directory '{remote_directory}': {error}")
            })?;
        for entry in entries {
            let name = entry.file_name().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let metadata = entry.metadata();
            let remote_path = join_remote_path(&remote_directory, &name);
            let local_path = local_directory.join(&name);
            if metadata.file_type().is_dir() {
                tree.directories.push(local_path.clone());
                pending.push((remote_path, local_path));
            } else {
                tree.files.push(SftpTransferFile {
                    remote_path,
                    local_path,
                    size: metadata.len(),
                });
            }
        }
    }
    tree.directories.sort();
    tree.directories.dedup();
    tree.files.sort_by(|a, b| a.remote_path.cmp(&b.remote_path));
    Ok(tree)
}

async fn prepare_remote_directory_upload_target(
    sftp: &RusshSftpSession,
    record: &SftpTransferRecord,
) -> Result<(), String> {
    let exists = sftp
        .try_exists(record.remote_path.clone())
        .await
        .map_err(|error| {
            format!(
                "failed to check remote upload target '{}': {error}",
                record.remote_path
            )
        })?;
    if !exists {
        return ensure_remote_dir(sftp, &record.remote_path).await;
    }

    let metadata = remote_metadata(sftp, &record.remote_path).await?;
    if !metadata.file_type().is_dir() {
        return Err(format!(
            "remote upload target already exists as a file; choose another folder name before uploading: {}",
            record.remote_path
        ));
    }

    match record.upload_conflict_action {
        SftpUploadConflictAction::Overwrite => {
            delete_remote_path(sftp, &record.remote_path).await?;
            ensure_remote_dir(sftp, &record.remote_path).await
        }
        SftpUploadConflictAction::Create => Err(format!(
            "remote upload target already exists; choose overwrite or resume before uploading: {}",
            record.remote_path
        )),
        SftpUploadConflictAction::Resume => ensure_remote_dir(sftp, &record.remote_path).await,
    }
}

async fn remote_file_resume_offset(
    sftp: &RusshSftpSession,
    file: &SftpTransferFile,
) -> Result<u64, String> {
    let exists = sftp
        .try_exists(file.remote_path.clone())
        .await
        .map_err(|error| {
            format!(
                "failed to check remote upload target '{}': {error}",
                file.remote_path
            )
        })?;
    if !exists {
        return Ok(0);
    }

    let metadata = remote_metadata(sftp, &file.remote_path).await?;
    if metadata.file_type().is_dir() {
        return Err(format!(
            "remote upload target already exists as a directory: {}",
            file.remote_path
        ));
    }
    let remote_len = metadata.len();
    if remote_len > file.size {
        return Err(format!(
            "remote upload target is larger than local file; cannot resume upload: {}",
            file.remote_path
        ));
    }
    Ok(remote_len)
}

async fn upload_file_resume_offset(
    sftp: &RusshSftpSession,
    record: &mut SftpTransferRecord,
    progress: &mut TransferProgress,
    file: &SftpTransferFile,
    completed_before_file: u64,
) -> Result<u64, String> {
    if record.transferred != completed_before_file
        || record.upload_conflict_action != SftpUploadConflictAction::Resume
    {
        return Ok(record.transferred.saturating_sub(completed_before_file));
    }

    let offset = remote_file_resume_offset(sftp, file).await?;
    if offset > 0 {
        record.transferred = completed_before_file.saturating_add(offset);
        progress.set_transferred(record.transferred);
        sftp_transfer_store().lock().update(record);
    }
    Ok(offset)
}

fn update_streaming_total(
    record: &mut SftpTransferRecord,
    progress: &mut TransferProgress,
    discovered_total: u64,
) {
    let next_total = discovered_total.max(record.transferred);
    if record.total == next_total {
        return;
    }
    record.total = next_total;
    progress.set_total(next_total);
    sftp_transfer_store().lock().update(record);
}

async fn ensure_local_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            format!(
                "failed to create local directory '{}': {error}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

async fn remote_metadata(
    sftp: &RusshSftpSession,
    remote_path: &str,
) -> Result<russh_sftp::client::fs::Metadata, String> {
    sftp.metadata(remote_path.to_string())
        .await
        .map_err(|error| format!("failed to stat remote path '{remote_path}': {error}"))
}

fn local_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("transfer")
        .to_string()
}

pub(super) fn sort_sftp_entries(entries: &mut [SftpEntry]) {
    entries.sort_by(|a, b| {
        let rank_a = if a.kind == "dir" { 0 } else { 1 };
        let rank_b = if b.kind == "dir" { 0 } else { 1 };
        rank_a
            .cmp(&rank_b)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub(super) fn remote_file_kind(file_type: FileType) -> String {
    match file_type {
        FileType::Dir => "dir",
        FileType::Symlink => "symlink",
        _ => "file",
    }
    .to_string()
}

pub(super) fn sftp_file_stat_result(
    path: String,
    metadata: &russh_sftp::client::fs::Metadata,
) -> SftpFileStatResult {
    SftpFileStatResult {
        path,
        kind: remote_file_kind(metadata.file_type()),
        size: metadata.len(),
        modified: remote_modified_timestamp(metadata),
    }
}

pub(super) fn remote_modified_timestamp(
    metadata: &russh_sftp::client::fs::Metadata,
) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

pub(super) fn normalize_remote_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ".".to_string();
    }
    let mut parts = Vec::new();
    let absolute = trimmed.starts_with('/');
    for part in trimmed.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    let joined = parts.join("/");
    if absolute {
        let result = format!("/{joined}");
        let result = result.trim_end_matches('/');
        if result.is_empty() {
            "/".to_string()
        } else {
            result.to_string()
        }
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

pub(super) fn join_remote_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{name}")
    } else if base == "." || base.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

pub(super) fn validate_remote_child_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("remote entry name is required".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("remote entry name cannot contain path separators".to_string());
    }
    Ok(trimmed.to_string())
}

pub(super) fn resolve_remote_child_path(parent_path: &str, name: &str) -> Result<String, String> {
    let parent = normalize_remote_path(parent_path);
    let name = validate_remote_child_name(name)?;
    Ok(normalize_remote_path(&join_remote_path(&parent, &name)))
}

pub(super) fn remote_parent_path(path: &str) -> Option<String> {
    if path == "/" || path == "." {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    let Some(index) = trimmed.rfind('/') else {
        return Some(".".to_string());
    };
    if index == 0 {
        Some("/".to_string())
    } else {
        Some(trimmed[..index].to_string())
    }
}

pub(super) fn expand_local_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        let home = home_dir().ok_or_else(|| "failed to resolve home directory".to_string())?;
        let rest = trimmed
            .trim_start_matches('~')
            .trim_start_matches(['/', '\\']);
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

pub(super) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub(super) async fn delete_remote_path(
    sftp: &RusshSftpSession,
    remote_path: &str,
) -> Result<(), String> {
    let metadata = remote_metadata(sftp, remote_path).await?;
    if !metadata.file_type().is_dir() {
        return delete_remote_file(sftp, remote_path).await;
    }
    delete_remote_tree(sftp, remote_path).await
}

async fn delete_remote_tree(sftp: &RusshSftpSession, root_path: &str) -> Result<(), String> {
    let mut pending = vec![(root_path.to_string(), false)];

    while let Some((directory, visited)) = pending.pop() {
        if visited {
            delete_remote_dir(sftp, &directory).await?;
            continue;
        }

        pending.push((directory.clone(), true));
        for child in read_remote_directory(sftp, &directory).await? {
            match child {
                SftpRemoteDeleteEntry::Directory(path) => pending.push((path, false)),
                SftpRemoteDeleteEntry::File(path) => delete_remote_file(sftp, &path).await?,
            }
        }
    }
    Ok(())
}

async fn delete_remote_file(sftp: &RusshSftpSession, path: &str) -> Result<(), String> {
    sftp.remove_file(path.to_string())
        .await
        .map_err(|error| format!("failed to delete remote file '{path}': {error}"))
}

async fn delete_remote_dir(sftp: &RusshSftpSession, path: &str) -> Result<(), String> {
    sftp.remove_dir(path.to_string())
        .await
        .map_err(|error| format!("failed to delete remote directory '{path}': {error}"))
}

async fn read_remote_directory(
    sftp: &RusshSftpSession,
    directory: &str,
) -> Result<Vec<SftpRemoteDeleteEntry>, String> {
    Ok(sftp
        .read_dir(directory.to_string())
        .await
        .map_err(|error| format!("failed to read remote directory '{directory}': {error}"))?
        .map(|child| {
            let path = join_remote_path(directory, &child.file_name());
            if child.metadata().file_type().is_dir() {
                SftpRemoteDeleteEntry::Directory(path)
            } else {
                SftpRemoteDeleteEntry::File(path)
            }
        })
        .collect())
}

pub(super) async fn ensure_remote_dir(sftp: &RusshSftpSession, path: &str) -> Result<(), String> {
    if sftp.try_exists(path.to_string()).await.unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = remote_parent_path(path) {
        Box::pin(ensure_remote_dir(sftp, &parent)).await?;
    }
    sftp.create_dir(path.to_string())
        .await
        .map_err(|error| format!("failed to create remote directory '{path}': {error}"))
}

pub(super) async fn rename_remote_path(
    sftp: &RusshSftpSession,
    from_path: &str,
    to_path: &str,
    conflict_action: SftpNameConflictAction,
) -> Result<(), String> {
    if normalize_remote_path(from_path) == normalize_remote_path(to_path) {
        return Ok(());
    }

    let target_exists = sftp
        .try_exists(to_path.to_string())
        .await
        .map_err(|error| format!("failed to check remote rename target '{to_path}': {error}"))?;

    if target_exists {
        match conflict_action {
            SftpNameConflictAction::Create => {
                return Err(format!(
                    "remote rename target already exists; choose overwrite before renaming: {to_path}"
                ));
            }
            SftpNameConflictAction::Overwrite => {
                delete_remote_path(sftp, to_path).await?;
            }
        }
    }

    sftp.rename(from_path.to_string(), to_path.to_string())
        .await
        .map_err(|error| format!("failed to rename remote path '{from_path}': {error}"))
}
