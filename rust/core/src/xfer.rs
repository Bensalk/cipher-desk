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

/// Size of the DROPFILES header that starts a CF_HDROP block.
const DROPFILES_HEADER: usize = 20;

/// Builds a CF_HDROP block: the DROPFILES header (file list right after it,
/// wide characters) and the paths, each NUL-terminated, then a final NUL.
pub fn build_dropfiles(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DROPFILES_HEADER + 64 * paths.len());
    out.extend_from_slice(&(DROPFILES_HEADER as u32).to_le_bytes()); // pFiles
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide
    for p in paths {
        for u in p.to_string_lossy().encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// Reads the paths from a CF_HDROP block (wide or ANSI), at most `max`.
/// Damaged blocks give the paths read so far.
pub fn parse_dropfiles(bytes: &[u8], max: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if bytes.len() < DROPFILES_HEADER {
        return out;
    }
    let field = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let start = usize::try_from(field(0)).unwrap_or(usize::MAX);
    let wide = field(16) != 0;
    let Some(list) = bytes.get(start..) else {
        return out;
    };
    if wide {
        let units: Vec<u16> = list
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        for name in units.split(|&u| u == 0) {
            if name.is_empty() || out.len() >= max {
                break;
            }
            if let Ok(s) = String::from_utf16(name) {
                out.push(PathBuf::from(s));
            }
        }
    } else {
        for name in list.split(|&b| b == 0) {
            if name.is_empty() || out.len() >= max {
                break;
            }
            out.push(PathBuf::from(String::from_utf8_lossy(name).into_owned()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_lists_round_trip() {
        let paths = vec![
            PathBuf::from(r"C:\Users\me\Report é.pdf"),
            PathBuf::from(r"D:\data\b.xlsx"),
        ];
        let block = build_dropfiles(&paths);
        assert_eq!(parse_dropfiles(&block, 100), paths);
        assert_eq!(parse_dropfiles(&block, 1), paths[..1].to_vec());
        assert!(parse_dropfiles(&block[..10], 100).is_empty());
    }

    #[test]
    fn reads_ansi_file_lists() {
        let mut block = vec![0u8; 20];
        block[0] = 20; // pFiles
        block.extend_from_slice(b"C:\\a.txt\0C:\\b.txt\0\0");
        assert_eq!(
            parse_dropfiles(&block, 100),
            vec![PathBuf::from("C:\\a.txt"), PathBuf::from("C:\\b.txt")]
        );
    }

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
