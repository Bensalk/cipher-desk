//! Two-step verification for Share screen (Google Authenticator).
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
        if !out.len().is_multiple_of(2) {
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

/// Where older versions (and Android) keep the device id: `dir/device-id`.
const FILE_NAME: &str = "device-id";

/// The device id's entry in the OS vault (Windows Credential Manager, Linux
/// Secret Service). Not a valid connection id, so it never clashes with one.
#[cfg(any(windows, target_os = "linux"))]
const VAULT_NAME: &str = "cipherdesk-twostep-device";

/// This app install's device id (made on first use).
///
/// Windows and Linux: kept in the OS vault, like saved passwords, so a copy of
/// the app's folder (a backup, a portable copy on a USB stick) does not carry
/// it and cannot skip the authenticator code. An id older versions kept in
/// `dir/device-id` is moved there, so computers that remember this device
/// still do. Without a usable vault the id lasts for this run of the app only,
/// and the code is asked for again next time. Within one run it never changes.
///
/// Android: `dir/device-id`, inside the app's private storage (the vault
/// there is used from Kotlin, not from here).
#[cfg(any(windows, target_os = "linux"))]
pub fn device_id(dir: &Path) -> std::io::Result<DeviceId> {
    // The same id for the whole run, whatever the vault does in between
    // (it can be locked, or come back later), so a reconnect is recognised.
    static CHOSEN: std::sync::OnceLock<DeviceId> = std::sync::OnceLock::new();
    if let Some(id) = CHOSEN.get() {
        return Ok(*id);
    }
    let id = device_id_from_vault(dir)?;
    Ok(*CHOSEN.get_or_init(|| id))
}

/// The id kept in the vault, moving an older file's id there or making a new
/// one; a new random id if the vault cannot be used.
#[cfg(any(windows, target_os = "linux"))]
fn device_id_from_vault(dir: &Path) -> std::io::Result<DeviceId> {
    use crate::secret::SecretString;
    use crate::vault::{self, VaultError};

    let old = dir.join(FILE_NAME);
    match vault::load(VAULT_NAME) {
        Ok(hex) => {
            if let Some(id) = from_hex(hex.expose()) {
                let _ = std::fs::remove_file(&old);
                return Ok(id);
            }
        }
        Err(VaultError::NotFound) => {}
        Err(_) => return random_id(),
    }
    let id = match read_file_id(&old)? {
        Some(id) => id,
        None => random_id()?,
    };
    if vault::save(VAULT_NAME, &SecretString::new(to_hex(&id))).is_ok() {
        let _ = std::fs::remove_file(&old);
    }
    Ok(id)
}

/// See the Windows/Linux version: on Android the id stays in the app's
/// private storage.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn device_id(dir: &Path) -> std::io::Result<DeviceId> {
    device_id_in_file(dir)
}

/// The id in `dir/device-id`, made and saved there if missing.
#[cfg(any(test, not(any(windows, target_os = "linux"))))]
fn device_id_in_file(dir: &Path) -> std::io::Result<DeviceId> {
    let path = dir.join(FILE_NAME);
    if let Some(id) = read_file_id(&path)? {
        return Ok(id);
    }
    let id = random_id()?;
    std::fs::create_dir_all(dir)?;
    crate::fsutil::write_atomically(&path, &id)?;
    Ok(id)
}

fn read_file_id(path: &Path) -> std::io::Result<Option<DeviceId>> {
    Ok(crate::fsutil::read_limited(path)?.and_then(|b| DeviceId::try_from(b.as_slice()).ok()))
}

fn random_id() -> std::io::Result<DeviceId> {
    let mut id = [0u8; DEVICE_ID_LEN];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut id)
        .map_err(|_| std::io::Error::other("no randomness"))?;
    Ok(id)
}

#[cfg(any(test, windows, target_os = "linux"))]
fn to_hex(id: &DeviceId) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(any(test, windows, target_os = "linux"))]
fn from_hex(text: &str) -> Option<DeviceId> {
    let text = text.trim();
    if text.len() != DEVICE_ID_LEN * 2 {
        return None;
    }
    let mut id = [0u8; DEVICE_ID_LEN];
    for (i, slot) in id.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(id)
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
    fn device_id_in_a_file_is_made_once() {
        let dir = std::env::temp_dir().join(format!("cd-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = device_id_in_file(&dir).expect("made");
        let b = device_id_in_file(&dir).expect("read back");
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn device_id_hex_round_trips() {
        let id = [0xa5u8; DEVICE_ID_LEN];
        assert_eq!(from_hex(&to_hex(&id)), Some(id));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex(&"zz".repeat(DEVICE_ID_LEN)), None);
    }

    /// Whatever the vault does on this machine (none on a CI runner without a
    /// keyring), the id stays the same for the whole run, so a reconnect is
    /// recognised.
    #[test]
    fn device_id_is_stable_within_a_run() {
        let dir = std::env::temp_dir().join(format!("cd-dev2-{}", std::process::id()));
        let a = device_id(&dir).expect("made");
        let b = device_id(&dir).expect("again");
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
