//! Two-step verification for Share screen (Google Authenticator), like AnyDesk.
//!
//! The viewer's sign-in carries, besides the sharing password, a small
//! "cookie" in the RDP Client Info PDU (sent inside the TLS connection, after
//! Network Level Authentication has checked the password):
//!
//! - this viewer's **device id** (random, made once per app install), so a host
//!   can remember a device that already entered a correct code (30 days);
//! - the 6-digit **code** from the authenticator app, when the user typed one.
//!
//! The host decides: if two-step verification is on and the device is not
//! remembered, it refuses the connection without a correct code, and the
//! viewer asks for one. Hosts without it ignore the cookie.

use std::path::Path;

/// Starts every cookie: a lone UTF-16 high surrogate makes the field invalid
/// text, so the host reads it as a cookie rather than a password, then a tag.
const MAGIC: &[u8] = b"\x00\xD8CD1:";

/// Length of a device id in bytes.
pub const DEVICE_ID_LEN: usize = 16;

/// A viewer install's id (random; not secret on its own: the password and,
/// the first time, a code are still needed).
pub type DeviceId = [u8; DEVICE_ID_LEN];

/// What the viewer sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondFactor {
    pub device: DeviceId,
    /// Six digits, or `None` when the user has not been asked yet.
    pub code: Option<String>,
}

impl SecondFactor {
    /// The cookie bytes: `MAGIC` + hex device id + `:` + code (may be empty),
    /// padded to an even length (the field is UTF-16).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for b in self.device {
            out.extend_from_slice(format!("{b:02x}").as_bytes());
        }
        out.push(b':');
        if let Some(code) = &self.code {
            out.extend(code.bytes().filter(u8::is_ascii_digit).take(8));
        }
        if out.len() % 2 != 0 {
            out.push(b' ');
        }
        out
    }

    /// Reads a cookie; `None` if it is not one of ours or is damaged.
    pub fn decode(cookie: &[u8]) -> Option<Self> {
        let rest = cookie.strip_prefix(MAGIC)?;
        // Tolerate the terminating NULs and padding a sender may add.
        let end = rest
            .iter()
            .rposition(|b| *b != 0 && *b != b' ')
            .map_or(0, |i| i + 1);
        let text = std::str::from_utf8(&rest[..end]).ok()?;
        let (hex, code) = text.split_once(':')?;
        if hex.len() != DEVICE_ID_LEN * 2 {
            return None;
        }
        let mut device = [0u8; DEVICE_ID_LEN];
        for (i, slot) in device.iter_mut().enumerate() {
            *slot = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        let code =
            (!code.is_empty() && code.bytes().all(|b| b.is_ascii_digit())).then(|| code.to_owned());
        Some(Self { device, code })
    }
}

/// This app install's device id, kept in `dir/device-id` (made on first use).
pub fn device_id(dir: &Path) -> std::io::Result<DeviceId> {
    let path = dir.join("device-id");
    if let Some(bytes) = crate::fsutil::read_limited(&path)? {
        if let Ok(id) = DeviceId::try_from(bytes.as_slice()) {
            return Ok(id);
        }
    }
    let mut id = [0u8; DEVICE_ID_LEN];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut id)
        .map_err(|_| std::io::Error::other("no randomness"))?;
    std::fs::create_dir_all(dir)?;
    crate::fsutil::write_atomically(&path, &id)?;
    Ok(id)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn cookie_round_trips() {
        let device = [7u8; DEVICE_ID_LEN];
        for code in [None, Some("012345".to_owned())] {
            let f = SecondFactor {
                device,
                code: code.clone(),
            };
            let bytes = f.encode();
            assert_eq!(bytes.len() % 2, 0);
            assert_eq!(SecondFactor::decode(&bytes), Some(f.clone()));
            // A trailing terminator from the wire is fine.
            let mut with_nul = bytes.clone();
            with_nul.extend_from_slice(&[0, 0]);
            assert_eq!(SecondFactor::decode(&with_nul), Some(f));
        }
    }

    #[test]
    fn the_cookie_is_not_valid_text() {
        // So the host reads it as a cookie, not as a password string.
        let bytes = SecondFactor {
            device: [1; DEVICE_ID_LEN],
            code: None,
        }
        .encode();
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(String::from_utf16(&units).is_err());
    }

    #[test]
    fn rejects_other_cookies() {
        assert_eq!(SecondFactor::decode(b"hello"), None);
        assert_eq!(SecondFactor::decode(b"\x00\xD8CD1:abc:123456"), None);
        assert_eq!(SecondFactor::decode(&[]), None);
    }

    #[test]
    fn device_id_is_made_once() {
        let dir = std::env::temp_dir().join(format!("cd-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = device_id(&dir).expect("made");
        let b = device_id(&dir).expect("read back");
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
