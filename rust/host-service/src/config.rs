//! Where the service keeps its settings and sharing password.
//!
//! The service runs as the machine's system account, which cannot read a
//! password saved in your personal Windows vault. So it keeps its **own**
//! sharing password, in a machine-wide folder under `ProgramData`, encrypted
//! with Windows DPAPI at machine scope (only this computer can read it). The
//! certificate the viewer pins lives in the same folder.

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

fn password_file() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("password.bin"))
}

fn config_file() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("config.txt"))
}

/// The saved username and port (defaults if the file is missing or damaged).
pub fn load_settings() -> (String, u16) {
    let text = config_file()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let mut username = DEFAULT_USERNAME.to_owned();
    let mut port = DEFAULT_PORT;
    if let Some(text) = text {
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("username=") {
                if !v.trim().is_empty() {
                    username = v.trim().to_owned();
                }
            } else if let Some(v) = line.strip_prefix("port=") {
                if let Ok(p) = v.trim().parse::<u16>() {
                    if p != 0 {
                        port = p;
                    }
                }
            }
        }
    }
    (username, port)
}

/// Saves the username and port (not secret).
pub fn save_settings(username: &str, port: u16) -> anyhow::Result<()> {
    std::fs::write(
        config_file()?,
        format!("username={username}\nport={port}\n"),
    )
    .context("write the service settings")?;
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
    Ok(data_dir()?.join("twostep.bin"))
}

fn twostep_pending_file() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("twostep-pending.bin"))
}

/// Devices remembered after a correct code (hashes and expiry dates only).
pub fn twostep_devices_file() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("twostep-devices.txt"))
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
    Ok(())
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
    Ok(())
}
