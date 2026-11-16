//! Keeps the service's log files small and recent: a file is started afresh
//! (the previous one kept as `<name>.old.<ext>`) when it passes its size
//! limit or its first line is more than [`ROTATE_DAYS`] days old, and the
//! `.old` copy is deleted once everything in it is more than [`DELETE_DAYS`]
//! days old. So nothing older than about three weeks is kept.
//!
//! Every log line starts with its date (`YYYY-MM-DD`), which is what the age
//! check reads.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Start a new file when the first line is older than this.
pub const ROTATE_DAYS: i64 = 10;
/// Delete the `.old` copy when its last change is older than this.
pub const DELETE_DAYS: u64 = 11;
/// Size limit for each log file.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// `worker.log` → `worker.old.log`.
pub fn old_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{stem}.old.{ext}"))
}

/// Days since 1970-01-01 for a calendar date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date at the start of `line` (`YYYY-MM-DD…`) as days since 1970.
pub fn line_day(line: &str) -> Option<i64> {
    let date = line.get(..10)?;
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || date.as_bytes().get(4) != Some(&b'-') {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn today() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    i64::try_from(secs / 86_400).unwrap_or(0)
}

/// Whether the file should be started afresh: too big, or its first line is
/// too old.
pub fn needs_rotation(size: u64, first_line: &str, today: i64) -> bool {
    size > MAX_BYTES || line_day(first_line).is_some_and(|day| today - day > ROTATE_DAYS)
}

fn first_line(path: &Path) -> String {
    use std::io::{BufRead as _, BufReader, Read as _};
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut line = String::new();
    let _ = BufReader::new(file.take(256)).read_line(&mut line);
    line
}

/// Applies the rules to `path`. Returns true if the file was moved aside
/// (an open writer must then reopen it).
pub fn tidy(path: &Path) -> bool {
    let old = old_path(path);
    let stale = std::fs::metadata(&old)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > Duration::from_secs(DELETE_DAYS * 86_400));
    if stale {
        let _ = std::fs::remove_file(&old);
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !needs_rotation(meta.len(), &first_line(path), today()) {
        return false;
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(path, &old).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_count_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(
            days_from_civil(2026, 11, 16) - days_from_civil(2026, 11, 5),
            11
        );
        assert_eq!(
            days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 28),
            2
        );
        assert_eq!(
            line_day("2026-11-16T11:21:32.852372Z  INFO x"),
            Some(days_from_civil(2026, 11, 16))
        );
        assert_eq!(
            line_day("2026-11-16 10:00:00  Connected"),
            Some(days_from_civil(2026, 11, 16))
        );
        assert_eq!(line_day("garbage"), None);
        assert_eq!(line_day(""), None);
    }

    #[test]
    fn rotates_when_big_or_old() {
        let today = days_from_civil(2026, 11, 30);
        assert!(!needs_rotation(10, "2026-11-25 x", today));
        assert!(needs_rotation(10, "2026-11-19 x", today));
        assert!(needs_rotation(MAX_BYTES + 1, "2026-11-30 x", today));
        assert!(!needs_rotation(10, "", today));
    }

    #[test]
    fn old_copy_name() {
        assert_eq!(
            old_path(Path::new(r"C:\x\worker.log")),
            Path::new(r"C:\x\worker.old.log")
        );
    }
}
