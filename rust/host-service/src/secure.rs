//! Who may open the service's files.
//!
//! The service folder (`C:\ProgramData\Cipher Desk\host-service`) gets its own
//! permissions instead of the ones `ProgramData` hands down (where any user
//! may add files): Windows itself and administrators may change everything;
//! other people on this computer may only read the plain files (logs,
//! connection history, the public certificate, received files). The secrets
//! (sharing password, two-step key, the certificate's private key) are in a
//! `secrets` folder only Windows itself and administrators can open.
#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::MetadataExt as _;
use std::path::Path;

use anyhow::Context as _;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};

/// Folders: system and administrators full control; users read only. The
/// owner gets no extra rights (in case someone else made the folder first).
const FOLDER_SDDL: &str =
    "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;OICI;RC;;;OW)";
/// Secrets: system and administrators only.
const SECRET_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;RC;;;OW)";

/// Secrets older versions kept in the main folder; moved into `secrets`.
const OLD_SECRETS: [&str; 4] = [
    "password.bin",
    "twostep.bin",
    "twostep-pending.bin",
    "twostep-devices.txt",
];

/// Applies the folder permissions to `ProgramData\Cipher Desk` and the
/// service folder, moves secrets into the locked `secrets` folder and locks
/// the private key. Run by the service at start and by every admin command.
pub fn lock_down(dir: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dir.parent() {
        replace_link(parent)?;
        std::fs::create_dir_all(parent).context("create the Cipher Desk folder")?;
        apply(parent, FOLDER_SDDL).context("lock the Cipher Desk folder")?;
    }
    replace_link(dir)?;
    std::fs::create_dir_all(dir).context("create the service folder")?;
    apply(dir, FOLDER_SDDL).context("lock the service folder")?;

    let secrets = secrets_dir(dir)?;
    apply(&secrets, SECRET_SDDL).context("lock the secrets folder")?;
    for name in OLD_SECRETS {
        let old = dir.join(name);
        if old.exists() {
            let new = secrets.join(name);
            if new.exists() {
                let _ = std::fs::remove_file(&old);
            } else {
                std::fs::rename(&old, &new).with_context(|| format!("move {name}"))?;
            }
        }
        let new = secrets.join(name);
        if new.exists() {
            // A moved file keeps its old permissions: set them again.
            apply(&new, SECRET_SDDL).with_context(|| format!("lock {name}"))?;
        }
    }
    let key = dir.join("host_private_key.pem");
    if key.exists() {
        apply(&key, SECRET_SDDL).context("lock the private key")?;
    }
    // Older versions left results (possibly a password) and the tray's log
    // here; neither belongs in this folder any more.
    for name in ["last-result.txt", "pending-password.bin", "tray.log"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    replace_link(&dir.join("Received"))?;
    // The readable settings say whether two-step is on (older versions did
    // not write it there).
    let mut settings = crate::config::load_settings();
    let twostep = secrets.join("twostep.bin").exists();
    if settings.twostep != twostep {
        settings.twostep = twostep;
        crate::config::save_settings(&settings)?;
    }
    Ok(())
}

/// Locks a single file the same way as a secret: only Windows itself and
/// administrators may open it. Used for the `--result` file an elevated
/// admin command writes for the app to read — it can hold the sharing
/// password, so it gets the same protection as the files in `secrets`,
/// not whatever the containing folder's permissions happen to allow.
pub fn lock_result_file(path: &Path) -> anyhow::Result<()> {
    apply(path, SECRET_SDDL)
}

/// `<dir>\secrets`, created locked if missing ([`lock_down`] checks the
/// permissions of an existing one).
pub fn secrets_dir(dir: &Path) -> anyhow::Result<std::path::PathBuf> {
    let secrets = dir.join("secrets");
    replace_link(&secrets)?;
    if !secrets.exists() {
        std::fs::create_dir(&secrets).context("create the secrets folder")?;
        apply(&secrets, SECRET_SDDL).context("lock the secrets folder")?;
    }
    Ok(secrets)
}

/// True if `path` is a link (junction or symbolic link) rather than a real
/// file or folder.
pub fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x400 != 0)
}

/// Removes a link someone put where one of our folders belongs (the link
/// only, never what it points to).
fn replace_link(path: &Path) -> anyhow::Result<()> {
    if is_link(path) {
        tracing::warn!(path = %path.display(), "removing a link where a folder belongs");
        std::fs::remove_dir(path)
            .or_else(|_| std::fs::remove_file(path))
            .with_context(|| format!("remove the link at {}", path.display()))?;
    }
    Ok(())
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// Sets the owner (best effort) and a protected DACL from `sddl` on `path`.
fn apply(path: &Path, sddl: &str) -> anyhow::Result<()> {
    let path_w = wide(path.as_os_str());
    let sddl_w = wide(std::ffi::OsStr::new(sddl));
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated input; `sd` is a valid out-pointer, freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            1,
            &mut sd,
            None,
        )
    }
    .context("security descriptor")?;

    let result = (|| -> anyhow::Result<()> {
        let mut present = BOOL::default();
        let mut defaulted = BOOL::default();
        let mut dacl: *mut ACL = core::ptr::null_mut();
        // SAFETY: `sd` is the descriptor made above; valid out-pointers.
        unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) }
            .context("read the DACL")?;
        let mut owner = PSID::default();
        let mut owner_defaulted = BOOL::default();
        // SAFETY: as above.
        unsafe { GetSecurityDescriptorOwner(sd, &mut owner, &mut owner_defaulted) }
            .context("read the owner")?;

        // Owner first (Administrators), so a folder someone else made
        // stops being theirs. Best effort: not every account may do this.
        // SAFETY: NUL-terminated path; `owner` points into `sd`, alive here.
        let _ = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(path_w.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(owner),
                None,
                None,
                None,
            )
        };
        // SAFETY: NUL-terminated path; `dacl` points into `sd`, alive here.
        let err = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(path_w.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl.cast_const()),
                None,
            )
        };
        if err.0 != 0 {
            anyhow::bail!("set permissions on {}: error {}", path.display(), err.0);
        }
        Ok(())
    })();

    // SAFETY: `sd` was allocated by Windows with LocalAlloc.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    result
}
