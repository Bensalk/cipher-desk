//! Shared helpers for copying files over the clipboard channel (Share screen).
//!
//! RDP moves files on the same channel as clipboard text: one side advertises a
//! file list, the other asks for each file's bytes. These helpers are the parts
//! both the viewer and the Host need — building the list of files to offer,
//! turning a received name into a safe local name, and the size limit — so the
//! two sides agree. The actual send/receive loops live with each side.

use std::path::{Path, PathBuf};

use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, ClipboardGeneralCapabilityFlags, FileContentsFlags,
    FileContentsRequest, FileDescriptor,
};

/// Most bytes sent or asked for in one piece. Files of any size travel as a
/// series of pieces written straight to disk, so memory use stays small.
pub const CHUNK: u32 = 4 * 1024 * 1024;

/// Most bytes answered for one request (a peer may ask for more than
/// [`CHUNK`]; it then simply asks again for the rest).
pub const MAX_SERVE: u64 = 8 * 1024 * 1024;

/// Most files and folders in one copy.
pub const MAX_ITEMS: usize = 20_000;

/// Longest name with its folder path that the protocol carries (259 UTF-16
/// characters plus the terminator).
const MAX_WIRE_NAME: usize = 259;

/// Capabilities to advertise so both plain text and file streams work. Long
/// format names carry "FileGroupDescriptorW"; the paths flag keeps the source
/// folder off the wire.
pub fn file_caps() -> ClipboardGeneralCapabilityFlags {
    ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
        | ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
        | ClipboardGeneralCapabilityFlags::FILECLIP_NO_FILE_PATHS
}

/// A file or folder lined up to send: the descriptor for the wire (name, the
/// folder path inside the copy, size), the real path, and the size (0 for a
/// folder).
pub struct Outgoing {
    pub descriptor: FileDescriptor,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
}

/// Builds the offer for a set of copied files and folders. Folders are sent
/// with everything inside them (each folder listed before its contents).
/// Missing items, links and names too long for the protocol are left out;
/// at most [`MAX_ITEMS`] entries.
pub fn offer(paths: &[PathBuf]) -> Vec<Outgoing> {
    let mut out = Vec::new();
    for p in paths {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        add_entry(&mut out, p, name, None);
    }
    out
}

fn add_entry(out: &mut Vec<Outgoing>, path: &Path, name: &str, relative: Option<&str>) {
    if out.len() >= MAX_ITEMS {
        return;
    }
    // symlink_metadata: links are not followed (no loops, nothing outside).
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    let wire_len =
        relative.map_or(0, |r| r.encode_utf16().count() + 1) + name.encode_utf16().count();
    if wire_len > MAX_WIRE_NAME {
        return;
    }
    let with_path = |d: FileDescriptor| match relative {
        Some(r) => d.with_relative_path(r.to_owned()),
        None => d,
    };
    if meta.is_file() {
        let size = meta.len();
        out.push(Outgoing {
            descriptor: with_path(
                FileDescriptor::new(name)
                    .with_file_size(size)
                    .with_attributes(ClipboardFileAttributes::ARCHIVE),
            ),
            path: path.to_path_buf(),
            size,
            is_dir: false,
        });
    } else if meta.is_dir() {
        out.push(Outgoing {
            descriptor: with_path(
                FileDescriptor::new(name)
                    .with_file_size(0)
                    .with_attributes(ClipboardFileAttributes::DIRECTORY),
            ),
            path: path.to_path_buf(),
            size: 0,
            is_dir: true,
        });
        let inner = match relative {
            Some(r) => format!("{r}\\{name}"),
            None => name.to_owned(),
        };
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<_> = entries.filter_map(Result::ok).collect();
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let child_name = child.file_name();
            let Some(child_name) = child_name.to_str() else {
                continue;
            };
            add_entry(out, &child.path(), child_name, Some(&inner));
        }
    }
}

/// Reads up to `len` bytes (at most [`MAX_SERVE`]) of `path` from `position`,
/// to answer a request for a piece of a file this side offered.
pub fn read_range(path: &Path, position: u64, len: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(position)).ok()?;
    let cap = len.min(MAX_SERVE);
    let mut buf = vec![0u8; usize::try_from(cap).ok()?];
    let mut read = 0usize;
    while read < buf.len() {
        match f.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
    }
    buf.truncate(read);
    Some(buf)
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

/// One file or folder the other side offers (name and folder path already
/// made safe by the clipboard library).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteItem {
    pub name: String,
    /// Folder path inside the copy, `\\`-separated; `None` at the top.
    pub relative_path: Option<String>,
    /// `None` if the sender did not say.
    pub size: Option<u64>,
    pub is_dir: bool,
}

impl RemoteItem {
    pub fn from_descriptor(d: &FileDescriptor) -> Self {
        Self {
            name: d.name.clone(),
            relative_path: d.relative_path.clone().filter(|p| !p.is_empty()),
            size: d.file_size,
            is_dir: d
                .attributes
                .is_some_and(|a| a.contains(ClipboardFileAttributes::DIRECTORY)),
        }
    }
}

/// What to do next while receiving.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Ask the other side for this piece.
    Request(FileContentsRequest),
    /// Everything has arrived (or was skipped); these are the top-level files
    /// and folders written, for the clipboard.
    Done(Vec<PathBuf>),
}

struct Current {
    index: usize,
    part: PathBuf,
    dest: PathBuf,
    file: std::fs::File,
    done: u64,
    size: Option<u64>,
    asked: u32,
}

/// Receives a copied set of files and folders into a folder, one file at a
/// time, each in pieces of [`CHUNK`] written straight to disk (a `.part` file
/// renamed when complete). Top-level names that already exist get " (1)" etc.
/// Dropping it mid-way removes the unfinished file.
pub struct Receiver {
    items: Vec<RemoteItem>,
    dests: Vec<Option<PathBuf>>,
    top_level: Vec<PathBuf>,
    next: usize,
    current: Option<Current>,
    data_id: Option<u32>,
    /// Total bytes announced (for "receiving …" messages).
    pub total_bytes: u64,
}

impl Receiver {
    /// Plans where each item goes under `dir`, creates the folders, and
    /// returns the first step.
    pub fn start(items: Vec<RemoteItem>, data_id: Option<u32>, dir: &Path) -> (Self, Step) {
        let items: Vec<RemoteItem> = items.into_iter().take(MAX_ITEMS).collect();
        let _ = std::fs::create_dir_all(dir);
        let mut tops: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
        let mut top_level = Vec::new();
        let mut dests = Vec::with_capacity(items.len());
        let mut total_bytes = 0u64;
        for item in &items {
            let mut parts: Vec<String> = item
                .relative_path
                .as_deref()
                .map(|p| p.split(['\\', '/']).map(str::to_owned).collect())
                .unwrap_or_default();
            parts.push(item.name.clone());
            // Every component must be a safe plain name.
            let safe: Option<Vec<String>> = parts.iter().map(|c| safe_file_name(c)).collect();
            let Some(safe) = safe.filter(|v| !v.is_empty()) else {
                dests.push(None);
                continue;
            };
            let top = tops.entry(safe[0].clone()).or_insert_with(|| {
                let p = unique_path(dir, &safe[0]);
                top_level.push(p.clone());
                p
            });
            let mut dest = top.clone();
            for c in &safe[1..] {
                dest.push(c);
            }
            if item.is_dir {
                let _ = std::fs::create_dir_all(&dest);
            } else {
                total_bytes = total_bytes.saturating_add(item.size.unwrap_or(0));
            }
            dests.push(Some(dest));
        }
        let mut r = Self {
            items,
            dests,
            top_level,
            next: 0,
            current: None,
            data_id,
            total_bytes,
        };
        let step = r.advance();
        (r, step)
    }

    /// Number of files (not folders) in this copy.
    pub fn file_count(&self) -> usize {
        self.items.iter().filter(|i| !i.is_dir).count()
    }

    /// A piece arrived for `stream_id` (`None` = the other side failed).
    pub fn on_data(&mut self, stream_id: u32, data: Option<&[u8]>) -> Option<Step> {
        use std::io::Write as _;
        let cur = self.current.as_mut()?;
        if u32::try_from(cur.index).ok() != Some(stream_id) {
            return None; // not ours (an older copy)
        }
        let Some(bytes) = data else {
            self.abandon();
            return Some(self.advance());
        };
        if cur.file.write_all(bytes).is_err() {
            self.abandon();
            return Some(self.advance());
        }
        cur.done += bytes.len() as u64;
        let finished = match cur.size {
            Some(size) => cur.done >= size || bytes.is_empty(),
            None => (bytes.len() as u64) < u64::from(cur.asked),
        };
        if finished {
            if let Some(cur) = self.current.take() {
                drop(cur.file);
                if std::fs::rename(&cur.part, &cur.dest).is_err() {
                    let _ = std::fs::remove_file(&cur.part);
                }
            }
            return Some(self.advance());
        }
        Some(Step::Request(self.request()))
    }

    /// Drops the file being received.
    fn abandon(&mut self) {
        if let Some(cur) = self.current.take() {
            drop(cur.file);
            let _ = std::fs::remove_file(&cur.part);
        }
    }

    /// Opens the next file (empty ones are written at once) or finishes.
    fn advance(&mut self) -> Step {
        while self.next < self.items.len() {
            let index = self.next;
            self.next += 1;
            let item = &self.items[index];
            let Some(dest) = self.dests[index].clone() else {
                continue;
            };
            if item.is_dir {
                continue;
            }
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if item.size == Some(0) {
                let _ = std::fs::write(&dest, []);
                continue;
            }
            let mut part = dest.clone().into_os_string();
            part.push(".part");
            let part = PathBuf::from(part);
            let Ok(file) = std::fs::File::create(&part) else {
                continue;
            };
            self.current = Some(Current {
                index,
                part,
                dest,
                file,
                done: 0,
                size: item.size,
                asked: 0,
            });
            return Step::Request(self.request());
        }
        Step::Done(std::mem::take(&mut self.top_level))
    }

    /// The request for the next piece of the current file.
    fn request(&mut self) -> FileContentsRequest {
        let data_id = self.data_id;
        let Some(cur) = self.current.as_mut() else {
            // Not reachable: callers have a current file.
            return FileContentsRequest {
                stream_id: 0,
                index: 0,
                flags: FileContentsFlags::RANGE,
                position: 0,
                requested_size: 0,
                data_id,
            };
        };
        let left = cur
            .size
            .map_or(u64::from(CHUNK), |s| s.saturating_sub(cur.done));
        let asked = u32::try_from(left.min(u64::from(CHUNK))).unwrap_or(CHUNK);
        cur.asked = asked;
        FileContentsRequest {
            stream_id: u32::try_from(cur.index).unwrap_or(0),
            index: i32::try_from(cur.index).unwrap_or(0),
            flags: FileContentsFlags::RANGE,
            position: cur.done,
            requested_size: asked,
            data_id,
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.abandon();
    }
}

/// Answers a request for a piece (or the size) of an offered file. `None`
/// means "send an error" (unknown index, a folder, or unreadable).
pub fn answer(offered: &[Outgoing], request: &FileContentsRequest) -> Option<FileContentsAnswer> {
    let item = usize::try_from(request.index)
        .ok()
        .and_then(|i| offered.get(i))?;
    if item.is_dir {
        return None;
    }
    if request.flags.contains(FileContentsFlags::SIZE) {
        return Some(FileContentsAnswer::Size(item.size));
    }
    read_range(
        &item.path,
        request.position,
        u64::from(request.requested_size),
    )
    .map(FileContentsAnswer::Data)
}

/// See [`answer`].
#[derive(Debug, PartialEq, Eq)]
pub enum FileContentsAnswer {
    Size(u64),
    Data(Vec<u8>),
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
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Sends `paths` through offer → receiver → answers, like the two sides
    /// would over the connection, into `dst`. Returns the top-level results.
    fn transfer(paths: &[PathBuf], dst: &Path) -> Vec<PathBuf> {
        let offered = offer(paths);
        let items = offered
            .iter()
            .map(|o| RemoteItem::from_descriptor(&o.descriptor))
            .collect();
        let (mut rx, mut step) = Receiver::start(items, Some(7), dst);
        let mut rounds = 0;
        loop {
            match step {
                Step::Done(top) => return top,
                Step::Request(req) => {
                    assert_eq!(req.data_id, Some(7));
                    assert!(req.requested_size <= CHUNK);
                    let data = match answer(&offered, &req) {
                        Some(FileContentsAnswer::Data(d)) => Some(d),
                        _ => None,
                    };
                    step = rx
                        .on_data(req.stream_id, data.as_deref())
                        .expect("our stream");
                }
            }
            rounds += 1;
            assert!(rounds < 1000, "transfer did not finish");
        }
    }

    #[test]
    fn copies_folders_and_big_files_in_pieces() {
        let src = tempfile_dir("src");
        let dst = tempfile_dir("dst");
        let proj = src.join("Proj");
        std::fs::create_dir_all(proj.join("sub").join("deeper")).expect("dirs");
        std::fs::write(proj.join("sub").join("a.txt"), b"hello").expect("a");
        std::fs::write(proj.join("empty.txt"), b"").expect("empty");
        let big: Vec<u8> = (0..(10 * 1024 * 1024 + 123u32))
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(proj.join("big.bin"), &big).expect("big");
        let zip: Vec<u8> = (0..(5 * 1024 * 1024u32)).map(|i| (i % 13) as u8).collect();
        std::fs::write(src.join("single.zip"), &zip).expect("zip");

        let top = transfer(&[proj.clone(), src.join("single.zip")], &dst);
        assert_eq!(top, vec![dst.join("Proj"), dst.join("single.zip")]);
        assert_eq!(
            std::fs::read(dst.join("Proj/sub/a.txt")).expect("a"),
            b"hello"
        );
        assert_eq!(
            std::fs::read(dst.join("Proj/empty.txt")).expect("e").len(),
            0
        );
        assert_eq!(std::fs::read(dst.join("Proj/big.bin")).expect("big"), big);
        assert_eq!(std::fs::read(dst.join("single.zip")).expect("zip"), zip);
        assert!(dst.join("Proj/sub/deeper").is_dir(), "empty folders too");
        let leftovers = walk(&dst)
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(".part"));
        assert_eq!(leftovers.count(), 0);

        // The same copy again does not overwrite: "Proj (1)".
        let again = transfer(&[proj], &dst);
        assert_eq!(again, vec![dst.join("Proj (1)")]);
        let _ = std::fs::remove_dir_all(src);
        let _ = std::fs::remove_dir_all(dst);
    }

    #[test]
    fn unsafe_names_from_the_other_side_are_skipped() {
        let dst = tempfile_dir("unsafe");
        let items = vec![
            RemoteItem {
                name: "..".into(),
                relative_path: None,
                size: Some(1),
                is_dir: false,
            },
            RemoteItem {
                name: "x".into(),
                relative_path: Some("ok\\..".into()),
                size: Some(1),
                is_dir: false,
            },
            RemoteItem {
                name: "CON".into(),
                relative_path: None,
                size: Some(1),
                is_dir: false,
            },
        ];
        let (_rx, step) = Receiver::start(items, None, &dst);
        assert_eq!(step, Step::Done(Vec::new()));
        let _ = std::fs::remove_dir_all(dst);
    }

    #[test]
    fn a_failed_piece_skips_that_file_and_cleans_up() {
        let dst = tempfile_dir("fail");
        let items = vec![
            RemoteItem {
                name: "a.bin".into(),
                relative_path: None,
                size: Some(10),
                is_dir: false,
            },
            RemoteItem {
                name: "b.bin".into(),
                relative_path: None,
                size: Some(3),
                is_dir: false,
            },
        ];
        let (mut rx, step) = Receiver::start(items, None, &dst);
        let Step::Request(first) = step else {
            panic!("expected a request")
        };
        let step = rx.on_data(first.stream_id, None).expect("ours");
        let Step::Request(second) = step else {
            panic!("expected the next file")
        };
        assert_eq!(second.index, 1);
        let step = rx.on_data(second.stream_id, Some(b"xyz")).expect("ours");
        assert_eq!(step, Step::Done(vec![dst.join("a.bin"), dst.join("b.bin")]));
        assert!(!dst.join("a.bin").exists() && !dst.join("a.bin.part").exists());
        assert_eq!(std::fs::read(dst.join("b.bin")).expect("b"), b"xyz");
        let _ = std::fs::remove_dir_all(dst);
    }

    fn tempfile_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cd-xfer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                }
                out.push(p);
            }
        }
        out
    }

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
