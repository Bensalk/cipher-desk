//! Host-side clipboard and file transfer for the Share screen method.
//!
//! The viewer (client) side already shares plain text and files; this is the
//! matching piece on the computer being shared, so copy and paste and file
//! copy work both ways:
//! - Text copied here can be pasted on the viewer, and vice versa (plain text
//!   only, CF_UNICODETEXT, with a size cap).
//! - Files the viewer sends arrive in a "Received" folder; files offered here
//!   with [`ClipHandle::offer_files`] are downloaded by the viewer.
//!
//! This is a `unsafe` module like [`crate::win`]: the Windows clipboard has no
//! safe wrapper. Every call says why it is sound. File reads and writes use
//! plain `std::fs`.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend, CliprdrBackendFactory};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardFormatName, ClipboardGeneralCapabilityFlags,
    FileContentsFlags, FileContentsRequest, FileContentsResponse, FileDescriptor,
    FormatDataRequest, FormatDataResponse, LockDataId, OwnedFormatDataResponse,
};
use ironrdp_server::{CliprdrServerFactory, ServerEvent, ServerEventSender};
use rdpv_core::xfer;
use tokio::sync::mpsc::UnboundedSender;

use crate::HostEvent;

/// How often the host clipboard is checked for a change, to tell the viewer.
const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// Supplies a clipboard backend to the server and holds the shared state the
/// backend and the app both use.
pub struct HostClipboard {
    inner: Arc<Inner>,
}

/// A file the viewer is sending, mid-arrival.
struct IncomingFile {
    name: String,
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
    /// Files arriving from the viewer, keyed by stream id.
    incoming: Mutex<HashMap<u32, IncomingFile>>,
    /// Largest text accepted from the viewer, in bytes of UTF-16.
    max_bytes: usize,
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
                incoming: Mutex::new(HashMap::new()),
                max_bytes,
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

    /// Path and size of the file offered at `index`, if any.
    fn offered(&self, index: i32) -> Option<(PathBuf, u64)> {
        let i = usize::try_from(index).ok()?;
        let out = self.outgoing.lock().ok()?;
        out.get(i).map(|o| (o.path.clone(), o.size))
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

/// Advertises to the viewer whether this computer's clipboard holds text.
fn advertise(inner: &Inner) {
    let formats = if winclip::has_text() {
        vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
    } else {
        Vec::new()
    };
    inner.send(ClipboardMessage::SendInitiateCopy(formats));
}

/// Reads up to `len` bytes of `path` starting at `position`.
fn read_range(path: &std::path::Path, position: u64, len: u64) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(position)).ok()?;
    let cap = len.min(xfer::MAX_FILE_BYTES);
    let mut buf = vec![0u8; usize::try_from(cap).ok()?];
    let mut read = 0usize;
    while read < buf.len() {
        match f.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
    }
    buf.truncate(read);
    Some(buf)
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
                let seq = winclip::sequence_number();
                if seq != prev {
                    prev = seq;
                    advertise(&inner);
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
        // The viewer is sending files: request each one's bytes in one range.
        for (i, fd) in files.iter().enumerate() {
            let Some(name) = xfer::safe_file_name(&fd.name) else {
                continue;
            };
            let size = fd.file_size.unwrap_or(0);
            if size > xfer::MAX_FILE_BYTES {
                continue;
            }
            let Ok(stream_id) = u32::try_from(i) else {
                continue;
            };
            let Ok(index) = i32::try_from(i) else {
                continue;
            };
            if let Ok(mut map) = self.inner.incoming.lock() {
                map.insert(stream_id, IncomingFile { name });
            }
            let requested_size = u32::try_from(size).unwrap_or(u32::MAX);
            self.inner.send(ClipboardMessage::SendFileContentsRequest(
                FileContentsRequest {
                    stream_id,
                    index,
                    flags: FileContentsFlags::RANGE,
                    position: 0,
                    requested_size,
                    data_id: clip_data_id,
                },
            ));
        }
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
        // The viewer is downloading a file we offered.
        let response = match self.inner.offered(request.index) {
            Some((_path, size)) if request.flags.contains(FileContentsFlags::SIZE) => {
                FileContentsResponse::new_size_response(request.stream_id, size)
            }
            Some((path, _)) => {
                match read_range(&path, request.position, u64::from(request.requested_size)) {
                    Some(bytes) => {
                        FileContentsResponse::new_data_response(request.stream_id, bytes)
                    }
                    None => FileContentsResponse::new_error(request.stream_id),
                }
            }
            None => FileContentsResponse::new_error(request.stream_id),
        };
        self.inner
            .send(ClipboardMessage::SendFileContentsResponse(response));
    }

    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        // A file we requested from the viewer arrived whole; write it.
        if response.is_error() {
            return;
        }
        let file = match self.inner.incoming.lock() {
            Ok(mut map) => map.remove(&response.stream_id()),
            Err(_) => None,
        };
        let Some(file) = file else {
            return;
        };
        if std::fs::create_dir_all(&self.inner.receive_dir).is_err() {
            return;
        }
        let path = xfer::unique_path(&self.inner.receive_dir, &file.name);
        if std::fs::write(&path, response.data()).is_ok() {
            let _ = self
                .inner
                .events
                .send(HostEvent::FileReceived { name: file.name });
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
        CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
        IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };

    const CF_UNICODETEXT: u32 = 13;

    /// Opens the clipboard and closes it on drop.
    struct Guard;

    impl Guard {
        fn open() -> anyhow::Result<Self> {
            // SAFETY: opens the clipboard with no owner window; Drop closes it.
            unsafe { OpenClipboard(None).context("open clipboard")? };
            Ok(Self)
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
    use super::{decode_unicode_text, read_range};

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

    #[test]
    fn read_range_reads_a_slice() {
        let dir = std::env::temp_dir().join(format!("cd-clip-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("f.bin");
        if std::fs::write(&p, b"0123456789").is_err() {
            return;
        }
        assert_eq!(read_range(&p, 2, 3).as_deref(), Some(&b"234"[..]));
        assert_eq!(read_range(&p, 8, 100).as_deref(), Some(&b"89"[..]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
