//! Certificate pinning store — trust on first use (S2, SA-6).
//!
//! The SHA-256 fingerprint of each host's TLS certificate is stored after the
//! user approves it. Later connections must present exactly the same
//! certificate; any change blocks the connection before credentials are sent.
//!
//! The file holds no secrets (fingerprints are public), but it is still written
//! atomically and, on Unix, readable only by the owner.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Current file format version.
const FORMAT_VERSION: u32 = 1;

/// SHA-256 fingerprint of a DER certificate, as upper-case hex pairs joined
/// with colons (the format Windows and OpenSSL display).
pub fn fingerprint(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    let mut out = String::with_capacity(digest.len() * 3);
    for (i, b) in digest.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        out.push_str(&format!("{b:02X}"));
    }
    out
}

/// Result of comparing a presented certificate with the stored pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCheck {
    /// Matches the pinned fingerprint.
    Match,
    /// No pin yet: the user must approve this fingerprint.
    Unknown { presented: String },
    /// Differs from the pinned fingerprint: block the connection.
    Mismatch { pinned: String, presented: String },
}

/// Errors reading or writing the pin file.
#[derive(Debug, thiserror::Error)]
pub enum PinStoreError {
    #[error("could not read or write the certificate pin file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the certificate pin file is damaged or from a newer version")]
    Format,
}

#[derive(Serialize, Deserialize, Default)]
struct PinFile {
    version: u32,
    /// Endpoint ("host:port", lower-case) -> fingerprint.
    pins: BTreeMap<String, String>,
}

/// Pinned certificate fingerprints, optionally backed by a file.
#[derive(Debug, Default)]
pub struct PinStore {
    path: Option<PathBuf>,
    pins: BTreeMap<String, String>,
}

/// Normalises an endpoint key so "Office-PC:3389" and "office-pc:3389" match.
fn key(endpoint: &str) -> String {
    endpoint.trim().to_ascii_lowercase()
}

fn is_valid_fingerprint(fp: &str) -> bool {
    fp.len() == 32 * 3 - 1
        && fp
            .split(':')
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl PinStore {
    /// An empty store kept only in memory (tests, or "don't remember").
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Opens the store at `path`. A missing file is an empty store.
    /// A damaged file is an error: it is never silently replaced, because that
    /// would make every host look new and invite the user to re-approve.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PinStoreError> {
        let path = path.into();
        let pins = match std::fs::read(&path) {
            Ok(bytes) => {
                let file: PinFile =
                    serde_json::from_slice(&bytes).map_err(|_| PinStoreError::Format)?;
                if file.version != FORMAT_VERSION
                    || !file.pins.values().all(|v| is_valid_fingerprint(v))
                {
                    return Err(PinStoreError::Format);
                }
                file.pins
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path: Some(path),
            pins,
        })
    }

    /// Compares a presented fingerprint with the pin for `endpoint`.
    pub fn check(&self, endpoint: &str, presented: &str) -> PinCheck {
        match self.pins.get(&key(endpoint)) {
            Some(pinned) if pinned == presented => PinCheck::Match,
            Some(pinned) => PinCheck::Mismatch {
                pinned: pinned.clone(),
                presented: presented.to_owned(),
            },
            None => PinCheck::Unknown {
                presented: presented.to_owned(),
            },
        }
    }

    /// The pinned fingerprint for `endpoint`, if any.
    pub fn get(&self, endpoint: &str) -> Option<&str> {
        self.pins.get(&key(endpoint)).map(String::as_str)
    }

    /// Pins (or re-pins, after the user explicitly accepts a change) a fingerprint
    /// and saves the file.
    pub fn pin(&mut self, endpoint: &str, fp: &str) -> Result<(), PinStoreError> {
        if !is_valid_fingerprint(fp) {
            return Err(PinStoreError::Format);
        }
        self.pins.insert(key(endpoint), fp.to_owned());
        self.save()
    }

    /// Removes the pin for `endpoint` (e.g. when the profile is deleted).
    pub fn forget(&mut self, endpoint: &str) -> Result<(), PinStoreError> {
        if self.pins.remove(&key(endpoint)).is_some() {
            self.save()?;
        }
        Ok(())
    }

    /// All pins as (endpoint, fingerprint), sorted by endpoint.
    pub fn entries(&self) -> Vec<(String, String)> {
        self.pins
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.pins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    fn save(&self) -> Result<(), PinStoreError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = PinFile {
            version: FORMAT_VERSION,
            pins: self.pins.clone(),
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|_| PinStoreError::Format)?;
        crate::fsutil::write_atomically(path, &json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT_A: &[u8] = b"certificate A (test bytes)";
    const CERT_B: &[u8] = b"certificate B (test bytes)";

    #[test]
    fn fingerprint_format() {
        let fp = fingerprint(CERT_A);
        assert!(is_valid_fingerprint(&fp), "{fp}");
        assert_eq!(fp, fingerprint(CERT_A));
        assert_ne!(fp, fingerprint(CERT_B));
        // SHA-256 of the empty input, a known value.
        assert!(fingerprint(b"").starts_with("E3:B0:C4:42:98:FC:1C:14"));
    }

    #[test]
    fn unknown_then_match_then_mismatch() {
        let mut s = PinStore::in_memory();
        let a = fingerprint(CERT_A);
        let b = fingerprint(CERT_B);
        assert_eq!(
            s.check("pc:3389", &a),
            PinCheck::Unknown {
                presented: a.clone()
            }
        );
        assert!(s.pin("PC:3389", &a).is_ok());
        assert_eq!(s.check("pc:3389", &a), PinCheck::Match);
        assert_eq!(
            s.check("pc:3389", &b),
            PinCheck::Mismatch {
                pinned: a.clone(),
                presented: b.clone()
            }
        );
        // Pins are per endpoint: another port is a different host.
        assert!(matches!(s.check("pc:3390", &a), PinCheck::Unknown { .. }));
    }

    #[test]
    fn rejects_malformed_fingerprints() {
        let mut s = PinStore::in_memory();
        assert!(s.pin("pc:3389", "not-a-fingerprint").is_err());
        assert!(s.is_empty());
    }

    #[test]
    fn persists_and_reloads() -> Result<(), PinStoreError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("pins.json");
        let a = fingerprint(CERT_A);
        {
            let mut s = PinStore::open(&path)?;
            assert!(s.is_empty());
            s.pin("pc:3389", &a)?;
        }
        let mut s = PinStore::open(&path)?;
        assert_eq!(s.get("pc:3389"), Some(a.as_str()));
        s.forget("pc:3389")?;
        assert!(PinStore::open(&path)?.is_empty());
        // Only the pin file remains: no temporary files left behind.
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() -> Result<(), PinStoreError> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("pins.json");
        PinStore::open(&path)?.pin("pc:3389", &fingerprint(CERT_A))?;
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        Ok(())
    }

    #[test]
    fn damaged_file_is_an_error_not_a_reset() -> Result<(), PinStoreError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("pins.json");
        std::fs::write(&path, b"{ not json")?;
        assert!(matches!(PinStore::open(&path), Err(PinStoreError::Format)));
        std::fs::write(&path, br#"{"version":1,"pins":{"pc:3389":"tampered"}}"#)?;
        assert!(matches!(PinStore::open(&path), Err(PinStoreError::Format)));
        Ok(())
    }
}
