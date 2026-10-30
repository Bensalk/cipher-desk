//! Where the service keeps its settings and sharing password.
//!
//! The service runs as the machine's system account, which cannot read a
//! password saved in your personal Windows vault. So it keeps its **own**
//! sharing password, in a machine-wide folder under `ProgramData`, encrypted
//! with Windows DPAPI at machine scope (only this computer can read it). The
//! certificate the viewer pins lives in the same folder.
#![allow(unsafe_code)]

use std::path::PathBuf;

use anyhow::Context as _;
use rdpv_core::secret::SecretString;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
};

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
    let plain = unprotect(&bytes)?;
    let text = String::from_utf8(plain).context("the saved password is damaged")?;
    Ok(Some(SecretString::new(text)))
}

/// Saves (or replaces) the sharing password, machine-encrypted.
pub fn save_password(password: &SecretString) -> anyhow::Result<()> {
    let bytes = protect(password.expose().as_bytes())?;
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

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(0),
        pbData: data.as_ptr().cast_mut(),
    }
}

/// DPAPI machine-scope encrypt.
fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points at `data` for the length given; `out` is owned by
    // Windows after the call and freed below with LocalFree via from_raw copy.
    unsafe {
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut out,
        )
        .map_err(|e| anyhow::anyhow!("protect: {e}"))?;
        let copied = slice_and_free(&mut out);
        Ok(copied)
    }
}

/// DPAPI decrypt.
fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as above; the output blob is copied out and then freed.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut out,
        )
        .map_err(|e| anyhow::anyhow!("unprotect: {e}"))?;
        Ok(slice_and_free(&mut out))
    }
}

/// Copies the bytes out of a DPAPI output blob and frees it with `LocalFree`.
///
/// # Safety
/// `out` must be an output blob filled by a DPAPI call.
unsafe fn slice_and_free(out: &mut CRYPT_INTEGER_BLOB) -> Vec<u8> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    // SAFETY: Windows gave us `pbData` with `cbData` valid bytes; we copy them
    // and hand the pointer back to LocalFree, as DPAPI requires.
    unsafe {
        let bytes = core::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        if !out.pbData.is_null() {
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
        bytes
    }
}
