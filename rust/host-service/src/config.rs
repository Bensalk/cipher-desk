//! Where the service keeps its settings and sharing password.
//!
//! The service runs as the machine's system account, which cannot read a
//! password saved in your personal Windows vault. So it keeps its **own**
//! sharing password in a machine-wide folder under `ProgramData`. Secrets
//! live in its `secrets` folder, which only Windows itself and administrators
//! can open (see [`crate::secure`]), and are also encrypted with Windows
//! DPAPI. The certificate the viewer pins lives in the main folder.

use std::path::PathBuf;

use anyhow::Context as _;
use rdpv_core::secret::SecretString;

/// Default sharing port (same as the in-session Share screen).
pub const DEFAULT_PORT: u16 = 3395;
/// Default viewer sign-in name.
pub const DEFAULT_USERNAME: &str = "cipher";

/// `C:\ProgramData\Cipher Desk\host-service` (created if needed).
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let base = std::env::var_os("ProgramData").context("ProgramData is not set")?;
    let dir = PathBuf::from(base).join("Cipher Desk").join("host-service");
    std::fs::create_dir_all(&dir).context("create the service folder")?;
    Ok(dir)
}

fn secret_file(name: &str) -> anyhow::Result<PathBuf> {
    Ok(crate::secure::secrets_dir(&data_dir()?)?.join(name))
}

fn password_file() -> anyhow::Result<PathBuf> {
    secret_file("password.bin")
}

fn config_file() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("config.txt"))
}

/// The service's settings (not secret; readable by everyone on this PC).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub username: String,
    pub port: u16,
    /// "Ask to connect" (no password; the person here allows it). Off unless
    /// turned on.
    pub ask: bool,
    /// Accept connections only over Tailscale (and from this PC itself).
    pub tailnet_only: bool,
    /// Two-step verification is on (the key itself is in `secrets`).
    pub twostep: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: DEFAULT_USERNAME.to_owned(),
            port: DEFAULT_PORT,
            ask: false,
            tailnet_only: true,
            twostep: false,
        }
    }
}

/// Reads settings from `config.txt`-style text (defaults for anything
/// missing or damaged).
pub fn parse_settings(text: &str) -> Settings {
    let mut s = Settings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "username" if !value.is_empty() => s.username = value.to_owned(),
            "port" => {
                if let Ok(p) = value.parse::<u16>() {
                    if p != 0 {
                        s.port = p;
                    }
                }
            }
            "ask" => s.ask = value == "on",
            "networks" => s.tailnet_only = value != "any",
            "twostep" => s.twostep = value == "on",
            _ => {}
        }
    }
    s
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// The text written to `config.txt`.
pub fn settings_text(s: &Settings) -> String {
    format!(
        "username={}\nport={}\nask={}\nnetworks={}\ntwostep={}\n",
        s.username,
        s.port,
        on_off(s.ask),
        if s.tailnet_only { "tailscale" } else { "any" },
        on_off(s.twostep),
    )
}

/// The saved settings (defaults if the file is missing or damaged).
pub fn load_settings() -> Settings {
    config_file()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_settings(&t))
        .unwrap_or_default()
}

/// Saves the settings (not secret).
pub fn save_settings(s: &Settings) -> anyhow::Result<()> {
    std::fs::write(config_file()?, settings_text(s)).context("write the service settings")?;
    Ok(())
}

/// Reads the sharing password, if one is set.
pub fn load_password() -> anyhow::Result<Option<SecretString>> {
    let path = password_file()?;
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    let plain = rdpv_host::winsecret::machine_unprotect(&bytes)?;
    let text = String::from_utf8(plain).context("the saved password is damaged")?;
    Ok(Some(SecretString::new(text)))
}

/// Saves (or replaces) the sharing password, machine-encrypted.
pub fn save_password(password: &SecretString) -> anyhow::Result<()> {
    let bytes = rdpv_host::winsecret::machine_protect(password.expose().as_bytes())?;
    std::fs::write(password_file()?, bytes).context("write the password")?;
    Ok(())
}

/// Makes sure a password exists; returns it, generating a strong one if needed.
pub fn ensure_password() -> anyhow::Result<SecretString> {
    if let Some(p) = load_password()? {
        return Ok(p);
    }
    let p = rdpv_host::generate_password()?;
    save_password(&p)?;
    Ok(p)
}

// ------------------------------------------------- two-step verification

fn twostep_file() -> anyhow::Result<PathBuf> {
    secret_file("twostep.bin")
}

fn twostep_pending_file() -> anyhow::Result<PathBuf> {
    secret_file("twostep-pending.bin")
}

/// Devices remembered after a correct code (hashes and expiry dates only).
pub fn twostep_devices_file() -> anyhow::Result<PathBuf> {
    secret_file("twostep-devices.txt")
}

/// Records in the (readable) settings whether two-step is on, so the app can
/// show it without opening the secrets.
fn set_twostep_flag(on: bool) -> anyhow::Result<()> {
    let mut s = load_settings();
    s.twostep = on;
    save_settings(&s)
}

fn load_secret(path: &std::path::Path) -> Option<rdpv_host::totp::Totp> {
    let bytes = std::fs::read(path).ok()?;
    let plain = rdpv_host::winsecret::machine_unprotect(&bytes).ok()?;
    Some(rdpv_host::totp::Totp::new(plain))
}

fn save_secret(path: &std::path::Path, totp: &rdpv_host::totp::Totp) -> anyhow::Result<()> {
    let bytes = rdpv_host::winsecret::machine_protect(totp.secret())?;
    std::fs::write(path, bytes).context("write the two-step secret")
}

/// The active two-step secret, if two-step verification is on.
pub fn load_twostep() -> Option<rdpv_host::totp::Totp> {
    twostep_file().ok().and_then(|p| load_secret(&p))
}

/// A secret being set up (shown to the user, not yet confirmed with a code).
pub fn load_twostep_pending() -> Option<rdpv_host::totp::Totp> {
    twostep_pending_file().ok().and_then(|p| load_secret(&p))
}

pub fn save_twostep_pending(totp: &rdpv_host::totp::Totp) -> anyhow::Result<()> {
    save_secret(&twostep_pending_file()?, totp)
}

/// Makes the pending secret the active one; forgets remembered devices.
pub fn activate_twostep(totp: &rdpv_host::totp::Totp) -> anyhow::Result<()> {
    save_secret(&twostep_file()?, totp)?;
    let _ = std::fs::remove_file(twostep_pending_file()?);
    let _ = std::fs::remove_file(twostep_devices_file()?);
    set_twostep_flag(true)
}

/// Turns two-step verification off and forgets remembered devices.
pub fn remove_twostep() -> anyhow::Result<()> {
    for f in [
        twostep_file()?,
        twostep_pending_file()?,
        twostep_devices_file()?,
    ] {
        let _ = std::fs::remove_file(f);
    }
    set_twostep_flag(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_files_get_the_safe_defaults() {
        let s = parse_settings("username=bob\nport=4000\n");
        assert_eq!(s.username, "bob");
        assert_eq!(s.port, 4000);
        assert!(!s.ask, "ask to connect is off unless turned on");
        assert!(s.tailnet_only, "Tailscale only unless turned off");
    }

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            username: "x".into(),
            port: 5000,
            ask: true,
            tailnet_only: false,
            twostep: true,
        };
        assert_eq!(parse_settings(&settings_text(&s)), s);
    }
}
