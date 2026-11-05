//! Host-side text clipboard for the Share screen method.
//!
//! The viewer (client) side already shares plain text; this is the matching
//! piece on the computer being shared, so copy and paste work both ways:
//! text copied here can be pasted on the viewer, and text copied on the viewer
//! can be pasted here. Only plain text (CF_UNICODETEXT) is exchanged — never
//! files, images or rich text. A size cap limits how much text is accepted.
//!
//! This is a `unsafe` module like [`crate::win`]: the Windows clipboard has no
//! safe wrapper. Every call says why it is sound.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend, CliprdrBackendFactory};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
    OwnedFormatDataResponse,
};
use ironrdp_server::{ServerEvent, ServerEventSender};
use tokio::sync::mpsc::UnboundedSender;

use ironrdp_server::CliprdrServerFactory;

/// How often the host clipboard is checked for a change, to tell the viewer.
const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// Supplies a clipboard backend to the server and holds the event sender the
/// backend uses to talk back to the server's loop.
pub struct HostClipboard {
    inner: Arc<Inner>,
}

struct Inner {
    sender: Mutex<Option<UnboundedSender<ServerEvent>>>,
    /// Largest text accepted from the viewer, in bytes of UTF-16.
    max_bytes: usize,
}

impl HostClipboard {
    /// `max_bytes` caps text accepted from the viewer (bytes of UTF-16).
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                sender: Mutex::new(None),
                max_bytes,
            }),
        }
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

/// Watches this computer's clipboard and re-advertises when it changes, so a
/// copy made here while a viewer is connected becomes pasteable there.
struct Poller {
    running: Arc<AtomicBool>,
    _join: JoinHandle<()>,
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
    match join {
        Some(join) => Poller {
            running,
            _join: join,
        },
        None => Poller {
            // The thread could not start; mark it stopped so Drop is a no-op.
            running: Arc::new(AtomicBool::new(false)),
            _join: std::thread::spawn(|| {}),
        },
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

impl CliprdrBackend for Backend {
    fn temporary_directory(&self) -> &str {
        // Files are never exchanged; nothing is written here.
        "."
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        // Plain text only: no file copy, no long format names.
        ClipboardGeneralCapabilityFlags::empty()
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
        // The viewer copied something: if it offers text, ask for it so we can
        // put it on this computer's clipboard.
        let has_text = formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT);
        if has_text {
            self.inner.send(ClipboardMessage::SendInitiatePaste(
                ClipboardFormatId::CF_UNICODETEXT,
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

    // Files and locking are never used (plain text only).
    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
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
