//! Portable mode (SA-10): when the app runs from a removable drive, its data
//! (connections, settings, certificate pins — never passwords) is kept next to
//! the program instead of in the user's profile.
//!
//! - **Windows portable ZIP:** a file named [`MARKER`] next to the `.exe` turns it
//!   on; data goes to the `data` folder beside it. The installed (MSI) copy has
//!   no marker, so it uses the normal per-user folders.
//! - **Linux AppImage:** a folder named `<AppImage file name>.data` next to the
//!   AppImage turns it on (AppImages run from a read-only image, so the folder
//!   must be outside it). Without that folder the normal folders are used.
//!
//! Passwords are never stored in these folders; they go to the current
//! computer's vault only (or are typed each time).

use std::path::{Path, PathBuf};

/// File next to the Windows `.exe` that turns portable mode on.
pub const MARKER: &str = "portable.txt";
/// Data folder next to the `.exe` in portable mode.
pub const DATA_DIR: &str = "data";

/// The portable data folder, or `None` for a normal installation.
/// `exe` is the running program; `appimage` is the `APPIMAGE` path set by the
/// AppImage runtime, if any.
pub fn data_dir(exe: &Path, appimage: Option<&Path>) -> Option<PathBuf> {
    if let Some(image) = appimage.filter(|p| p.is_absolute()) {
        let mut name = image.file_name()?.to_os_string();
        name.push(".data");
        let dir = image.with_file_name(name);
        return dir.is_dir().then_some(dir);
    }
    let folder = exe.parent()?;
    folder.join(MARKER).is_file().then(|| folder.join(DATA_DIR))
}

/// [`data_dir`] for the running program.
pub fn current() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    data_dir(&exe, appimage.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_copy_is_not_portable() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let exe = dir.path().join("cipher_desk.exe");
        std::fs::write(&exe, b"")?;
        assert_eq!(data_dir(&exe, None), None);
        Ok(())
    }

    #[test]
    fn marker_next_to_the_exe_turns_portable_on() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let exe = dir.path().join("cipher_desk.exe");
        std::fs::write(&exe, b"")?;
        std::fs::write(dir.path().join(MARKER), b"")?;
        assert_eq!(data_dir(&exe, None), Some(dir.path().join(DATA_DIR)));
        // A folder named like the marker does not count.
        std::fs::remove_file(dir.path().join(MARKER))?;
        std::fs::create_dir(dir.path().join(MARKER))?;
        assert_eq!(data_dir(&exe, None), None);
        Ok(())
    }

    #[test]
    fn appimage_uses_a_data_folder_next_to_it_only_if_present() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let image = dir.path().join("Cipher_Desk-x86_64.AppImage");
        std::fs::write(&image, b"")?;
        let exe = Path::new("/tmp/.mount_abc/usr/bin/cipher_desk");
        assert_eq!(data_dir(exe, Some(&image)), None);
        let data = dir.path().join("Cipher_Desk-x86_64.AppImage.data");
        std::fs::create_dir(&data)?;
        assert_eq!(data_dir(exe, Some(&image)), Some(data));
        // A relative APPIMAGE value is ignored (not set by the AppImage runtime).
        assert_eq!(data_dir(exe, Some(Path::new("x.AppImage"))), None);
        Ok(())
    }
}
