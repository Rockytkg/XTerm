//! A single TFTP transfer session: after `server` accepts an RRQ/WRQ, the
//! session owns a dedicated transfer socket (its TID, RFC 1350) and drives
//! the send/receive state machine to completion.

use std::{
    fs, io,
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::{watch, Semaphore},
    time::{timeout, Instant},
};

use crate::{
    file_service::models::{FileTransferEvent, TransferRegistry},
    logging,
};

use super::{
    error::{io_transfer_error, TransferError},
    packet::{
        ack_packet, data_packet, decode_netascii, decode_transfer_packet, encode_netascii,
        negotiate, oack_packet, request_tsize, Negotiated, Opcode, Request, TransferMode,
        TransferPacket, MAX_PACKET_SIZE,
    },
};

const TFTP_MAX_RETRIES: usize = 5;
const TFTP_PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

/// Tracks the requests that currently own a session so the accept loops can
/// drop retransmitted RRQ/WRQ duplicates instead of spawning phantom
/// transfers.  Keyed by (peer, opcode, filename).
pub(super) type ActiveRequests =
    Arc<parking_lot::Mutex<std::collections::HashSet<(SocketAddr, Opcode, String)>>>;

/// How sessions report progress to the UI.  A closure (rather than an
/// `AppHandle`) keeps the session logic testable without a Tauri runtime.
pub(super) type TransferEmitter = Arc<dyn Fn(FileTransferEvent) + Send + Sync>;

#[derive(Clone)]
pub(super) struct SessionContext {
    pub(super) emitter: TransferEmitter,
    pub(super) shared: Arc<TransferRegistry>,
    pub(super) root: PathBuf,
    pub(super) local_ip: IpAddr,
    pub(super) session_permits: Arc<Semaphore>,
    pub(super) active_requests: ActiveRequests,
}

struct ResolvedTransfer {
    request: Request,
    path: PathBuf,
    name: String,
    id: String,
}

/// Buffers an upload under a hidden temporary name and only replaces the
/// target on success, so a failed transfer never corrupts the existing file.
struct PendingUpload {
    path: PathBuf,
    committed: bool,
}

fn target_file_name(target: &Path) -> &str {
    target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload")
}

impl PendingUpload {
    fn new(target: &Path, transfer_id: &str) -> Self {
        Self {
            path: target
                .with_file_name(format!(".{}.{transfer_id}.part", target_file_name(target))),
            committed: false,
        }
    }

    fn commit(mut self, target: &Path) -> Result<(), String> {
        let backup = target.with_file_name(format!(
            ".{}.{}.backup",
            target_file_name(target),
            crate::ids::new_id()
        ));
        let had_target = target.exists();
        if had_target {
            fs::rename(target, &backup).map_err(|error| {
                format!(
                    "failed to preserve existing '{}': {error}",
                    target.display()
                )
            })?;
        }
        if let Err(error) = fs::rename(&self.path, target) {
            if had_target {
                let _ = fs::rename(&backup, target);
            }
            return Err(format!(
                "failed to commit uploaded file '{}': {error}",
                target.display()
            ));
        }
        self.committed = true;
        if had_target {
            let _ = fs::remove_file(backup);
        }
        Ok(())
    }
}

impl Drop for PendingUpload {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Emits throttled progress events and guarantees a terminal event: if the
/// transfer task dies without calling `finish`, `Drop` closes the registry
/// entry instead of leaking an "active" transfer forever.
struct TransferProgress {
    emitter: TransferEmitter,
    shared: Arc<TransferRegistry>,
    id: String,
    last_emit: Instant,
    finished: bool,
}

impl TransferProgress {
    fn new(context: &SessionContext, id: String) -> Self {
        Self {
            emitter: context.emitter.clone(),
            shared: context.shared.clone(),
            id,
            last_emit: Instant::now(),
            finished: false,
        }
    }

    fn add(&mut self, bytes: usize, force: bool) {
        if let Some(event) = self.shared.record_progress(&self.id, bytes as u64) {
            if force || self.last_emit.elapsed() >= TFTP_PROGRESS_INTERVAL {
                (self.emitter)(event);
                self.last_emit = Instant::now();
            }
        }
    }

    fn finish(mut self, result: &Result<(), TransferError>) {
        let error = result.as_ref().err().map(ToString::to_string);
        if let Some(event) = self.shared.finish_transfer(&self.id, error) {
            (self.emitter)(event);
        }
        self.finished = true;
    }
}

impl Drop for TransferProgress {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(event) = self.shared.finish_transfer(
            &self.id,
            Some("TFTP transfer task ended before completion".to_string()),
        ) {
            (self.emitter)(event);
        }
    }
}

pub(super) async fn serve_session(
    context: SessionContext,
    peer: SocketAddr,
    request: Request,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), TransferError> {
    // Bound the number of concurrent transfers so a flood of requests cannot
    // exhaust file handles or ephemeral ports.
    let _session_permit = context
        .session_permits
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| TransferError::undefined("TFTP session limiter closed"))?;
    let transfer_ip = if context.local_ip.is_unspecified() {
        discover_local_ip(peer).map_err(|error| {
            TransferError::undefined(format!("failed to select TFTP interface: {error}"))
        })?
    } else {
        context.local_ip
    };
    let socket = UdpSocket::bind(SocketAddr::new(transfer_ip, 0))
        .await
        .map_err(|error| {
            TransferError::undefined(format!("failed to bind TFTP transfer socket: {error}"))
        })?;
    logging::event("tftp.runtime", "tftp.session.open")
        .field("peer", peer)
        .maybe_field("transfer_addr", socket.local_addr().ok())
        .info();

    let result = serve_transfer(&context, &socket, peer, request, &mut shutdown_rx).await;
    if let Err(TransferError::Send(code, message)) = &result {
        let _ = send_error(&socket, peer, *code, message).await;
    }
    result
}

async fn serve_transfer(
    context: &SessionContext,
    socket: &UdpSocket,
    peer: SocketAddr,
    request: Request,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    let path = secure_transfer_path(&context.root, Path::new(&request.filename)).await?;
    let name = Path::new(&request.filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&request.filename)
        .to_string();
    let transfer = ResolvedTransfer {
        request,
        path,
        name,
        id: crate::ids::new_id(),
    };
    match transfer.request.opcode {
        Opcode::Read => serve_read(context, socket, peer, transfer, shutdown_rx).await,
        Opcode::Write => serve_write(context, socket, peer, transfer, shutdown_rx).await,
    }
}

async fn serve_read(
    context: &SessionContext,
    socket: &UdpSocket,
    peer: SocketAddr,
    transfer: ResolvedTransfer,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    let ResolvedTransfer {
        request,
        path,
        name,
        id,
    } = transfer;
    let mut file = File::open(&path)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                TransferError::not_found(format!("file not found '{}': {error}", path.display()))
            }
            io::ErrorKind::PermissionDenied => TransferError::access_violation(format!(
                "Access violation reading '{}': {error}",
                path.display()
            )),
            _ => TransferError::undefined(format!("failed to open '{}': {error}", path.display())),
        })?;
    let mut source = Vec::new();
    file.read_to_end(&mut source).await.map_err(|error| {
        TransferError::undefined(format!("failed to read '{}': {error}", path.display()))
    })?;
    if request.mode == TransferMode::Netascii {
        source = encode_netascii(&source);
    }
    // RFC 2349 reports the octets that the client will receive.  Netascii
    // expands LF and CR while converting the local representation.
    let transfer_size = source.len() as u64;
    // Validate the options before announcing the transfer so an invalid option
    // list never produces phantom transfer events.
    let negotiated = negotiate(&request, transfer_size, true)?;
    context
        .shared
        .start_transfer(&id, "read", &name, &peer.to_string(), transfer_size);
    emit_transfer_start(context, &id);
    let mut progress = TransferProgress::new(context, id);
    let result = match send_oack_or_start(socket, peer, &negotiated, shutdown_rx).await {
        Ok(()) => {
            send_file(
                socket,
                peer,
                &source,
                negotiated,
                &mut progress,
                shutdown_rx,
            )
            .await
        }
        Err(error) => Err(error),
    };
    progress.finish(&result);
    result
}

async fn serve_write(
    context: &SessionContext,
    socket: &UdpSocket,
    peer: SocketAddr,
    transfer: ResolvedTransfer,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    let ResolvedTransfer {
        request,
        path,
        name,
        id,
    } = transfer;
    // Validate all negotiated options before mutating the target path.
    let transfer_size = request_tsize(&request);
    let negotiated = negotiate(&request, transfer_size, false)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            TransferError::access_violation(format!(
                "failed to create '{}': {error}",
                parent.display()
            ))
        })?;
    }
    let pending_upload = PendingUpload::new(&path, &id);
    let mut file = File::create(&pending_upload.path).await.map_err(|error| {
        TransferError::access_violation(format!(
            "failed to create temporary upload '{}': {error}",
            pending_upload.path.display()
        ))
    })?;
    context
        .shared
        .start_transfer(&id, "write", &name, &peer.to_string(), transfer_size);
    emit_transfer_start(context, &id);
    let mut progress = TransferProgress::new(context, id);
    let mut target = UploadTarget {
        file: &mut file,
        path: &pending_upload.path,
        mode: request.mode,
    };
    let mut result = receive_file(
        socket,
        peer,
        &mut target,
        negotiated,
        &mut progress,
        shutdown_rx,
    )
    .await;
    if result.is_ok() {
        if let Err(error) = file.flush().await {
            result = Err(io_transfer_error("flush", &path, error));
        }
    }
    drop(file);
    if result.is_ok() {
        // rename() is synchronous; keep it off the async runtime threads.
        let target = path.clone();
        result = match tokio::task::spawn_blocking(move || pending_upload.commit(&target)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(TransferError::access_violation(error)),
            Err(error) => Err(TransferError::undefined(format!(
                "failed to commit upload task: {error}"
            ))),
        };
    }
    progress.finish(&result);
    result
}

struct DataWindow {
    packets: Vec<Vec<u8>>,
    payload_bytes: usize,
    offset: usize,
    next_block: u16,
    last_block: u16,
    is_final: bool,
}

/// Builds one RFC 7440 window of DATA packets starting at `offset`/`block`.
/// `next_block` already points past the last queued block (with u16 rollover),
/// so the caller must NOT increment it again between windows.
fn build_data_window(
    source: &[u8],
    mut offset: usize,
    mut block: u16,
    block_size: usize,
    window_size: usize,
) -> DataWindow {
    let mut packets = Vec::new();
    let mut payload_bytes = 0;
    let mut last_block = block;
    let mut is_final = false;
    for _ in 0..window_size.max(1) {
        let size = (source.len() - offset).min(block_size);
        packets.push(data_packet(block, &source[offset..offset + size]));
        payload_bytes += size;
        last_block = block;
        offset += size;
        if size < block_size {
            is_final = true;
            break;
        }
        block = block.wrapping_add(1);
    }
    DataWindow {
        packets,
        payload_bytes,
        offset,
        next_block: block,
        last_block,
        is_final,
    }
}

async fn send_file(
    socket: &UdpSocket,
    peer: SocketAddr,
    source: &[u8],
    negotiated: Negotiated,
    progress: &mut TransferProgress,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    let mut offset = 0;
    let mut block = 1u16;
    loop {
        let window = build_data_window(
            source,
            offset,
            block,
            negotiated.block_size,
            negotiated.window_size,
        );
        let is_final = window.is_final;
        send_window_with_ack(
            socket,
            peer,
            &window.packets,
            window.last_block,
            negotiated.timeout,
            shutdown_rx,
        )
        .await?;
        progress.add(window.payload_bytes, is_final);
        if is_final {
            return Ok(());
        }
        offset = window.offset;
        block = window.next_block;
    }
}

struct UploadTarget<'a> {
    file: &'a mut File,
    path: &'a Path,
    mode: TransferMode,
}

impl UploadTarget<'_> {
    async fn write(&mut self, data: &[u8]) -> Result<(), TransferError> {
        self.file
            .write_all(data)
            .await
            .map_err(|error| io_transfer_error("write", self.path, error))
    }
}

async fn receive_file(
    socket: &UdpSocket,
    peer: SocketAddr,
    target: &mut UploadTarget<'_>,
    negotiated: Negotiated,
    progress: &mut TransferProgress,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    // For WRQ an OACK is acknowledged by DATA(1), per RFC 2347 §5.
    let initial = initial_write_packet(&negotiated);
    send_packet(socket, peer, &initial).await?;
    let mut expected = 1u16;
    // Until the first DATA block arrives this is the OACK (or ACK(0) when no
    // options were negotiated); afterwards it becomes the ACK of the last
    // correctly received block, used for retransmits per RFC 1350/7440.
    let mut retry_packet = initial;
    let mut packet = [0u8; MAX_PACKET_SIZE];
    let mut retries = 0;
    let mut in_window = 0usize;
    let mut pending_cr = false;
    loop {
        let Some(size) =
            recv_packet(socket, peer, &mut packet, negotiated.timeout, shutdown_rx).await?
        else {
            retries += 1;
            if retries > TFTP_MAX_RETRIES {
                return Err(TransferError::Timeout(format!(
                    "timed out waiting for DATA {expected}"
                )));
            }
            send_packet(socket, peer, &retry_packet).await?;
            continue;
        };
        retries = 0;
        let (block, data) = match decode_transfer_packet(&packet[..size])? {
            TransferPacket::Data { block, payload } if payload.len() <= negotiated.block_size => {
                (block, payload)
            }
            TransferPacket::Data { .. } => {
                return Err(TransferError::illegal("oversized DATA packet"))
            }
            TransferPacket::Error { code, message } => {
                return Err(TransferError::Peer(format!(
                    "peer sent ERROR {code}: {message}"
                )))
            }
            TransferPacket::Ack(_) => return Err(TransferError::illegal("expected DATA packet")),
        };
        if block == expected {
            let raw_len = data.len();
            let data = if target.mode == TransferMode::Netascii {
                decode_netascii(data, &mut pending_cr)
            } else {
                data.to_vec()
            };
            target.write(&data).await?;
            retry_packet = ack_packet(block).to_vec();
            in_window += 1;
            let final_data = raw_len < negotiated.block_size;
            if in_window == negotiated.window_size || final_data {
                send_packet(socket, peer, &retry_packet).await?;
                in_window = 0;
            }
            progress.add(raw_len, final_data);
            if final_data {
                if pending_cr {
                    target.write(b"\r").await?;
                }
                return Ok(());
            }
            expected = expected.wrapping_add(1);
        } else {
            // RFC 7440: re-acknowledge the last correctly received block for
            // any sequence error.  Before DATA(1) this retransmits the OACK
            // (RFC 2347 §5), not a bare ACK(0).
            send_packet(socket, peer, &retry_packet).await?;
        }
    }
}

/// For RRQ with options the server answers with an OACK and waits for ACK(0)
/// before sending DATA(1) (RFC 2347 §5); without options it starts directly.
async fn send_oack_or_start(
    socket: &UdpSocket,
    peer: SocketAddr,
    negotiated: &Negotiated,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    if negotiated.oack.is_empty() {
        return Ok(());
    }
    let packets = [oack_packet(&negotiated.oack)];
    send_window_with_ack(socket, peer, &packets, 0, negotiated.timeout, shutdown_rx).await
}

fn initial_write_packet(negotiated: &Negotiated) -> Vec<u8> {
    if negotiated.oack.is_empty() {
        ack_packet(0).to_vec()
    } else {
        oack_packet(&negotiated.oack)
    }
}

/// Sends one window (or a single OACK) and waits for the ACK of `block`,
/// retransmitting the whole window on timeout or stale ACK (RFC 7440).
async fn send_window_with_ack(
    socket: &UdpSocket,
    peer: SocketAddr,
    packets: &[Vec<u8>],
    block: u16,
    timeout_duration: Duration,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), TransferError> {
    let mut retries = 0;
    let mut received = [0u8; MAX_PACKET_SIZE];
    loop {
        for packet in packets {
            logging::event("tftp.protocol", "tftp.packet.send")
                .field("bytes", packet.len())
                .debug();
            send_packet(socket, peer, packet).await?;
        }
        if let Some(size) =
            recv_packet(socket, peer, &mut received, timeout_duration, shutdown_rx).await?
        {
            logging::event("tftp.protocol", "tftp.packet.receive")
                .maybe_field("local", socket.local_addr().ok())
                .field("block", block)
                .field("bytes", size)
                .debug();
            match decode_transfer_packet(&received[..size])? {
                TransferPacket::Ack(ack) if ack == block => return Ok(()),
                // Stale/duplicate ACK: retransmit the whole window below, but
                // count the attempt so a misbehaving peer cannot loop forever.
                TransferPacket::Ack(_) => {}
                TransferPacket::Error { code, message } => {
                    return Err(TransferError::Peer(format!(
                        "peer sent ERROR {code}: {message}"
                    )))
                }
                TransferPacket::Data { .. } => {
                    return Err(TransferError::illegal("expected ACK packet"))
                }
            }
        }
        retries += 1;
        if retries > TFTP_MAX_RETRIES {
            return Err(TransferError::Timeout(format!(
                "timed out waiting for ACK {block}"
            )));
        }
    }
}

/// Receives the next packet from the transfer's peer.  Returns `Ok(None)` on
/// timeout so callers can apply their own retransmit policy.  Packets from
/// any other source get ERROR 5 (Unknown transfer ID) per RFC 1350.
async fn recv_packet(
    socket: &UdpSocket,
    peer: SocketAddr,
    packet: &mut [u8],
    timeout_duration: Duration,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<Option<usize>, TransferError> {
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                debug_assert!(changed.is_err() || *shutdown_rx.borrow());
                return Err(TransferError::Shutdown);
            }
            result = timeout(timeout_duration, socket.recv_from(packet)) => match result {
                Ok(Ok((size, sender))) if sender == peer => return Ok(Some(size)),
                Ok(Ok((_, sender))) => { let _ = send_error(socket, sender, 5, "Unknown transfer ID").await; }
                Ok(Err(error)) => {
                    return Err(TransferError::undefined(format!("socket receive failed: {error}")))
                }
                Err(_) => return Ok(None),
            }
        }
    }
}

async fn send_packet(
    socket: &UdpSocket,
    peer: SocketAddr,
    packet: &[u8],
) -> Result<(), TransferError> {
    socket
        .send_to(packet, peer)
        .await
        .map(|_| ())
        .map_err(|error| TransferError::undefined(format!("failed to send TFTP packet: {error}")))
}

/// Fire-and-forget ERROR packet; failures are irrelevant because the transfer
/// is already terminating.
pub(super) async fn send_error(
    socket: &UdpSocket,
    peer: SocketAddr,
    code: u16,
    message: &str,
) -> Result<usize, io::Error> {
    socket
        .send_to(&super::packet::error_packet(code, message), peer)
        .await
}

fn emit_transfer_start(context: &SessionContext, id: &str) {
    if let Some(event) = context.shared.transfer_event(id, false, None) {
        (context.emitter)(event);
    }
}

/// Maps a client-supplied filename onto the shared root, rejecting any
/// traversal outside it with RFC 1350 error 2 (Access violation).
async fn secure_transfer_path(root: &Path, requested: &Path) -> Result<PathBuf, TransferError> {
    let candidate = root.join(secure_transfer_path_components(requested)?);
    let canonical = match tokio::fs::canonicalize(&candidate).await {
        Ok(path) => path,
        Err(_) => match candidate.parent() {
            Some(parent) => {
                let parent = tokio::fs::canonicalize(parent)
                    .await
                    .map_err(|_| TransferError::access_violation("Access violation"))?;
                candidate
                    .file_name()
                    .map(|name| parent.join(name))
                    .ok_or_else(|| TransferError::access_violation("Access violation"))?
            }
            None => return Err(TransferError::access_violation("Access violation")),
        },
    };
    if canonical.starts_with(root) {
        Ok(canonical)
    } else {
        Err(TransferError::access_violation("Access violation"))
    }
}

fn secure_transfer_path_components(requested: &Path) -> Result<PathBuf, TransferError> {
    let relative = requested
        .strip_prefix("/")
        .or_else(|_| requested.strip_prefix("./"))
        .unwrap_or(requested);
    let mut clean = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => clean.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(TransferError::access_violation("Access violation"))
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(TransferError::access_violation("Access violation"));
    }
    Ok(clean)
}

/// Picks the local interface address facing the peer when the server socket
/// is bound to an unspecified address.  Uses a connected probe socket; no
/// traffic is actually sent.
fn discover_local_ip(peer: SocketAddr) -> io::Result<IpAddr> {
    let probe = std::net::UdpSocket::bind(SocketAddr::new(
        match peer.ip() {
            IpAddr::V4(_) => IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
        },
        0,
    ))?;
    probe.connect(peer)?;
    Ok(probe.local_addr()?.ip())
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc, time::Duration};

    use tokio::{net::UdpSocket, sync::watch, time::timeout};

    use super::{
        super::packet::data_packet, build_data_window, recv_packet, send_window_with_ack,
        PendingUpload,
    };

    fn block_of(packet: &[u8]) -> u16 {
        u16::from_be_bytes([packet[2], packet[3]])
    }

    #[test]
    fn data_windows_number_blocks_consecutively_across_windows() {
        // Regression: send_file used to increment the block counter once per
        // window in addition to the per-block increment, skipping every other
        // block and breaking any transfer larger than one window.
        let source = vec![7u8; 512 * 2 + 100];
        let first = build_data_window(&source, 0, 1, 512, 2);
        assert_eq!(
            first
                .packets
                .iter()
                .map(|p| block_of(p))
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(!first.is_final);
        assert_eq!(first.offset, 1024);
        assert_eq!(first.payload_bytes, 1024);
        assert_eq!(first.next_block, 3);
        let second = build_data_window(&source, first.offset, first.next_block, 512, 2);
        assert_eq!(second.packets.len(), 1);
        assert_eq!(block_of(&second.packets[0]), 3);
        assert_eq!(second.last_block, 3);
        assert_eq!(second.payload_bytes, 100);
        assert!(second.is_final);
    }

    #[test]
    fn exact_multiple_of_block_size_ends_with_empty_final_block() {
        // RFC 1350: a file whose length is an exact multiple of the block size
        // terminates with a zero-length DATA packet.
        let source = vec![1u8; 1024];
        let first = build_data_window(&source, 0, 1, 512, 1);
        assert!(!first.is_final);
        assert_eq!(first.next_block, 2);
        let second = build_data_window(&source, first.offset, first.next_block, 512, 1);
        assert!(!second.is_final);
        assert_eq!(block_of(&second.packets[0]), 2);
        let third = build_data_window(&source, second.offset, second.next_block, 512, 1);
        assert!(third.is_final);
        assert_eq!(third.payload_bytes, 0);
        assert_eq!(block_of(&third.packets[0]), 3);
    }

    #[test]
    fn block_numbers_wrap_at_u16_boundary() {
        let source = vec![1u8; 1024];
        let window = build_data_window(&source, 0, u16::MAX, 512, 2);
        assert_eq!(
            window
                .packets
                .iter()
                .map(|p| block_of(p))
                .collect::<Vec<_>>(),
            vec![u16::MAX, 0]
        );
        assert_eq!(window.next_block, 1);
    }

    #[test]
    fn pending_upload_commits_atomically_and_cleans_abandoned_data() {
        let directory = std::env::temp_dir().join(format!("xterm-tftp-{}", crate::ids::new_id()));
        fs::create_dir(&directory).unwrap();
        let target = directory.join("firmware.bin");
        fs::write(&target, b"old").unwrap();

        let committed = PendingUpload::new(&target, "committed");
        fs::write(&committed.path, b"new").unwrap();
        committed.commit(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");

        let abandoned_path = {
            let abandoned = PendingUpload::new(&target, "abandoned");
            fs::write(&abandoned.path, b"partial").unwrap();
            abandoned.path.clone()
        };
        assert!(!abandoned_path.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn window_sender_waits_for_the_last_block_ack() {
        let server = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_addr = client.local_addr().unwrap();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let sender = server.clone();
        let task = tokio::spawn(async move {
            let window = vec![data_packet(1, b"a"), data_packet(2, b"b")];
            send_window_with_ack(
                &sender,
                client_addr,
                &window,
                2,
                Duration::from_secs(1),
                &mut shutdown_rx,
            )
            .await
        });
        let mut packet = [0u8; 516];
        let (_, source) = timeout(Duration::from_secs(1), client.recv_from(&mut packet))
            .await
            .unwrap()
            .unwrap();
        let _ = timeout(Duration::from_secs(1), client.recv_from(&mut packet))
            .await
            .unwrap()
            .unwrap();
        client.send_to(&[0, 4, 0, 2], source).await.unwrap();
        assert!(task.await.unwrap().is_ok());
        drop(shutdown_tx);
    }

    #[tokio::test]
    async fn unknown_transfer_id_receives_error_five() {
        let server = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let intruder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let client_addr = client.local_addr().unwrap();
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let receiver = server.clone();
        let task = tokio::spawn(async move {
            let mut packet = [0u8; 516];
            recv_packet(
                &receiver,
                client_addr,
                &mut packet,
                Duration::from_secs(1),
                &mut shutdown_rx,
            )
            .await
        });
        intruder.send_to(&[0, 4, 0, 1], server_addr).await.unwrap();
        let mut error = [0u8; 516];
        let (size, _) = timeout(Duration::from_secs(1), intruder.recv_from(&mut error))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&error[..4], &[0, 5, 0, 5]);
        client.send_to(&[0, 4, 0, 1], server_addr).await.unwrap();
        assert_eq!(task.await.unwrap().unwrap(), Some(4));
        assert!(size >= 5);
    }
}
