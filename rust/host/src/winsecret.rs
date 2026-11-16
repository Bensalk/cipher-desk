//! Machine-scope DPAPI encryption (Windows), shared by the Host service and the
//! app so a custom sharing password can be handed to the service without ever
//! writing it in the clear or passing it on a command line. A value encrypted
//! here on this computer can be decrypted only on this computer, by any account
//! (so the user-level app can encrypt it and the system-level service can read
//! it).
#![allow(unsafe_code)]

use anyhow::Context as _;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
};

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(0),
        pbData: data.as_ptr().cast_mut(),
    }
}

/// Encrypts `data` with DPAPI at machine scope.
pub fn machine_protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points at `data` for its length; `out` is filled by
    // Windows and copied out, then freed with LocalFree.
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
        Ok(take(&mut out))
    }
}

/// Decrypts data produced by [`machine_protect`] on this computer.
pub fn machine_unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = blob(data);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as above.
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
        Ok(take(&mut out))
    }
}

/// Copies the bytes out of a DPAPI output blob and frees it with `LocalFree`.
///
/// # Safety
/// `out` must be an output blob filled by a DPAPI call.
unsafe fn take(out: &mut CRYPT_INTEGER_BLOB) -> Vec<u8> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    // SAFETY: Windows gave us `pbData` with `cbData` valid bytes; copy them and
    // hand the pointer back to LocalFree, as DPAPI requires.
    unsafe {
        let bytes = core::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        if !out.pbData.is_null() {
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
        bytes
    }
}

/// A password handed from the app to the service, encrypted at machine scope so
/// it is never written or passed as plain text. The app writes it with
/// [`write_pending`]; the service imports it with [`read_pending`].
pub fn write_pending(path: &std::path::Path, password: &str) -> anyhow::Result<()> {
    let sealed = machine_protect(password.as_bytes())?;
    rdpv_core::fsutil::write_atomically(path, &sealed).context("write pending password")?;
    Ok(())
}

/// Reads and decrypts a pending password written by [`write_pending`], then
/// deletes the file. Returns the plain password for immediate re-storage.
/// The file is removed whether this succeeds or not — it held a password
/// (or an attempt at one), so it should never outlive this one read, not
/// even when decryption itself fails.
pub fn read_pending(path: &std::path::Path) -> anyhow::Result<String> {
    let result = (|| -> anyhow::Result<String> {
        let sealed = std::fs::read(path).context("read pending password")?;
        let plain = machine_unprotect(&sealed)?;
        String::from_utf8(plain).context("pending password is not valid text")
    })();
    let _ = std::fs::remove_file(path);
    result
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_removes_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("pending-password.bin");
        write_pending(&path, "a password, not a real one").expect("write");
        assert!(path.exists());

        let plain = read_pending(&path).expect("read");
        assert_eq!(plain, "a password, not a real one");
        assert!(!path.exists(), "the pending file must not outlive a successful read");
    }

    #[test]
    fn a_failed_read_still_removes_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("pending-password.bin");
        // Not a real sealed blob, so decryption fails — the leftover-cleanup
        // bug this guards against left exactly this kind of file on disk.
        std::fs::write(&path, b"not a real sealed blob").expect("write");

        let err = read_pending(&path).expect_err("garbage must not decrypt");
        assert!(!path.exists(), "a failed read must not leave the file behind");
        // The error must not echo the file's content back.
        assert!(!err.to_string().contains("not a real sealed blob"));
    }
}
