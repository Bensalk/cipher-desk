//! Host-side clipboard and file transfer for the Share screen method.
//!
//! The viewer (client) side already shares plain text and files; this is the
//! matching piece on the computer being shared, so copy and paste and file
//! copy work both ways:
//! - Text copied here can be pasted on the viewer, and vice versa (plain text
//!   only, CF_UNICODETEXT, with a size cap).
//! - Files the viewer sends arrive in a "Received" folder; files offered here
//!   with [`ClipHandle::offer_files`] are downloaded by the viewer.
//! - Copy and paste of files: files copied in Explorer here
//!   (while a viewer is connected) are sent to the viewer and put on its
//!   clipboard; files the viewer copies arrive here and are put on this
//!   computer's clipboard, so Ctrl+V in Explorer pastes them.
//!
//! This is a `unsafe` module like [`crate::win`]: the Windows clipboard has no
//! safe wrapper. Every call says why it is sound. File reads and writes use
//! plain `std::fs`.
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend, CliprdrBackendFactory};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardFormatName, ClipboardGeneralCapabilityFlags,
    FileContentsRequest, FileContentsResponse, FileDescriptor, FormatDataRequest,
    FormatDataResponse, LockDataId, OwnedFormatDataResponse,
};
use ironrdp_server::{CliprdrServerFactory, ServerEvent, ServerEventSender};
use rdpv_core::xfer;
use tokio::sync::mpsc::UnboundedSender;

use crate::HostEvent;

/// How often the host clipboard is checked for a change, to tell the viewer.
const POLL_INTERVAL: Duration = Duration::from_millis(400);
/// Wait after a clipboard change before reading it (the copying program may
/// still be adding formats).
const SETTLE: Duration = Duration::from_millis(150);

/// Supplies a clipboard backend to the server and holds the shared state the
/// backend and the app both use.
pub struct HostClipboard {
    inner: Arc<Inner>,
}

struct Inner {
    sender: Mutex<Option<UnboundedSender<ServerEvent>>>,
    events: std::sync::mpsc::Sender<HostEvent>,
    /// Where files the viewer sends are written.
    receive_dir: PathBuf,
    /// Same path as text, for the cliprdr temporary directory.
    receive_dir_str: String,
    /// Files currently offered to the viewer, indexed as advertised.
    outgoing: Mutex<Vec<xfer::Outgoing>>,
    /// Files and folders arriving from the viewer, in pieces.
    receiving: Mutex<Option<xfer::Receiver>>,
    /// Largest text accepted from the viewer, in bytes of UTF-16.
    max_bytes: usize,
    /// Clipboard sequence number right after we put received files on it, so
    /// the poller does not send them straight back.
    own_seq: AtomicU32,
    /// Milliseconds since UNIX epoch when we last wrote CF_HDROP, so the
    /// poller suppresses cascade echoes for a few seconds afterwards even
    /// when the sequence number has advanced past `own_seq`.
    own_write_ms: AtomicU64,
}

impl HostClipboard {
    /// `max_bytes` caps text accepted from the viewer (bytes of UTF-16).
    /// `receive_dir` is where files the viewer sends are written.
    pub fn new(
        events: std::sync::mpsc::Sender<HostEvent>,
        receive_dir: PathBuf,
        max_bytes: usize,
    ) -> Self {
        let receive_dir_str = receive_dir.to_string_lossy().into_owned();
        Self {
            inner: Arc::new(Inner {
                sender: Mutex::new(None),
                events,
                receive_dir,
                receive_dir_str,
                outgoing: Mutex::new(Vec::new()),
                receiving: Mutex::new(None),
                max_bytes,
                own_seq: AtomicU32::new(0),
                own_write_ms: AtomicU64::new(0),
            }),
        }
    }

    /// A handle the app keeps to offer files to the viewer.
    pub fn handle(&self) -> ClipHandle {
        ClipHandle {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Lets the app offer files to the connected viewer.
#[derive(Clone)]
pub struct ClipHandle {
    inner: Arc<Inner>,
}

impl ClipHandle {
    /// Offers these local files to the viewer, which downloads them. Files that
    /// are missing or too large are skipped; if none qualify, nothing is sent.
    pub fn offer_files(&self, paths: Vec<PathBuf>) {
        let files = xfer::offer(&paths);
        if files.is_empty() {
            return;
        }
        let descriptors: Vec<FileDescriptor> = files.iter().map(|o| o.descriptor.clone()).collect();
        if let Ok(mut out) = self.inner.outgoing.lock() {
            *out = files;
        }
        self.inner
            .send(ClipboardMessage::SendInitiateFileCopy(descriptors));
    }
}

impl Inner {
    fn send(&self, msg: ClipboardMessage) {
        if let Ok(guard) = self.sender.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(ServerEvent::Clipboard(msg));
            }
        }
    }

    /// Carries out a step of receiving: ask for the next piece, or, when
    /// everything is here, put it on this computer's clipboard (Ctrl+V in
    /// Explorer pastes it) and tell the app.
    fn receive_step(&self, step: xfer::Step) {
        match step {
            xfer::Step::Request(req) => self.send(ClipboardMessage::SendFileContentsRequest(req)),
            xfer::Step::Done(top_level) => {
                tracing::info!(count = top_level.len(), "host clip: file receive done");
                if let Ok(mut r) = self.receiving.lock() {
                    *r = None;
                }
                let done: Vec<PathBuf> = top_level.into_iter().filter(|p| p.exists()).collect();
                if done.is_empty() {
                    tracing::warn!("host clip: no received files exist on disk");
                    return;
                }
                match winclip::write_files(&done) {
                    Ok(()) => {
                        tracing::info!(count = done.len(), "host clip: wrote CF_HDROP to clipboard");
                        self.own_seq
                            .store(winclip::sequence_number(), Ordering::Relaxed);
                        let now_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        self.own_write_ms.store(now_ms, Ordering::Relaxed);
                    }
                    Err(ref e) => {
                        tracing::error!(?e, "host clip: write_files (CF_HDROP) FAILED");
                    }
                }
                for p in done {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let _ = self.events.send(HostEvent::FileReceived { name });
                }
            }
        }
    }
}

impl ServerEventSender for HostClipboard {
    fn set_sender(&mut self, sender: UnboundedSender<ServerEvent>) {
        if let Ok(mut s) = self.inner.sender.lock() {
            *s = Some(sender);
        }
    }
}

impl CliprdrBackendFactory for HostClipboard {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        Box::new(Backend {
            inner: Arc::clone(&self.inner),
            poller: None,
        })
    }
}

impl CliprdrServerFactory for HostClipboard {}

/// Deletes files left in `dir` from previous sessions so the Received folder
/// does not accumulate duplicates across connections. Silently ignores errors.
fn cleanup_received_dir(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, "host clip: cleaned up received files from previous session");
    }
}

/// Sends the files copied here (Explorer) to the viewer, if the clipboard
/// holds files. Returns whether it did.
fn offer_copied_files(inner: &Inner) -> bool {
    let Some(paths) = winclip::read_files() else {
        tracing::info!("host clip: clipboard changed but no CF_HDROP files found");
        return false;
    };
    tracing::info!(count = paths.len(), ?paths, "host clip: CF_HDROP files found on clipboard");
    // Files we received from the viewer and put there ourselves: not a new
    // copy (sending them back would duplicate them on the viewer).
    if paths.iter().all(|p| p.starts_with(&inner.receive_dir)) {
        tracing::info!("host clip: all clipboard files are own-received, skipping echo");
        return true;
    }
    let files = xfer::offer(&paths);
    if files.is_empty() {
        tracing::info!(
            count = paths.len(),
            "host clip: clipboard has files but none could be offered (too large or missing?)"
        );
        return false;
    }
    let descriptors: Vec<FileDescriptor> = files.iter().map(|o| o.descriptor.clone()).collect();
    tracing::info!(
        count = descriptors.len(),
        "offering host clipboard files to viewer"
    );
    if let Ok(mut out) = inner.outgoing.lock() {
        *out = files;
    }
    inner.send(ClipboardMessage::SendInitiateFileCopy(descriptors));
    true
}

/// Advertises to the viewer whether this computer's clipboard holds text.
fn advertise(inner: &Inner) {
    let formats = if winclip::has_text() {
        vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
    } else {
        Vec::new()
    };
    inner.send(ClipboardMessage::SendInitiateCopy(formats));
}

/// Watches this computer's clipboard and re-advertises when it changes, so a
/// copy made here while a viewer is connected becomes pasteable there.
struct Poller {
    running: Arc<AtomicBool>,
    _join: Option<JoinHandle<()>>,
}

fn spawn_poller(inner: Arc<Inner>) -> Poller {
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    let join = std::thread::Builder::new()
        .name("cipher-desk-clip".into())
        .spawn(move || {
            let mut prev = winclip::sequence_number();
            while flag.load(Ordering::Relaxed) {
                std::thread::sleep(POLL_INTERVAL);
                if !flag.load(Ordering::Relaxed) {
                    break;
                }
                let mut seq = winclip::sequence_number();
                if seq != prev {
                    // Explorer puts several formats on the clipboard one after
                    // another: let it finish before reading the file list.
                    std::thread::sleep(SETTLE);
                    seq = winclip::sequence_number();
                    prev = seq;
                    // Files we just received and placed there: not a new copy.
                    // Also suppress cascade echoes for 5 s after writing CF_HDROP
                    // (Windows Clipboard History and the CLIPRDR round-trip can
                    // trigger several additional sequence bumps that would
                    // otherwise overwrite CF_HDROP with the viewer's format).
                    let is_own_echo = seq == inner.own_seq.load(Ordering::Relaxed);
                    let write_ms = inner.own_write_ms.load(Ordering::Relaxed);
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let in_quiet_window = write_ms > 0 && now_ms.saturating_sub(write_ms) < 5_000;
                    if is_own_echo || in_quiet_window {
                        tracing::info!(
                            is_own_echo,
                            in_quiet_window,
                            "host clip: clipboard change is our own echo, skipping"
                        );
                        continue;
                    }
                    tracing::info!(seq, "host clip: clipboard sequence changed, checking for files");
                    // Files copied here go to the viewer; otherwise text (or
                    // nothing). Files already on the clipboard when the viewer
                    // connected are not sent: only a new copy is.
                    if !offer_copied_files(&inner) {
                        advertise(&inner);
                    }
                }
            }
        })
        .ok();
    if join.is_none() {
        // The thread could not start; nothing to stop later.
        running.store(false, Ordering::Relaxed);
    }
    Poller {
        running,
        _join: join,
    }
}

/// One connection's clipboard bridge. Created per viewer; dropped when they
/// leave, which stops the poller.
struct Backend {
    inner: Arc<Inner>,
    poller: Option<Poller>,
}

impl core::fmt::Debug for Backend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HostClipboardBackend")
    }
}

ironrdp_core::impl_as_any!(Backend);

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(p) = self.poller.take() {
            p.running.store(false, Ordering::Relaxed);
            // Not joined: the thread may be mid-sleep and will exit on its own.
        }
    }
}

/// Whether a format in the list is the file-list format ("FileGroupDescriptorW").
fn is_file_list(f: &ClipboardFormat) -> bool {
    f.name().map(|n| n.value()) == Some(ClipboardFormatName::FILE_LIST.value())
}

impl CliprdrBackend for Backend {
    fn temporary_directory(&self) -> &str {
        &self.inner.receive_dir_str
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        // Plain text plus stream-based file copy.
        xfer::file_caps()
    }

    fn on_ready(&mut self) {
        tracing::info!("host clip: clipboard channel ready, starting poller");
        // Clean up leftover files from previous sessions so the Received
        // folder doesn't accumulate duplicates across connections.
        cleanup_received_dir(&self.inner.receive_dir);
        if self.poller.is_none() {
            self.poller = Some(spawn_poller(Arc::clone(&self.inner)));
        }
        advertise(&self.inner);
    }

    fn on_request_format_list(&mut self) {
        advertise(&self.inner);
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        // The viewer copied something: pull files if offered, else text.
        if let Some(file_format) = formats.iter().find(|f| is_file_list(f)) {
            self.inner
                .send(ClipboardMessage::SendInitiatePaste(file_format.id()));
            return;
        }
        let has_text = formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT);
        if has_text {
            self.inner.send(ClipboardMessage::SendInitiatePaste(
                ClipboardFormatId::CF_UNICODETEXT,
            ));
        }
    }

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], clip_data_id: Option<u32>) {
        // The viewer is sending files and folders: receive them in pieces.
        tracing::info!(count = files.len(), ?clip_data_id, "host clip: got file list from viewer, starting receive");
        let items = files
            .iter()
            .map(xfer::RemoteItem::from_descriptor)
            .collect();
        let step = {
            let Ok(mut r) = self.inner.receiving.lock() else {
                tracing::error!("host clip: receiving mutex poisoned");
                return;
            };
            // A new copy replaces one still arriving.
            *r = None;
            let (rx, step) = xfer::Receiver::start(items, clip_data_id, &self.inner.receive_dir);
            tracing::info!(file_count = rx.file_count(), total_bytes = rx.total_bytes, "host clip: receiver started");
            *r = Some(rx);
            step
        };
        self.inner.receive_step(step);
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        // The viewer wants this computer's clipboard text.
        let response = if request.format == ClipboardFormatId::CF_UNICODETEXT {
            match winclip::read_text(self.inner.max_bytes) {
                Some(text) => OwnedFormatDataResponse::new_unicode_string(&text),
                None => OwnedFormatDataResponse::new_error(),
            }
        } else {
            OwnedFormatDataResponse::new_error()
        };
        self.inner.send(ClipboardMessage::SendFormatData(response));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        // Text arrived from the viewer: put it on this computer's clipboard.
        if response.is_error() {
            return;
        }
        let data = response.data();
        if data.len() > self.inner.max_bytes + 2 {
            return;
        }
        if let Some(text) = decode_unicode_text(data) {
            if !text.is_empty() {
                let _ = winclip::write_text(&text);
            }
        }
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        // The viewer is downloading (a piece of) a file we offered.
        tracing::info!(stream_id = request.stream_id, index = request.index, "host clip: viewer requesting file contents");
        let answer = self
            .inner
            .outgoing
            .lock()
            .ok()
            .and_then(|out| xfer::answer(&out, &request));
        let response = match answer {
            Some(xfer::FileContentsAnswer::Size(size)) => {
                tracing::info!(stream_id = request.stream_id, size, "host clip: answering SIZE request");
                FileContentsResponse::new_size_response(request.stream_id, size)
            }
            Some(xfer::FileContentsAnswer::Data(bytes)) => {
                tracing::info!(stream_id = request.stream_id, len = bytes.len(), "host clip: answering DATA request");
                FileContentsResponse::new_data_response(request.stream_id, bytes)
            }
            None => {
                tracing::error!(stream_id = request.stream_id, index = request.index, "host clip: no answer for file request (outgoing empty?)");
                FileContentsResponse::new_error(request.stream_id)
            }
        };
        self.inner
            .send(ClipboardMessage::SendFileContentsResponse(response));
    }

    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        // A piece of a file we asked the viewer for.
        let data = (!response.is_error()).then(|| response.data());
        let step = match self.inner.receiving.lock() {
            Ok(mut r) => r
                .as_mut()
                .and_then(|rx| rx.on_data(response.stream_id(), data)),
            Err(_) => None,
        };
        if let Some(step) = step {
            self.inner.receive_step(step);
        }
    }

    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}

/// Decodes CF_UNICODETEXT bytes (UTF-16LE, usually NUL-terminated) to a string.
fn decode_unicode_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// The Windows clipboard, for plain text only. All access opens and closes the
/// clipboard around the shortest possible critical section.
mod winclip {
    use anyhow::Context as _;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows::Win32::UI::Shell::{ILCombine, ILFree, SHGetPathFromIDListW};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;

    const CF_UNICODETEXT: u32 = 13;
    /// A list of files (what Explorer puts there on Ctrl+C).
    const CF_HDROP: u32 = 15;
    /// Most files read from one copy (a guard against huge selections).
    const MAX_FILES: usize = 1000;

    /// Opens the clipboard and closes it on drop.
    struct Guard;

    impl Guard {
        fn open() -> anyhow::Result<Self> {
            // Another program (Explorer, clipboard manager, Windows Clipboard
            // History, antivirus) may hold the clipboard for an extended period.
            // Retry up to 4 seconds (80 × 50 ms) before giving up.  The old
            // 500 ms window was not enough — real-world logs showed the lock
            // being held for >500 ms, causing "Access is denied." (0x80070005).
            let start = std::time::Instant::now();
            let mut last = None;
            for attempt in 0..80u32 {
                // SAFETY: opens the clipboard with no owner window; Drop closes it.
                match unsafe { OpenClipboard(None) } {
                    Ok(()) => {
                        if attempt > 0 {
                            tracing::debug!(
                                attempt,
                                elapsed_ms = start.elapsed().as_millis(),
                                "open clipboard succeeded after retries"
                            );
                        }
                        return Ok(Self);
                    }
                    Err(e) => last = Some(e),
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(anyhow::anyhow!(
                "open clipboard: {last:?} (gave up after {} ms)",
                start.elapsed().as_millis()
            ))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: closes the clipboard opened in `open`.
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    /// A counter Windows bumps on every clipboard change; no open needed.
    pub fn sequence_number() -> u32 {
        // SAFETY: reads a global counter, no state kept.
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Whether the clipboard currently holds plain text.
    pub fn has_text() -> bool {
        // SAFETY: queries format availability only.
        unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() }
    }

    /// Reads the clipboard's text, or `None` if there is none, it is too large,
    /// or it cannot be read. `max_bytes` is the UTF-16 size limit.
    pub fn read_text(max_bytes: usize) -> Option<String> {
        if !has_text() {
            return None;
        }
        let _guard = Guard::open().ok()?;
        // SAFETY: the handle is owned by the clipboard and only read while the
        // clipboard is open (held by `_guard`); the lock is released before it
        // closes. `units` is bounded by the reported global size.
        unsafe {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let hg = HGLOBAL(handle.0);
            let size = GlobalSize(hg);
            if size == 0 || size > max_bytes + 2 {
                return None;
            }
            let ptr = GlobalLock(hg) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let units = size / 2;
            let mut buf: Vec<u16> = core::slice::from_raw_parts(ptr, units).to_vec();
            let _ = GlobalUnlock(hg);
            if let Some(end) = buf.iter().position(|&u| u == 0) {
                buf.truncate(end);
            }
            String::from_utf16(&buf).ok()
        }
    }

    /// The files on the clipboard (copied in Explorer), or `None` if it holds
    /// no files.
    ///
    /// Two strategies are tried in order:
    ///
    /// 1. **CF_HDROP** (format 15) — the classic drop-files format.  On older
    ///    Windows versions Explorer sets this directly; on Windows 11 it may be
    ///    absent or only present as delayed rendering.
    ///
    /// 2. **CFSTR_SHELLIDLIST** ("Shell IDList Array") — always present when
    ///    files are copied in Explorer regardless of OS version.  Each item is a
    ///    PIDL that `SHGetPathFromIDListW` converts to a filesystem path.
    ///
    /// If CF_HDROP is absent we also log every format that *is* present so the
    /// log tells us exactly what Explorer put on the clipboard.
    pub fn read_files() -> Option<Vec<std::path::PathBuf>> {
        // Keep the clipboard open for the duration; both strategies need it.
        let _guard = Guard::open().ok()?;

        // --- Strategy 1: CF_HDROP -------------------------------------------
        // SAFETY: clipboard is open; handle, global memory, and lock follow the
        // standard MSDN "Clipboard Operations" pattern.
        let hdrop = unsafe {
            GetClipboardData(CF_HDROP).ok().and_then(|handle| {
                if handle.is_invalid() {
                    return None;
                }
                let hg = HGLOBAL(handle.0);
                let size = GlobalSize(hg);
                let ptr = GlobalLock(hg) as *const u8;
                if ptr.is_null() || size == 0 {
                    return None;
                }
                let v = core::slice::from_raw_parts(ptr, size).to_vec();
                let _ = GlobalUnlock(hg);
                Some(v)
            })
        };
        if let Some(bytes) = hdrop {
            let files = rdpv_core::xfer::parse_dropfiles(&bytes, MAX_FILES);
            if !files.is_empty() {
                return Some(files);
            }
        }

        // --- Diagnostic: log every format that is on the clipboard -----------
        // This runs only when CF_HDROP is absent, so it costs nothing in the
        // common (text-only) case.
        // SAFETY: clipboard is open; EnumClipboardFormats is a read-only walk.
        let present: Vec<u32> = unsafe {
            let mut fmt = 0u32;
            let mut v = Vec::new();
            loop {
                fmt = EnumClipboardFormats(fmt);
                if fmt == 0 {
                    break;
                }
                v.push(fmt);
            }
            v
        };
        tracing::info!(
            "host clip: CF_HDROP absent; formats on clipboard: {:?}",
            present
        );

        // --- Strategy 2: CFSTR_SHELLIDLIST -----------------------------------
        // Explorer always places a Shell IDList Array (CIDA) on the clipboard
        // when files are copied, even when CF_HDROP is not present.
        // SAFETY: RegisterClipboardFormatW is a one-time idempotent registration
        // using a literal name; the returned ID is used only while the clipboard
        // is still open.
        let cf_shell_idlist =
            unsafe { RegisterClipboardFormatW(windows::core::w!("Shell IDList Array")) };
        if cf_shell_idlist == 0 || !present.contains(&cf_shell_idlist) {
            return None;
        }

        // SAFETY: clipboard is still open; we lock the global memory, walk the
        // CIDA structure to build absolute PIDLs, convert each PIDL to a path,
        // and unlock before returning.
        unsafe {
            let handle = GetClipboardData(cf_shell_idlist).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let hg = HGLOBAL(handle.0);
            let size = GlobalSize(hg);
            if size < 8 {
                return None;
            }
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() {
                return None;
            }

            // CIDA layout: { cidl: u32, aoffset[cidl+1]: u32, raw_pidl_data }
            // aoffset[0] = parent folder PIDL; aoffset[1..=cidl] = item PIDLs.
            let cidl = {
                let b = core::slice::from_raw_parts(ptr, 4);
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
            };
            if cidl == 0 || cidl > MAX_FILES || size < 4 + (cidl + 1) * 4 {
                let _ = GlobalUnlock(hg);
                return None;
            }

            let parent_offset = {
                let b = core::slice::from_raw_parts(ptr.add(4), 4);
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
            };
            if parent_offset >= size {
                let _ = GlobalUnlock(hg);
                return None;
            }

            let mut paths = Vec::new();
            for i in 1..=cidl {
                let off_idx = 4 + i * 4;
                let item_off = {
                    let b = core::slice::from_raw_parts(ptr.add(off_idx), 4);
                    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
                };
                if item_off >= size {
                    continue;
                }

                // Combine parent + relative item PIDL into an absolute PIDL.
                let parent_pidl = ptr.add(parent_offset) as *const ITEMIDLIST;
                let item_pidl = ptr.add(item_off) as *const ITEMIDLIST;
                // ILCombine takes Option<*const ITEMIDLIST> in windows 0.62.
                let abs_pidl: *mut ITEMIDLIST =
                    ILCombine(Some(parent_pidl), Some(item_pidl));
                if abs_pidl.is_null() {
                    continue;
                }

                // Convert the absolute PIDL to a filesystem path.
                // SHGetPathFromIDListW takes &mut [u16; 260] (MAX_PATH) in windows 0.62.
                let mut path_buf = [0u16; 260];
                let ok =
                    SHGetPathFromIDListW(abs_pidl as *const _, &mut path_buf);
                // ILFree takes Option<*const ITEMIDLIST>; abs_pidl is *mut, cast needed.
                ILFree(Some(abs_pidl as *const _));

                if ok.as_bool() {
                    let len = path_buf
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(path_buf.len());
                    if let Ok(s) = String::from_utf16(&path_buf[..len]) {
                        if !s.is_empty() {
                            paths.push(std::path::PathBuf::from(s));
                        }
                    }
                }
            }

            let _ = GlobalUnlock(hg);
            (!paths.is_empty()).then_some(paths)
        }
    }

    /// Replaces the clipboard's contents with these files, so Ctrl+V in
    /// Explorer copies them.
    pub fn write_files(paths: &[std::path::PathBuf]) -> anyhow::Result<()> {
        let bytes = rdpv_core::xfer::build_dropfiles(paths);
        let _guard = Guard::open()?;
        // SAFETY: as in `write_text`: fill a moveable block while locked, then
        // hand it to the clipboard, which owns it on success.
        unsafe {
            EmptyClipboard().context("empty clipboard")?;
            let hg =
                GlobalAlloc(GMEM_MOVEABLE, bytes.len()).context("allocate clipboard memory")?;
            let dst = GlobalLock(hg) as *mut u8;
            if dst.is_null() {
                anyhow::bail!("lock clipboard memory");
            }
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            let _ = GlobalUnlock(hg);
            SetClipboardData(CF_HDROP, Some(HANDLE(hg.0))).context("set clipboard data")?;
        }
        Ok(())
    }

    /// Replaces the clipboard's contents with `text`.
    pub fn write_text(text: &str) -> anyhow::Result<()> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0); // NUL terminator, as CF_UNICODETEXT expects.
        let bytes = units.len().saturating_mul(2);
        let _guard = Guard::open()?;
        // SAFETY: standard SetClipboardData flow. The moveable block is filled
        // while locked, unlocked, then handed to the clipboard, which takes
        // ownership on success (so it is not freed here).
        unsafe {
            EmptyClipboard().context("empty clipboard")?;
            let hg = GlobalAlloc(GMEM_MOVEABLE, bytes).context("allocate clipboard memory")?;
            let dst = GlobalLock(hg) as *mut u16;
            if dst.is_null() {
                anyhow::bail!("lock clipboard memory");
            }
            core::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
            let _ = GlobalUnlock(hg);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hg.0))).context("set clipboard data")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::decode_unicode_text;

    #[test]
    fn decodes_utf16_with_terminator() {
        let mut bytes: Vec<u8> = "héllo 😀"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.extend_from_slice(&[0, 0, 0x41, 0]); // NUL then junk
        assert_eq!(decode_unicode_text(&bytes).as_deref(), Some("héllo 😀"));
        assert_eq!(decode_unicode_text(&[0x41]), None); // odd length
        assert_eq!(decode_unicode_text(&[0x00, 0xD8]), None); // lone surrogate
    }
}
