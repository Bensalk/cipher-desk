//! Text clipboard sharing (Stage 6; S3, S6).
//!
//! Only plain text (CF_UNICODETEXT) is exchanged. Other formats (files, images,
//! rich text) are never offered or requested. The policy decides direction and
//! size; it is applied in [`crate::session::Session`] before anything is sent
//! to the host or handed to the app.
//!
//! The backend does no work itself: IronRDP's clipboard channel calls it, and
//! it queues what happened for the session loop to act on.

use std::sync::{Arc, Mutex};

use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
};

/// Which way text may flow, and how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardPolicy {
    /// Text copied on this device may be pasted on the host.
    pub local_to_host: bool,
    /// Text copied on the host may be pasted on this device.
    pub host_to_local: bool,
    /// Largest text accepted or sent, in bytes of UTF-16.
    pub max_bytes: usize,
}

impl ClipboardPolicy {
    /// Clipboard off (the default, S3).
    pub const fn off() -> Self {
        Self {
            local_to_host: false,
            host_to_local: false,
            max_bytes: 0,
        }
    }

    /// From the security defaults' direction setting.
    pub fn from_direction(d: crate::policy::ClipboardDirection, max_bytes: usize) -> Self {
        use crate::policy::ClipboardDirection as D;
        Self {
            local_to_host: matches!(d, D::LocalToHost | D::Both),
            host_to_local: matches!(d, D::HostToLocal | D::Both),
            max_bytes,
        }
    }

    pub const fn is_enabled(&self) -> bool {
        self.local_to_host || self.host_to_local
    }

    /// Size of `text` as UTF-16 bytes, the format sent over RDP.
    pub fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count() * 2
    }

    pub fn fits(&self, text: &str) -> bool {
        Self::utf16_len(text) <= self.max_bytes
    }
}

impl Default for ClipboardPolicy {
    fn default() -> Self {
        Self::off()
    }
}

/// Something the clipboard channel reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipAction {
    /// The channel is ready, or the host asked for our format list:
    /// advertise what this device can offer.
    AdvertiseLocal,
    /// The host's clipboard changed; `has_text` if plain text is available.
    RemoteChanged { has_text: bool },
    /// The host wants our clipboard data in this format.
    DataRequested(ClipboardFormatId),
    /// Text arrived from the host (`None` = the host sent an error or no text).
    DataReceived(Option<Vec<u8>>),
}

/// Queue shared between the backend and the session.
pub(crate) type ClipQueue = Arc<Mutex<Vec<ClipAction>>>;

#[derive(Debug)]
pub(crate) struct QueueBackend {
    queue: ClipQueue,
}

impl QueueBackend {
    pub(crate) fn new(queue: ClipQueue) -> Self {
        Self { queue }
    }

    fn push(&self, a: ClipAction) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(a);
        }
    }
}

ironrdp_core::impl_as_any!(QueueBackend);

impl CliprdrBackend for QueueBackend {
    fn temporary_directory(&self) -> &str {
        // File transfer is never enabled; nothing is written here.
        ""
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        // No file copy, no long format names: plain text only.
        ClipboardGeneralCapabilityFlags::empty()
    }

    fn on_ready(&mut self) {
        self.push(ClipAction::AdvertiseLocal);
    }

    fn on_request_format_list(&mut self) {
        self.push(ClipAction::AdvertiseLocal);
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        let has_text = formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT);
        self.push(ClipAction::RemoteChanged { has_text });
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.push(ClipAction::DataRequested(request.format));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        let data = (!response.is_error()).then(|| response.data().to_vec());
        self.push(ClipAction::DataReceived(data));
    }

    // Files are never offered or accepted.
    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}

/// Decodes CF_UNICODETEXT bytes (UTF-16LE, usually NUL-terminated).
pub(crate) fn decode_unicode_text(bytes: &[u8]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ClipboardDirection;

    #[test]
    fn default_is_off() {
        let p = ClipboardPolicy::default();
        assert!(!p.is_enabled());
        assert!(!p.fits("x"));
    }

    #[test]
    fn directions() {
        let p = ClipboardPolicy::from_direction(ClipboardDirection::HostToLocal, 1024);
        assert!(p.host_to_local && !p.local_to_host);
        let p = ClipboardPolicy::from_direction(ClipboardDirection::Both, 1024);
        assert!(p.host_to_local && p.local_to_host);
        assert!(!ClipboardPolicy::from_direction(ClipboardDirection::Off, 1024).is_enabled());
    }

    #[test]
    fn size_limit_counts_utf16_bytes() {
        let p = ClipboardPolicy::from_direction(ClipboardDirection::Both, 8);
        assert!(p.fits("abcd")); // 8 bytes
        assert!(!p.fits("abcde")); // 10 bytes
        assert_eq!(ClipboardPolicy::utf16_len("é😀"), 6); // 1 + 2 UTF-16 units
    }

    #[test]
    fn decodes_utf16_with_terminator() {
        let mut bytes: Vec<u8> = "héllo 😀"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.extend_from_slice(&[0, 0, 0x41, 0]); // NUL then junk
        assert_eq!(decode_unicode_text(&bytes).as_deref(), Some("héllo 😀"));
        assert_eq!(decode_unicode_text(&[0x41]), None);
        assert_eq!(decode_unicode_text(&[0x00, 0xD8]), None); // lone surrogate
    }

    #[test]
    fn backend_queues_only_text_events() {
        let q: ClipQueue = Arc::default();
        let mut b = QueueBackend::new(Arc::clone(&q));
        b.on_ready();
        b.on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]);
        b.on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::new(49_000))]);
        let got = q.lock().map(|v| v.clone()).unwrap_or_default();
        assert_eq!(
            got,
            vec![
                ClipAction::AdvertiseLocal,
                ClipAction::RemoteChanged { has_text: true },
                ClipAction::RemoteChanged { has_text: false },
            ]
        );
    }
}
