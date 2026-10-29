//! Entry points for the fuzzers in `rust/fuzz` (SA-3). Each takes raw bytes,
//! runs one of our parsers of untrusted input, and checks that what it accepts
//! is consistent. A panic here is a bug: the fuzzer reports it.
//!
//! Input sources: **server** = the remote computer (possibly hostile);
//! **file** = app data or an imported file; **user** = typed into the app.

use crate::{clipboard, input, pins, profiles, session, settings, wol};

/// Server: error and disconnect texts are mapped to fixed messages.
pub fn server_text(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let a = session::classify_text(&text);
    let b = session::classify_disconnect_reason(&text);
    // Whatever the server says, the result is one of the fixed catalogue
    // errors, whose text never includes anything the server sent.
    for e in [a, b] {
        assert!(crate::status::ConnectionError::ALL.contains(&e));
    }
}

/// Server: clipboard text (CF_UNICODETEXT bytes).
pub fn clipboard_text(data: &[u8]) {
    if let Some(text) = clipboard::decode_unicode_text(data) {
        assert!(!text.contains('\0'), "decoded text keeps a NUL");
        let policy = clipboard::ClipboardPolicy::from_direction(
            crate::policy::ClipboardDirection::Both,
            1024 * 1024,
        );
        let _ = policy.fits(&text);
        assert!(clipboard::ClipboardPolicy::utf16_len(&text) <= data.len() + 2);
    }
}

/// Server: the certificate presented during TLS.
pub fn certificate(data: &[u8]) {
    let _ = session::server_public_key(data);
    let fp = pins::fingerprint(data);
    assert_eq!(fp.len(), 32 * 3 - 1);
}

/// Server: desktop size announcements.
pub fn desktop_size(data: &[u8]) {
    if let [a, b, c, d, ..] = *data {
        let (w, h) = (u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d]));
        if session::check_desktop_size(w, h).is_ok() {
            assert!(u32::from(w) * u32::from(h) <= session::MAX_DESKTOP_PIXELS);
        }
    }
}

/// File: connections file, import file, settings file, pin file.
pub fn files(data: &[u8]) {
    if let Some((conns, history, _skipped)) = profiles::decode(data) {
        assert!(conns.len() <= profiles::MAX_CONNECTIONS);
        assert!(history.len() <= profiles::MAX_HISTORY);
        assert!(conns.iter().all(profiles::is_valid));
        assert!(history
            .iter()
            .all(|h| conns.iter().any(|c| c.id == h.connection_id)));
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let imported = profiles::import(text);
        assert!(imported.connections.len() <= profiles::MAX_CONNECTIONS);
        assert!(imported
            .connections
            .iter()
            .all(|c| c.clipboard == profiles::ClipboardSetting::Off));
        if imported.not_an_export {
            assert!(imported.connections.is_empty());
        }
    }
    if let Some(s) = settings::decode(data) {
        assert!(settings::IDLE_CHOICES.contains(&s.idle_minutes));
    }
    let _ = pins::decode(data);
}

/// User: the connection form's fields, separated by newlines in the input.
pub fn form_fields(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let mut parts = text.splitn(4, '\n');
    let (host, port, user, rest) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    if let Ok(ep) = input::parse_endpoint(host, port) {
        // What was accepted parses again to the same thing.
        assert!(!ep.host.is_empty());
        assert!(!ep.host.chars().any(char::is_control));
        let again = input::parse_endpoint(&ep.host, &ep.port.to_string());
        assert!(again.is_ok_and(|e| e == ep), "endpoint does not round-trip");
    }
    if let Ok(u) = input::parse_username(user) {
        assert!(!u.user.is_empty() && !u.user.chars().any(char::is_control));
    }
    let _ = input::validate_profile_name(rest);
    let _ = input::parse_port(port);
    if let Ok(mac) = wol::parse_mac(rest) {
        let printed = mac.map(|b| format!("{b:02x}")).join(":");
        assert_eq!(wol::parse_mac(&printed), Ok(mac), "MAC does not round-trip");
    }
}
