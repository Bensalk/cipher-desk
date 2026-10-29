//! Safe file writes for app data (profiles, settings, certificate pins).
//!
//! - Atomic: write a temporary file in the same folder, flush it to disk, then
//!   rename it over the target, so a crash never leaves a half-written file.
//! - Private: on Unix the files are readable only by the owner (0600).
//! - Backed up: [`write_with_backup`] keeps the previous good version as
//!   `<name>.bak`, which loaders use if the main file is damaged.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Largest app data file we read (protects against huge or hostile files).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Path of the backup kept next to `path`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Copies the current file (if any) to `<name>.bak`, then writes atomically.
pub fn write_with_backup(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        let bak = backup_path(path);
        let current = std::fs::read(path)?;
        write_atomically(&bak, &current)?;
    }
    write_atomically(path, data)
}

/// Reads a file, refusing anything larger than [`MAX_FILE_BYTES`].
/// `Ok(None)` if it does not exist.
pub fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_BYTES => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
        )),
        Ok(_) => std::fs::read(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Moves a damaged file aside as `<name>.damaged-<unix seconds>` so it is kept
/// for inspection but no longer used.
pub fn set_aside(path: &Path) -> std::io::Result<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".damaged-{secs}"));
    let target = path.with_file_name(name);
    std::fs::rename(path, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_keeps_previous_version() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("data.json");
        write_with_backup(&p, b"one")?;
        assert!(!backup_path(&p).exists());
        write_with_backup(&p, b"two")?;
        assert_eq!(std::fs::read(&p)?, b"two");
        assert_eq!(std::fs::read(backup_path(&p))?, b"one");
        assert!(!p.with_extension("tmp").exists());
        Ok(())
    }

    #[test]
    fn read_limits_size_and_handles_missing() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("x.json");
        assert_eq!(read_limited(&p)?, None);
        std::fs::write(&p, vec![b' '; (MAX_FILE_BYTES + 1) as usize])?;
        assert!(read_limited(&p).is_err());
        Ok(())
    }

    #[test]
    fn set_aside_moves_the_file() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("x.json");
        std::fs::write(&p, b"broken")?;
        let moved = set_aside(&p)?;
        assert!(!p.exists());
        assert_eq!(std::fs::read(moved)?, b"broken");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("x.json");
        write_with_backup(&p, b"a")?;
        write_with_backup(&p, b"b")?;
        for f in [&p, &backup_path(&p)] {
            assert_eq!(std::fs::metadata(f)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }
}
