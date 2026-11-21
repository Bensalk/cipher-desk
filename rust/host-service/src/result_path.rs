//! Pure validation for the `--result` file path `manage.rs` writes admin
//! command output to (it can hold the sharing password or a two-step setup
//! key). The actual Windows service code is `#[cfg(windows)]`-gated (this
//! whole binary does nothing on other systems), but this one piece of logic
//! has no Windows dependency of its own, so it lives in its own always-built
//! module — otherwise its tests could never run outside a Windows CI job.

use std::path::Path;

/// File name the app asks results to be written to.
// Only `manage.rs` (Windows-only) uses this outside tests; on other
// platforms it would otherwise be flagged as dead code.
#[cfg_attr(not(windows), allow(dead_code))]
pub const RESULT_NAME: &str = "service-result.txt";

/// True only if `path` is named [`RESULT_NAME`], sits directly inside an
/// existing folder named "Cipher Desk", and neither it nor that folder is a
/// link. The actual link check (Windows-only: a junction or a symbolic link
/// look the same to a program reading the folder, so both count) is passed
/// in as two closures rather than called directly, so this logic — the part
/// worth getting right — can be exercised on any platform with made-up
/// answers, including a real symlink on Linux standing in for the Windows
/// check. See the tests below and `crate::secure::is_link`, which supplies
/// the real answer on Windows.
// Same reasoning as RESULT_NAME above: only used outside tests on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn validate_result_path(
    path: &Path,
    parent_is_dir: bool,
    path_is_link: impl FnOnce() -> bool,
    parent_is_link: impl FnOnce() -> bool,
) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let named = path.file_name().is_some_and(|n| n == RESULT_NAME)
        && parent.file_name().is_some_and(|n| n == "Cipher Desk");
    named && parent_is_dir && !path_is_link() && !parent_is_link()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_file(dir: &Path) -> std::path::PathBuf {
        dir.join(RESULT_NAME)
    }

    #[test]
    fn a_plain_file_in_a_plain_folder_named_cipher_desk_is_accepted() {
        let dir = std::env::temp_dir().join("Cipher Desk");
        let path = result_file(&dir);
        assert!(validate_result_path(&path, true, || false, || false));
    }

    #[test]
    fn the_wrong_file_name_is_refused_even_if_nothing_is_a_link() {
        let dir = std::env::temp_dir().join("Cipher Desk");
        let path = dir.join("not-the-result-file.txt");
        assert!(!validate_result_path(&path, true, || false, || false));
    }

    #[test]
    fn the_wrong_folder_name_is_refused_even_if_nothing_is_a_link() {
        let dir = std::env::temp_dir().join("Not Cipher Desk");
        let path = result_file(&dir);
        assert!(!validate_result_path(&path, true, || false, || false));
    }

    #[test]
    fn a_missing_folder_is_refused() {
        let dir = std::env::temp_dir().join("Cipher Desk");
        let path = result_file(&dir);
        assert!(!validate_result_path(&path, false, || false, || false));
    }

    #[test]
    fn a_link_in_place_of_the_file_is_refused() {
        let dir = std::env::temp_dir().join("Cipher Desk");
        let path = result_file(&dir);
        assert!(!validate_result_path(&path, true, || true, || false));
    }

    #[test]
    fn a_link_in_place_of_the_folder_is_refused() {
        let dir = std::env::temp_dir().join("Cipher Desk");
        let path = result_file(&dir);
        assert!(!validate_result_path(&path, true, || false, || true));
    }

    // These two use a *real* link (a symlink standing in for a Windows
    // junction, which sets the same reparse-point attribute `is_link`
    // checks), to prove the logic also works against the actual filesystem
    // shape a swapped-in link would have, not only against made-up booleans
    // above. They use `Path::is_symlink` (cross-platform) rather than
    // `crate::secure::is_link` (Windows-only) to stand in for the real
    // check, the same way the real caller's closures would.

    #[cfg(unix)]
    #[test]
    fn a_real_symlink_in_place_of_the_folder_is_refused_on_linux() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("temp dir: {e}"));
        let real_other_dir = base.path().join("somewhere else entirely");
        std::fs::create_dir(&real_other_dir).unwrap_or_else(|e| panic!("create real dir: {e}"));
        let link_dir = base.path().join("Cipher Desk");
        std::os::unix::fs::symlink(&real_other_dir, &link_dir)
            .unwrap_or_else(|e| panic!("create symlink: {e}"));
        let path = result_file(&link_dir);

        assert!(link_dir.is_symlink());
        assert!(!validate_result_path(
            &path,
            link_dir.is_dir(),
            || path.is_symlink(),
            || link_dir.is_symlink(),
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_real_symlink_in_place_of_the_file_is_refused_on_linux() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("temp dir: {e}"));
        let dir = base.path().join("Cipher Desk");
        std::fs::create_dir(&dir).unwrap_or_else(|e| panic!("create dir: {e}"));
        let real_other_file = base.path().join("somewhere-else.txt");
        std::fs::write(&real_other_file, b"not the result file")
            .unwrap_or_else(|e| panic!("write real file: {e}"));
        let path = result_file(&dir);
        std::os::unix::fs::symlink(&real_other_file, &path)
            .unwrap_or_else(|e| panic!("create symlink: {e}"));

        assert!(path.is_symlink());
        assert!(!validate_result_path(
            &path,
            dir.is_dir(),
            || path.is_symlink(),
            || dir.is_symlink(),
        ));
    }

    // Windows only, and exercises the real `crate::secure::is_link` (not a
    // stand-in), against a real directory symlink — standard-library only,
    // no new dependency. `is_link` checks the reparse-point attribute, which
    // is identical for a symlink and a junction, so this proves the same
    // detection a junction would trigger. GitHub's own Windows runners (and
    // this test's CI) run elevated, so creating the symlink itself needs no
    // extra setup there; running this test by hand on an ordinary,
    // non-elevated Windows account may need Developer Mode turned on first.
    #[cfg(windows)]
    #[test]
    fn a_real_symlink_in_place_of_the_folder_is_refused_on_windows() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("temp dir: {e}"));
        let real_other_dir = base.path().join("somewhere else entirely");
        std::fs::create_dir(&real_other_dir).unwrap_or_else(|e| panic!("create real dir: {e}"));
        let link_dir = base.path().join("Cipher Desk");
        std::os::windows::fs::symlink_dir(&real_other_dir, &link_dir)
            .unwrap_or_else(|e| panic!("create symlink (needs admin or Developer Mode): {e}"));
        let path = result_file(&link_dir);

        assert!(crate::secure::is_link(&link_dir));
        assert!(!validate_result_path(
            &path,
            link_dir.is_dir(),
            || crate::secure::is_link(&path),
            || crate::secure::is_link(&link_dir),
        ));
    }
}
