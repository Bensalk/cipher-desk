//! Shared helpers for copying files over the clipboard channel (Share screen).
//!
//! RDP moves files on the same channel as clipboard text: one side advertises a
//! file list, the other asks for each file's bytes. These helpers are the parts
//! both the viewer and the Host need — building the list of files to offer,
//! turning a received name into a safe local name, and the size limit — so the
//! two sides agree. The actual send/receive loops live with each side.

use std::path::{Path, PathBuf};

use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, ClipboardGeneralCapabilityFlags, FileDescriptor,
};

/// Largest single file accepted or sent (100 MiB). Transfers are held in memory
/// a file at a time, and this is meant for documents, not disk images.
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Capabilities to advertise so both plain text and file streams work. Long
/// format names carry "FileGroupDescriptorW"; the paths flag keeps the source
/// folder off the wire.
pub fn file_caps() -> ClipboardGeneralCapabilityFlags {
    ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
        | ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
        | ClipboardGeneralCapabilityFlags::FILECLIP_NO_FILE_PATHS
}

/// A file lined up to send: the descriptor for the wire, the real path to read
/// from, and its size.
pub struct Outgoing {
    pub descriptor: FileDescriptor,
    pub path: PathBuf,
    pub size: u64,
}

/// Builds the offer for a set of local paths. Files that are missing, are not
/// regular files, or are larger than [`MAX_FILE_BYTES`] are left out. Returns
/// an empty list if nothing qualifies.
pub fn offer(paths: &[PathBuf]) -> Vec<Outgoing> {
    let mut out = Vec::new();
    for p in paths {
        let Ok(meta) = std::fs::metadata(p) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        if size > MAX_FILE_BYTES {
            continue;
        }
        let name = match p.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => continue,
        };
        let descriptor = FileDescriptor::new(name)
            .with_file_size(size)
            .with_attributes(ClipboardFileAttributes::ARCHIVE);
        out.push(Outgoing {
            descriptor,
            path: p.clone(),
            size,
        });
    }
    out
}

/// Turns a name received from the other side into a safe base file name, or
/// `None` if it is unusable. Strips any directory part (both `/` and `\`),
/// rejects `.`/`..`, empty names, Windows device names, and control characters.
pub fn safe_file_name(raw: &str) -> Option<String> {
    // Keep only the last path component, however it was separated.
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
    if base.is_empty() || base == "." || base == ".." {
        return None;
    }
    if base.chars().any(|c| c.is_control()) {
        return None;
    }
    // Reserved Windows device names (CON, NUL, COM1, …) are unsafe to create.
    if ironrdp_cliprdr::is_windows_device_name(base) {
        return None;
    }
    Some(base.to_owned())
}

/// A path under `dir` for `name` that does not clash with an existing file:
/// `report.pdf`, then `report (1).pdf`, and so on.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_owned();
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1..10_000 {
        let next = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(&next);
        if !candidate.exists() {
            return candidate;
        }
    }
    // Extremely unlikely; fall back to the plain name.
    dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_names() {
        assert_eq!(safe_file_name("notes.txt").as_deref(), Some("notes.txt"));
        assert_eq!(
            safe_file_name(r"C:\secret\pay.xlsx").as_deref(),
            Some("pay.xlsx")
        );
        assert_eq!(safe_file_name("a/b/c.pdf").as_deref(), Some("c.pdf"));
        assert_eq!(safe_file_name(".."), None);
        assert_eq!(safe_file_name(""), None);
        assert_eq!(safe_file_name("   "), None);
        assert_eq!(safe_file_name("CON"), None);
        assert_eq!(safe_file_name("a\u{0007}b"), None);
    }

    #[test]
    fn unique_path_on_empty_dir_is_plain() {
        let dir = std::env::temp_dir().join(format!("cd-xfer-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = unique_path(&dir, "x.txt");
        assert_eq!(p, dir.join("x.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
