//! Two-step verification on the Host (Google Authenticator and similar apps).
//!
//! - [`Totp`]: the standard time-based one-time codes (RFC 6238, SHA-1,
//!   6 digits, 30-second steps), the kind Google Authenticator shows.
//! - [`Gate`]: decides whether a viewer that already passed the password may
//!   in: a remembered device (30 days after a correct code), or a correct code
//!   now. Too many wrong codes lock code entry for a while.
//!
//! The viewer's device id and code arrive in a cookie in the sign-in data,
//! see `rdpv_core::second_factor`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rdpv_core::second_factor::SecondFactor;

/// Seconds each code is valid for (what authenticator apps use).
const STEP: u64 = 30;
/// Codes one step early or late are accepted too (phone clock drift, typing).
const DRIFT_STEPS: u64 = 1;
/// How long a device that entered a correct code is remembered.
pub const REMEMBER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Wrong codes allowed in [`FAIL_WINDOW`] before code entry is locked.
const MAX_FAILS: usize = 5;
const FAIL_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// A shared secret for time-based codes.
#[derive(Clone)]
pub struct Totp {
    secret: Vec<u8>,
}

impl Totp {
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    /// A new random secret (20 bytes, as Google Authenticator expects).
    pub fn generate() -> anyhow::Result<Self> {
        let mut secret = vec![0u8; 20];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut secret)
            .map_err(|_| anyhow::anyhow!("no randomness"))?;
        Ok(Self { secret })
    }

    pub fn secret(&self) -> &[u8] {
        &self.secret
    }

    /// The setup key to type into the authenticator app (Base32, in groups
    /// of four for reading).
    pub fn setup_key(&self) -> String {
        let plain = base32_encode(&self.secret);
        plain
            .as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The `otpauth://` link apps read from a QR code.
    pub fn uri(&self, account: &str) -> String {
        let enc = |s: &str| {
            s.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                        char::from(b).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        };
        format!(
            "otpauth://totp/Cipher%20Desk:{}?secret={}&issuer=Cipher%20Desk&digits=6&period={STEP}",
            enc(account),
            base32_encode(&self.secret)
        )
    }

    /// The QR code of [`Totp::uri`], as rows of `1` (dark) and `0` (light),
    /// for the app to draw. `None` if it cannot be made.
    pub fn qr_rows(&self, account: &str) -> Option<Vec<String>> {
        let code = qrcode::QrCode::with_error_correction_level(
            self.uri(account).as_bytes(),
            qrcode::EcLevel::M,
        )
        .ok()?;
        let width = code.width();
        let colors = code.to_colors();
        Some(
            colors
                .chunks(width)
                .map(|row| {
                    row.iter()
                        .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
                        .collect()
                })
                .collect(),
        )
    }

    /// The 6-digit code for one 30-second step.
    pub fn code_at_step(&self, step: u64) -> String {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &self.secret);
        let tag = ring::hmac::sign(&key, &step.to_be_bytes());
        let h = tag.as_ref();
        let offset = usize::from(h[h.len() - 1] & 0x0f);
        let value = u32::from_be_bytes([
            h[offset] & 0x7f,
            h[offset + 1],
            h[offset + 2],
            h[offset + 3],
        ]);
        format!("{:06}", value % 1_000_000)
    }

    /// Whether `code` is right at `now` (one step of drift either way).
    pub fn verify(&self, code: &str, now: SystemTime) -> bool {
        let code = code.trim();
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let step = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) / STEP;
        let lo = step.saturating_sub(DRIFT_STEPS);
        (lo..=step + DRIFT_STEPS)
            .any(|s| constant_time_eq(self.code_at_step(s).as_bytes(), code.as_bytes()))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 Base32 without padding (what authenticator apps take).
pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in data {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(BASE32[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

/// Where the gate gets the current secret; `None` = two-step verification
/// is off (everyone with the password gets in).
pub type SecretSource = Box<dyn Fn() -> Option<Totp> + Send + Sync>;

/// Decides whether a viewer that passed the password may in.
pub struct Gate {
    totp: SecretSource,
    /// File of remembered devices: one `hex(sha256(device id)) expiry-secs`
    /// per line. Only hashes are kept.
    store: PathBuf,
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    fails: Vec<SystemTime>,
    locked_until: Option<SystemTime>,
}

impl Gate {
    /// A gate with a fixed secret.
    pub fn new(totp: Totp, store: PathBuf) -> Self {
        Self::with_source(Box::new(move || Some(totp.clone())), store)
    }

    /// A gate that reads the secret at each sign-in, so turning two-step
    /// verification on or off applies at once. `source` returning `None`
    /// means it is off.
    pub fn with_source(source: SecretSource, store: PathBuf) -> Self {
        Self {
            totp: source,
            store,
            state: Mutex::new(GateState::default()),
        }
    }

    /// `cookie`: the viewer's sign-in cookie, if any. `true` lets it in.
    pub fn check(&self, cookie: Option<&[u8]>) -> bool {
        self.check_at(cookie, SystemTime::now())
    }

    fn check_at(&self, cookie: Option<&[u8]>, now: SystemTime) -> bool {
        let Some(totp) = (self.totp)() else {
            return true; // two-step verification is off
        };
        let Some(factor) = cookie.and_then(SecondFactor::decode) else {
            return false;
        };
        let device = device_key(&factor.device);
        let mut remembered = self.load();
        remembered.retain(|_, expiry| *expiry > now);
        if remembered.contains_key(&device) {
            return true;
        }
        let Some(code) = factor.code else {
            return false;
        };
        let Ok(mut st) = self.state.lock() else {
            return false;
        };
        if st.locked_until.is_some_and(|t| now < t) {
            tracing::warn!("authenticator code refused: too many wrong codes");
            return false;
        }
        if totp.verify(&code, now) {
            st.fails.clear();
            remembered.insert(device, now + REMEMBER);
            self.save(&remembered);
            true
        } else {
            st.fails
                .retain(|t| now.duration_since(*t).unwrap_or_default() < FAIL_WINDOW);
            st.fails.push(now);
            if st.fails.len() >= MAX_FAILS {
                st.fails.clear();
                st.locked_until = Some(now + LOCKOUT);
            }
            false
        }
    }

    fn load(&self) -> HashMap<String, SystemTime> {
        let text = std::fs::read_to_string(&self.store).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let (k, v) = l.split_once(' ')?;
                let secs: u64 = v.trim().parse().ok()?;
                Some((k.to_owned(), UNIX_EPOCH + Duration::from_secs(secs)))
            })
            .collect()
    }

    fn save(&self, map: &HashMap<String, SystemTime>) {
        let text: String = map
            .iter()
            .map(|(k, t)| {
                let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                format!("{k} {secs}\n")
            })
            .collect();
        if let Err(e) = rdpv_core::fsutil::write_atomically(&self.store, text.as_bytes()) {
            tracing::warn!(error = %e, "remembered devices could not be saved");
        }
    }
}

impl ironrdp_server::CookieCheck for Gate {
    fn check(&self, cookie: Option<&[u8]>) -> bool {
        Gate::check(self, cookie)
    }
}

fn device_key(device: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, device)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use rdpv_core::second_factor::DEVICE_ID_LEN;

    /// RFC 6238 Appendix B, SHA-1 (last 6 of the 8 digits).
    #[test]
    fn matches_the_rfc_test_values() {
        let t = Totp::new(b"12345678901234567890".to_vec());
        for (time, code) in [
            (59u64, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(t.code_at_step(time / STEP), code, "at {time}");
            assert!(t.verify(code, UNIX_EPOCH + Duration::from_secs(time)));
        }
    }

    #[test]
    fn makes_a_square_qr_code() {
        let t = Totp::new(vec![1; 20]);
        let rows = t.qr_rows("DESKTOP-PC").expect("qr");
        assert!(rows.len() >= 21);
        assert!(rows.iter().all(|r| r.len() == rows.len()));
        // The top-left finder pattern starts with seven dark modules.
        assert!(rows[0].starts_with("1111111"));
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_encode(b"f"), "MY");
    }

    #[test]
    fn accepts_one_step_of_drift_only() {
        let t = Totp::new(vec![9; 20]);
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let step = 1_700_000_000 / STEP;
        assert!(t.verify(&t.code_at_step(step - 1), now));
        assert!(t.verify(&t.code_at_step(step + 1), now));
        assert!(!t.verify(&t.code_at_step(step - 3), now));
        assert!(!t.verify("12345", now));
    }

    fn cookie(device: u8, code: Option<String>) -> Vec<u8> {
        SecondFactor {
            device: [device; DEVICE_ID_LEN],
            code,
        }
        .encode()
    }

    #[test]
    fn gate_remembers_a_device_for_30_days() {
        let dir = tempfile::tempdir().expect("dir");
        let totp = Totp::new(vec![3; 20]);
        let gate = Gate::new(totp.clone(), dir.path().join("devices.txt"));
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let code = totp.code_at_step(1_800_000_000 / STEP);

        assert!(!gate.check_at(None, now), "no cookie: refused");
        assert!(!gate.check_at(Some(&cookie(1, None)), now), "no code yet");
        assert!(
            gate.check_at(Some(&cookie(1, Some(code))), now),
            "right code"
        );
        let later = now + Duration::from_secs(29 * 24 * 3600);
        assert!(gate.check_at(Some(&cookie(1, None)), later), "remembered");
        assert!(
            !gate.check_at(Some(&cookie(2, None)), later),
            "another device"
        );
        let expired = now + REMEMBER + Duration::from_secs(1);
        assert!(
            !gate.check_at(Some(&cookie(1, None)), expired),
            "forgotten after 30 days"
        );
    }

    #[test]
    fn gate_is_open_while_two_step_is_off() {
        let dir = tempfile::tempdir().expect("dir");
        let gate = Gate::with_source(Box::new(|| None), dir.path().join("devices.txt"));
        assert!(gate.check(None));
    }

    #[test]
    fn gate_locks_after_wrong_codes() {
        let dir = tempfile::tempdir().expect("dir");
        let totp = Totp::new(vec![4; 20]);
        let gate = Gate::new(totp.clone(), dir.path().join("devices.txt"));
        let now = UNIX_EPOCH + Duration::from_secs(1_900_000_000);
        let right = totp.code_at_step(1_900_000_000 / STEP);
        let wrong = if right == "000000" {
            "111111"
        } else {
            "000000"
        };
        for _ in 0..MAX_FAILS {
            assert!(!gate.check_at(Some(&cookie(5, Some(wrong.into()))), now));
        }
        assert!(
            !gate.check_at(Some(&cookie(5, Some(right.clone()))), now),
            "locked"
        );
        let after = now + LOCKOUT + Duration::from_secs(STEP * 2);
        let right_then = totp.code_at_step((1_900_000_000 + LOCKOUT.as_secs() + STEP * 2) / STEP);
        assert!(
            gate.check_at(Some(&cookie(5, Some(right_then))), after),
            "unlocked again"
        );
    }
}
