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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    /// Set to true when we write received files to CF_HDROP; cleared when the
    /// host user copies something new (poller detects a genuine change).
    /// While set, `on_remote_copy` ignores file-list announcements from the
    /// viewer, which prevents IronRDP's CLIPRDR backend from calling
    /// EmptyClipboard + SetClipboardData(49161) and wiping our CF_HDROP entry
    /// before the user can paste.
    hdrop_pending: AtomicBool,
    /// Text from the last capture of this computer's clipboard. On some hosts
    /// another program replaces the clipboard a fraction of a second after a
    /// copy; this lets `on_format_data_request` still serve the copied text.
    last_text: Mutex<Option<String>>,
    /// The file list last offered to the viewer, and when — so a copy of the
    /// same files announced again moments later (Explorer can raise the
    /// clipboard-changed notification more than once for a single Ctrl+C) is
    /// not sent to the viewer a second time.
    last_offered: Mutex<(Vec<PathBuf>, u64)>,
    /// When `last_text` was captured (ms since the UNIX epoch; 0 = never).
    /// While recent, the poller keeps telling the viewer text is available
    /// even if the clipboard itself no longer holds it.
    text_cached_ms: AtomicU64,
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
                hdrop_pending: AtomicBool::new(false),
                last_text: Mutex::new(None),
                text_cached_ms: AtomicU64::new(0),
                last_offered: Mutex::new((Vec::new(), 0)),
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
        let files = with_file_rights(|| xfer::offer(&paths)).unwrap_or_default();
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
                        tracing::info!(
                            count = done.len(),
                            "host clip: wrote CF_HDROP to clipboard"
                        );
                        self.own_seq
                            .store(winclip::sequence_number(), Ordering::Relaxed);
                        let now_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        self.own_write_ms.store(now_ms, Ordering::Relaxed);
                        // Block on_remote_copy from calling SendInitiatePaste(49161)
                        // until the user copies something new. That CLIPRDR round-trip
                        // calls EmptyClipboard and wipes our CF_HDROP entry.
                        self.hdrop_pending.store(true, Ordering::Relaxed);
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
            text_poller: None,
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
        if path.is_file() && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(
            removed,
            "host clip: cleaned up received files from previous session"
        );
    }
}

/// Asks someone else for the files copied here, when this process cannot see
/// them itself (see [`set_copied_files_source`]).
type CopiedFilesSource = Box<dyn Fn() -> Option<Vec<PathBuf>> + Send + Sync>;

static COPIED_FILES_SOURCE: std::sync::OnceLock<CopiedFilesSource> = std::sync::OnceLock::new();

/// Read and send files as the signed-in user, not as this process.
static READ_AS_SIGNED_IN_USER: AtomicBool = AtomicBool::new(false);

/// For the Windows service host, which runs as the system account: Windows
/// hides what the signed-in person copies in Explorer from that account, so
/// a helper running as the person reads it and `source` asks that helper.
/// Called once; later calls are ignored.
pub fn set_copied_files_source(source: CopiedFilesSource) {
    let _ = COPIED_FILES_SOURCE.set(source);
}

/// For the Windows service host: open the files the viewer downloads with
/// the rights of the person signed in at this computer, never with the
/// system account's (which could read files that person may not). With
/// nobody signed in, no files are sent.
pub fn read_files_as_signed_in_user() {
    READ_AS_SIGNED_IN_USER.store(true, Ordering::SeqCst);
}

/// The files copied in Explorer in this session (classic or OLE clipboard),
/// or `None`. Used by the helper that runs as the signed-in person.
pub fn copied_files() -> Option<Vec<PathBuf>> {
    winclip::read_files()
}

/// Runs `f` with the file access rights chosen above. `None` if it must not
/// run (the service host with nobody signed in).
fn with_file_rights<T>(f: impl FnOnce() -> T) -> Option<T> {
    if !READ_AS_SIGNED_IN_USER.load(Ordering::SeqCst) {
        return Some(f());
    }
    let Some(_as_user) = winclip::SignedInUser::impersonate() else {
        tracing::warn!("host clip: nobody signed in to read files as; not sending files");
        return None;
    };
    Some(f())
}

/// Milliseconds since the UNIX epoch, for comparing timestamps; never used
/// as a real calendar time.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Sends the files copied here (Explorer) to the viewer, if the clipboard
/// holds files. Returns whether it did (or skipped them on purpose).
///
/// `refresh`: called because a viewer (re)connected or asked for the
/// clipboard, not because something was copied. Files already sent once are
/// then not sent again: the clipboard simply still holds the last copy, and
/// sending it would make the viewer receive the same file again.
fn offer_copied_files(inner: &Inner, refresh: bool) -> bool {
    let from_helper = || {
        let files = COPIED_FILES_SOURCE.get().and_then(|source| source());
        if let Some(f) = &files {
            tracing::info!(
                count = f.len(),
                "host clip: files copied here (from the helper)"
            );
        }
        files
    };
    let Some(paths) = winclip::read_files().or_else(from_helper) else {
        tracing::info!("host clip: clipboard changed but no CF_HDROP files found");
        return false;
    };
    tracing::info!(
        count = paths.len(),
        ?paths,
        "host clip: CF_HDROP files found on clipboard"
    );
    // Files we received from the viewer and put there ourselves: not a new
    // copy (sending them back would duplicate them on the viewer).
    if paths.iter().all(|p| p.starts_with(&inner.receive_dir)) {
        tracing::info!("host clip: all clipboard files are own-received, skipping echo");
        return true;
    }
    // Same files as the last thing we offered, seconds ago: Explorer
    // re-announcing one copy, not a fresh one. Skip it so the viewer does
    // not get it twice for a single Ctrl+C.
    const REOFFER_WINDOW_MS: u64 = 5_000;
    if let Ok(mut last) = inner.last_offered.lock() {
        let now = now_ms();
        if refresh && last.0 == paths {
            tracing::info!(
                count = paths.len(),
                "host clip: same files as already sent; not sending again on reconnect"
            );
            return true;
        }
        if last.0 == paths && now.saturating_sub(last.1) < REOFFER_WINDOW_MS {
            tracing::info!(
                count = paths.len(),
                "host clip: same files offered moments ago, not sending again"
            );
            return true;
        }
        last.0 = paths.clone();
        last.1 = now;
    }
    let files = with_file_rights(|| xfer::offer(&paths)).unwrap_or_default();
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
/// Also caches the text so `on_format_data_request` can serve it even if
/// another program replaces the clipboard before the viewer asks for it.
fn advertise(inner: &Inner) {
    let has_text = winclip::has_text();
    tracing::info!(has_text, "host clip: advertising clipboard state to viewer");
    if has_text {
        if let Ok(mut cache) = inner.last_text.lock() {
            *cache = winclip::read_text(inner.max_bytes);
        }
        inner.text_cached_ms.store(epoch_ms(), Ordering::Relaxed);
    }
    let formats = if has_text {
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
                    if is_own_echo {
                        tracing::info!(
                            is_own_echo,
                            "host clip: clipboard change is our own echo, skipping"
                        );
                        continue;
                    }
                    if in_quiet_window {
                        // We wrote received files recently. Skip file clipboard
                        // announcements to prevent echo, but if the user has
                        // now copied TEXT, still advertise it — text is a
                        // genuine new copy, not an echo of our CF_HDROP write.
                        if winclip::has_text() {
                            tracing::info!(
                                "host clip: in quiet window but text found — advertising text"
                            );
                            inner.hdrop_pending.store(false, Ordering::Relaxed);
                            advertise(&inner);
                        } else {
                            tracing::info!(
                                in_quiet_window,
                                "host clip: clipboard change suppressed by quiet window"
                            );
                        }
                        continue;
                    }
                    // A genuine new copy on the host: clear the flag so the
                    // viewer's format list can be accepted again next time.
                    inner.hdrop_pending.store(false, Ordering::Relaxed);
                    tracing::info!(seq, "host clip: clipboard changed — checking format");
                    // Files copied here go to the viewer; otherwise text (or
                    // nothing). Files already on the clipboard when the viewer
                    // connected are not sent: only a new copy is.
                    if offer_copied_files(&inner, false) {
                        // User copied files: clear the text cache (new clipboard content).
                        if let Ok(mut cache) = inner.last_text.lock() {
                            *cache = None;
                        }
                        inner.text_cached_ms.store(0, Ordering::Relaxed);
                    } else {
                        // No files. The fast watcher captures copied text the
                        // moment it appears, because a program on some hosts
                        // replaces the clipboard a fraction of a second later.
                        let cached_ms = inner.text_cached_ms.load(Ordering::Relaxed);
                        let age_ms = epoch_ms().saturating_sub(cached_ms);
                        let cache_ok = cached_ms > 0
                            && inner.last_text.lock().ok().is_some_and(|g| g.is_some());
                        if cache_ok && age_ms < 2_000 {
                            // Already captured and announced for this copy.
                            tracing::info!(age_ms, "host clip: text already captured");
                            continue;
                        }
                        if !winclip::has_text() {
                            // Record what replaced the text, and which program.
                            tracing::info!(
                                "host clip: no text on clipboard; {}",
                                winclip::describe()
                            );
                            if cache_ok && age_ms < 300_000 {
                                tracing::info!(
                                    age_ms,
                                    "host clip: keeping captured text for the viewer"
                                );
                                continue;
                            }
                        }
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

/// How often the fast text watcher checks the clipboard counter. Cheap: it
/// only reads a number unless the clipboard changed.
const FAST_POLL_INTERVAL: Duration = Duration::from_millis(15);

/// Milliseconds since the UNIX epoch.
fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Reads the clipboard's text right now, caches it and tells the viewer this
/// computer has text. Returns false when there is no text to read.
///
/// Some host programs replace the clipboard a fraction of a second after a
/// copy (logs showed a single registered format left behind). Reading the
/// text immediately means the viewer can still paste it afterwards.
fn capture_text_now(inner: &Inner) -> bool {
    let Some(text) = winclip::read_text(inner.max_bytes) else {
        return false;
    };
    let units = text.encode_utf16().count();
    if let Ok(mut cache) = inner.last_text.lock() {
        *cache = Some(text);
    }
    inner.text_cached_ms.store(epoch_ms(), Ordering::Relaxed);
    tracing::info!(units, "host clip: captured copied text");
    inner.send(ClipboardMessage::SendInitiateCopy(vec![
        ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT),
    ]));
    true
}

/// Watches the clipboard counter every few milliseconds and captures text the
/// moment it appears. Files and everything else stay with the main poller.
fn spawn_fast_text_poller(inner: Arc<Inner>) -> Poller {
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    let join = std::thread::Builder::new()
        .name("cipher-desk-clip-text".into())
        .spawn(move || {
            let mut prev = winclip::sequence_number();
            while flag.load(Ordering::Relaxed) {
                std::thread::sleep(FAST_POLL_INTERVAL);
                if !flag.load(Ordering::Relaxed) {
                    break;
                }
                let seq = winclip::sequence_number();
                if seq == prev {
                    continue;
                }
                prev = seq;
                // Our own write of text received from the viewer: not a copy.
                if seq == inner.own_seq.load(Ordering::Relaxed) {
                    continue;
                }
                if winclip::has_text() && capture_text_now(&inner) {
                    tracing::info!(seq, "host clip: text captured by fast watcher");
                }
            }
        })
        .ok();
    if join.is_none() {
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
    text_poller: Option<Poller>,
}

impl core::fmt::Debug for Backend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HostClipboardBackend")
    }
}

ironrdp_core::impl_as_any!(Backend);

impl Drop for Backend {
    fn drop(&mut self) {
        // Not joined: the threads may be mid-sleep and will exit on their own.
        for p in [self.poller.take(), self.text_poller.take()]
            .into_iter()
            .flatten()
        {
            p.running.store(false, Ordering::Relaxed);
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
        if self.text_poller.is_none() {
            self.text_poller = Some(spawn_fast_text_poller(Arc::clone(&self.inner)));
        }
        // Offer files if the host clipboard already has them; otherwise
        // advertise text. Without this, files copied before the viewer
        // connects are invisible to the viewer (the poller only detects
        // changes, so a pre-existing CF_HDROP is never sent).
        if !offer_copied_files(&self.inner, true) {
            advertise(&self.inner);
        }
    }

    fn on_request_format_list(&mut self) {
        // Same logic as on_ready: offer files if present, else advertise text.
        if !offer_copied_files(&self.inner, true) {
            advertise(&self.inner);
        }
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        // The viewer copied something: pull files if offered, else text.
        if let Some(file_format) = formats.iter().find(|f| is_file_list(f)) {
            // If we just wrote received files to CF_HDROP, skip the paste
            // initiation so a new transfer does not replace them before the
            // user can Ctrl+V in Explorer.
            if self.inner.hdrop_pending.load(Ordering::Relaxed) {
                tracing::info!(
                    "host clip: viewer announced files but hdrop_pending — \
                     skipping SendInitiatePaste to protect CF_HDROP"
                );
                return;
            }
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
        tracing::info!(
            count = files.len(),
            ?clip_data_id,
            "host clip: got file list from viewer, starting receive"
        );
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
            tracing::info!(
                file_count = rx.file_count(),
                total_bytes = rx.total_bytes,
                "host clip: receiver started"
            );
            *r = Some(rx);
            step
        };
        self.inner.receive_step(step);
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        // The viewer wants this computer's clipboard text.
        let response = if request.format == ClipboardFormatId::CF_UNICODETEXT {
            // Try the live Windows clipboard first. Fall back to the cached
            // copy if another program replaced the text since we captured it.
            let text = winclip::read_text(self.inner.max_bytes).or_else(|| {
                self.inner
                    .last_text
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
            });
            match text {
                Some(t) => OwnedFormatDataResponse::new_unicode_string(&t),
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
            if !text.is_empty() && winclip::write_text(&text).is_ok() {
                // Text from viewer replaced the clipboard; CF_HDROP is gone.
                self.inner.hdrop_pending.store(false, Ordering::Relaxed);
            }
        }
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        // The viewer is downloading (a piece of) a file we offered.
        tracing::info!(
            stream_id = request.stream_id,
            index = request.index,
            "host clip: viewer requesting file contents"
        );
        let answer = self
            .inner
            .outgoing
            .lock()
            .ok()
            .and_then(|out| with_file_rights(|| xfer::answer(&out, &request)).flatten());
        let response = match answer {
            Some(xfer::FileContentsAnswer::Size(size)) => {
                tracing::info!(
                    stream_id = request.stream_id,
                    size,
                    "host clip: answering SIZE request"
                );
                FileContentsResponse::new_size_response(request.stream_id, size)
            }
            Some(xfer::FileContentsAnswer::Data(bytes)) => {
                tracing::info!(
                    stream_id = request.stream_id,
                    len = bytes.len(),
                    "host clip: answering DATA request"
                );
                FileContentsResponse::new_data_response(request.stream_id, bytes)
            }
            None => {
                tracing::error!(
                    stream_id = request.stream_id,
                    index = request.index,
                    "host clip: no answer for file request (outgoing empty?)"
                );
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
    use windows::Win32::System::Com::{
        IDataObject, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardFormatNameW, GetClipboardOwner, GetClipboardSequenceNumber,
        IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows::Win32::System::Ole::{OleGetClipboard, ReleaseStgMedium};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{ILCombine, ILFree, SHGetPathFromIDListW};

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

    /// While alive, this thread acts with the rights of the person signed in
    /// to this process's session; dropping it goes back to the process's own.
    pub struct SignedInUser;

    impl SignedInUser {
        /// `None` when nobody is signed in to this session (or the token
        /// cannot be had).
        pub fn impersonate() -> Option<Self> {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::Security::ImpersonateLoggedOnUser;
            use windows::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSQueryUserToken};
            use windows::Win32::System::Threading::GetCurrentProcessId;

            // SAFETY: out-pointers are valid locals; the token is closed on
            // every path; impersonation is undone in Drop on this thread.
            unsafe {
                let mut session = 0u32;
                ProcessIdToSessionId(GetCurrentProcessId(), &mut session).ok()?;
                let mut token = HANDLE::default();
                WTSQueryUserToken(session, &mut token).ok()?;
                let done = ImpersonateLoggedOnUser(token);
                let _ = CloseHandle(token);
                done.ok()?;
            }
            Some(Self)
        }
    }

    impl Drop for SignedInUser {
        fn drop(&mut self) {
            use windows::Win32::Security::RevertToSelf;
            // SAFETY: undoes the impersonation started in `impersonate` on
            // this same thread.
            unsafe {
                let _ = RevertToSelf();
            }
        }
    }

    /// A counter Windows bumps on every clipboard change; no open needed.
    pub fn sequence_number() -> u32 {
        // SAFETY: reads a global counter, no state kept.
        unsafe { GetClipboardSequenceNumber() }
    }

    /// A short description of the clipboard for the log: the program that
    /// owns it (file name only) and each format with its name. Never
    /// includes the clipboard's contents.
    pub fn describe() -> String {
        let owner = owner_program().unwrap_or_else(|| "unknown".into());
        let formats = match Guard::open() {
            Ok(_guard) => {
                let mut v = Vec::new();
                // SAFETY: clipboard is open (held by `_guard`); a read-only walk.
                unsafe {
                    let mut fmt = 0u32;
                    loop {
                        fmt = EnumClipboardFormats(fmt);
                        if fmt == 0 || v.len() >= 32 {
                            break;
                        }
                        v.push(format_label(fmt));
                    }
                }
                v.join(", ")
            }
            Err(_) => "clipboard busy".into(),
        };
        let ole = if ole_clipboard().is_some() {
            format!("ole_formats=[{}]", ole_format_list().join(", "))
        } else {
            "ole=unavailable".to_string()
        };
        format!("owner={owner}; formats=[{formats}]; {ole}")
    }

    /// A format's number plus its name, e.g. `49161 "DataObject"`.
    fn format_label(fmt: u32) -> String {
        let standard = match fmt {
            1 => "CF_TEXT",
            2 => "CF_BITMAP",
            3 => "CF_METAFILEPICT",
            7 => "CF_OEMTEXT",
            8 => "CF_DIB",
            13 => "CF_UNICODETEXT",
            14 => "CF_ENHMETAFILE",
            15 => "CF_HDROP",
            16 => "CF_LOCALE",
            17 => "CF_DIBV5",
            _ => "",
        };
        if !standard.is_empty() {
            return format!("{fmt} {standard}");
        }
        if fmt >= 0xC000 {
            let mut buf = [0u16; 128];
            // SAFETY: writes at most buf.len() units into our own buffer.
            let n = unsafe { GetClipboardFormatNameW(fmt, &mut buf) };
            if n > 0 {
                let name = String::from_utf16_lossy(&buf[..n as usize]);
                return format!("{fmt} \"{name}\"");
            }
        }
        fmt.to_string()
    }

    /// The file name and process id of the program that owns the clipboard.
    fn owner_program() -> Option<String> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
        // SAFETY: read-only queries; the process handle is closed before
        // returning and the name is read only up to the length Windows reports.
        unsafe {
            let hwnd = GetClipboardOwner().ok()?;
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            if pid == 0 {
                return None;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let res = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            res.ok()?;
            let path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
            // File name only: the full path can contain the user's name.
            let name = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
            Some(format!("{name} (pid {pid})"))
        }
    }

    /// Whether the clipboard currently holds plain text.
    ///
    /// Checks the classic clipboard first, then falls back to OLE. Some
    /// programs put data on the clipboard only through `OleSetClipboard`
    /// (confirmed from a host log: every copy left just one private format,
    /// "DataObject", and no CF_UNICODETEXT) — the plain `IsClipboardFormatAvailable`
    /// / `GetClipboardData` calls above can never see that text at all, no
    /// matter how fast or how often they are polled. The OLE clipboard
    /// (`IDataObject`) is the only way to reach it.
    pub fn has_text() -> bool {
        // SAFETY: queries format availability only.
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() } {
            return true;
        }
        ole_has_text()
    }

    /// Reads the clipboard's text, or `None` if there is none, it is too large,
    /// or it cannot be read. `max_bytes` is the UTF-16 size limit.
    pub fn read_text(max_bytes: usize) -> Option<String> {
        if let Some(text) = read_text_plain(max_bytes) {
            return Some(text);
        }
        ole_read_text(max_bytes)
    }

    /// Reads CF_UNICODETEXT via the classic clipboard functions only.
    fn read_text_plain(max_bytes: usize) -> Option<String> {
        // SAFETY: queries format availability only.
        if !unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() } {
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

    thread_local! {
        // OLE requires one-time, per-thread initialization before
        // OleGetClipboard works. The clipboard poller threads and the RDP
        // runtime's callback threads are all separate OS threads, so this
        // runs once on each the first time it touches the OLE clipboard.
        static OLE_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Initializes OLE on this thread if it has not been already. Safe to
    /// call repeatedly; cheap after the first call.
    fn ensure_ole_ready() {
        OLE_READY.with(|ready| {
            if !ready.get() {
                // SAFETY: OleInitialize has no preconditions beyond being
                // called once per thread before other OLE calls on it. We
                // never pair it with OleUninitialize: these threads run for
                // the life of the connection or the process and Windows
                // reclaims COM state when a thread exits.
                unsafe {
                    let _ = windows::Win32::System::Ole::OleInitialize(None);
                }
                ready.set(true);
            }
        });
    }

    /// The current OLE clipboard contents, if any program has registered an
    /// `IDataObject` there (via `OleSetClipboard`).
    fn ole_clipboard() -> Option<IDataObject> {
        ensure_ole_ready();
        // SAFETY: returns a new reference to the clipboard's data object, or
        // an error if nothing has used OLE for the clipboard; no state kept.
        unsafe { OleGetClipboard().ok() }
    }

    fn hglobal_format(cf: u32) -> FORMATETC {
        FORMATETC {
            cfFormat: cf as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    /// Whether the OLE clipboard data object (if any) offers this format.
    fn ole_has_format(cf: u32) -> bool {
        let Some(data_obj) = ole_clipboard() else {
            return false;
        };
        let fmt = hglobal_format(cf);
        // SAFETY: QueryGetData only asks whether the format is available; it
        // does not render or return any data.
        unsafe { data_obj.QueryGetData(&fmt).is_ok() }
    }

    fn ole_has_text() -> bool {
        ole_has_format(CF_UNICODETEXT)
    }

    /// Every format the OLE clipboard data object offers, with its medium,
    /// e.g. `"49 \"FileGroupDescriptorW\" tymed=1"`. Diagnostic only — lists
    /// what is really available instead of guessing one format at a time.
    fn ole_format_list() -> Vec<String> {
        let Some(data_obj) = ole_clipboard() else {
            return vec!["unavailable".into()];
        };
        // SAFETY: EnumFormatEtc(DATADIR_GET) returns an enumerator this call
        // owns; windows-rs releases it (Drop -> Release) when it goes out of
        // scope below.
        let Ok(enum_fmt) = (unsafe { data_obj.EnumFormatEtc(DATADIR_GET.0 as u32) }) else {
            return vec!["enum unavailable".into()];
        };
        let mut out = Vec::new();
        loop {
            let mut one = FORMATETC {
                cfFormat: 0,
                ptd: std::ptr::null_mut(),
                dwAspect: 0,
                lindex: 0,
                tymed: 0,
            };
            let mut fetched = 0u32;
            // SAFETY: writes at most one FORMATETC into `one` and reports how
            // many in `fetched`; we stop once it reports none.
            let hr = unsafe { enum_fmt.Next(core::slice::from_mut(&mut one), Some(&mut fetched)) };
            if hr.is_err() || fetched == 0 || out.len() >= 24 {
                break;
            }
            out.push(format!(
                "{} tymed={}",
                format_label(one.cfFormat as u32),
                one.tymed
            ));
        }
        if out.is_empty() {
            out.push("none".into());
        }
        out
    }

    /// Asks the OLE clipboard data object to render `cf` into memory and
    /// returns a copy of the bytes (at most `max_bytes`).
    ///
    /// Programs that copy with `OleSetClipboard` (File Explorer on recent
    /// Windows 11 does, for files) may leave only the private "DataObject"
    /// format on the classic clipboard; the real formats are reachable only
    /// through this data object.
    fn ole_read_bytes(cf: u32, max_bytes: usize) -> Option<Vec<u8>> {
        let data_obj = ole_clipboard()?;
        let fmt = hglobal_format(cf);
        // SAFETY: GetData renders the format into a medium this call owns;
        // it is released below with ReleaseStgMedium.
        let mut medium = unsafe { data_obj.GetData(&fmt).ok()? };
        // SAFETY: tymed is TYMED_HGLOBAL (what we asked for), so the union's
        // hGlobal field is the one GetData filled in. The memory is only
        // locked for the copy below, then unlocked before the medium (which
        // owns the handle) is released.
        let bytes = unsafe {
            let hg = medium.u.hGlobal;
            let size = if hg.is_invalid() { 0 } else { GlobalSize(hg) };
            if size == 0 || size > max_bytes {
                None
            } else {
                let ptr = GlobalLock(hg) as *const u8;
                if ptr.is_null() {
                    None
                } else {
                    let v = core::slice::from_raw_parts(ptr, size).to_vec();
                    let _ = GlobalUnlock(hg);
                    Some(v)
                }
            }
        };
        // SAFETY: medium was returned by GetData above; we own it and must
        // release it exactly once, which frees the hGlobal handle.
        unsafe {
            ReleaseStgMedium(&mut medium);
        }
        bytes
    }

    /// Reads CF_UNICODETEXT through the OLE clipboard (`IDataObject`).
    fn ole_read_text(max_bytes: usize) -> Option<String> {
        let bytes = ole_read_bytes(CF_UNICODETEXT, max_bytes + 2)?;
        let mut buf: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Some(end) = buf.iter().position(|&u| u == 0) {
            buf.truncate(end);
        }
        String::from_utf16(&buf).ok()
    }

    /// The files on the clipboard via OLE: asks the data object for CF_HDROP
    /// first, then the Shell IDList Array, either of which a shell data
    /// object may render on request even when neither is listed on the
    /// classic clipboard (confirmed from a host log: Explorer's own
    /// IDataObject answered "no" for both when asked directly for CF_HDROP —
    /// this also tries the format Explorer actually uses internally).
    fn ole_read_files() -> Option<Vec<std::path::PathBuf>> {
        // A DROPFILES block for 1000 long paths is well under 1 MiB.
        if let Some(bytes) = ole_read_bytes(CF_HDROP, 1 << 20) {
            let files = rdpv_core::xfer::parse_dropfiles(&bytes, MAX_FILES);
            if !files.is_empty() {
                return Some(files);
            }
        }
        // SAFETY: RegisterClipboardFormatW is a one-time idempotent
        // registration using a literal name; no clipboard access here.
        let cf_shell_idlist =
            unsafe { RegisterClipboardFormatW(windows::core::w!("Shell IDList Array")) };
        if cf_shell_idlist == 0 {
            return None;
        }
        let bytes = ole_read_bytes(cf_shell_idlist, 1 << 20)?;
        parse_shell_idlist(&bytes)
    }

    /// The files on the clipboard (copied in Explorer), or `None` if it holds
    /// no files. Tries the classic clipboard first, then OLE (after the
    /// classic clipboard has been closed again, since OLE opens it itself).
    pub fn read_files() -> Option<Vec<std::path::PathBuf>> {
        if let Some(files) = read_files_plain() {
            return Some(files);
        }
        let files = ole_read_files();
        if let Some(f) = &files {
            tracing::info!(
                count = f.len(),
                "host clip: files found through OLE clipboard"
            );
        }
        files
    }

    /// The files on the classic clipboard, or `None` if it holds none.
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
    fn read_files_plain() -> Option<Vec<std::path::PathBuf>> {
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

        // SAFETY: clipboard is still open; we lock the global memory just long
        // enough to copy it out, then hand the bytes to the format-agnostic parser.
        unsafe {
            let handle = GetClipboardData(cf_shell_idlist).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let hg = HGLOBAL(handle.0);
            let size = GlobalSize(hg);
            let ptr = GlobalLock(hg) as *const u8;
            if ptr.is_null() || size == 0 {
                return None;
            }
            let bytes = core::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(hg);
            parse_shell_idlist(&bytes)
        }
    }

    /// Parses a Shell IDList Array ("CIDA") blob — the format Explorer uses
    /// to represent copied files/folders — into filesystem paths. Works on a
    /// plain byte copy, so it is shared by the classic clipboard read above
    /// and the OLE read below.
    fn parse_shell_idlist(bytes: &[u8]) -> Option<Vec<std::path::PathBuf>> {
        let size = bytes.len();
        if size < 8 {
            return None;
        }
        // SAFETY: `ptr` points into `bytes`, which outlives every use below;
        // every offset is checked against `size` before it is read.
        unsafe {
            let ptr = bytes.as_ptr();

            // CIDA layout: { cidl: u32, aoffset[cidl+1]: u32, raw_pidl_data }
            // aoffset[0] = parent folder PIDL; aoffset[1..=cidl] = item PIDLs.
            let cidl = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
            if cidl == 0 || cidl > MAX_FILES || size < 4 + (cidl + 1) * 4 {
                return None;
            }

            let parent_offset =
                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            if parent_offset >= size {
                return None;
            }

            let mut paths = Vec::new();
            for i in 1..=cidl {
                let off_idx = 4 + i * 4;
                if off_idx + 4 > size {
                    continue;
                }
                let item_off = u32::from_le_bytes([
                    bytes[off_idx],
                    bytes[off_idx + 1],
                    bytes[off_idx + 2],
                    bytes[off_idx + 3],
                ]) as usize;
                if item_off >= size {
                    continue;
                }

                // Combine parent + relative item PIDL into an absolute PIDL.
                let parent_pidl = ptr.add(parent_offset) as *const ITEMIDLIST;
                let item_pidl = ptr.add(item_off) as *const ITEMIDLIST;
                // ILCombine takes Option<*const ITEMIDLIST> in windows 0.62.
                let abs_pidl: *mut ITEMIDLIST = ILCombine(Some(parent_pidl), Some(item_pidl));
                if abs_pidl.is_null() {
                    continue;
                }

                // Convert the absolute PIDL to a filesystem path.
                // SHGetPathFromIDListW takes &mut [u16; 260] (MAX_PATH) in windows 0.62.
                let mut path_buf = [0u16; 260];
                let ok = SHGetPathFromIDListW(abs_pidl as *const _, &mut path_buf);
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
