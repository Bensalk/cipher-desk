//! App settings (Stage 7), stored as `settings.json` next to the connections.
//! Missing or invalid values fall back to the secure defaults.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil;

/// Allowed idle-disconnect choices, in minutes (S7). 0 = never.
pub const IDLE_CHOICES: [u32; 6] = [5, 15, 30, 60, 120, 0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    /// Android: block screenshots and recent-apps thumbnails (S10).
    pub block_screenshots: bool,
    /// Clipboard sharing for new connections (S3).
    pub clipboard_default: bool,
    /// Confirm the user (Windows Hello) before a saved password is used (S5, S13).
    pub confirm_saved_password: bool,
    /// Disconnect after this many minutes without input; 0 = never (S7).
    pub idle_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            block_screenshots: true,
            clipboard_default: false,
            confirm_saved_password: true,
            idle_minutes: 30,
        }
    }
}

impl Settings {
    /// Replaces out-of-range values with defaults.
    fn sanitized(mut self) -> Self {
        if !IDLE_CHOICES.contains(&self.idle_minutes) {
            self.idle_minutes = Self::default().idle_minutes;
        }
        self
    }
}

/// Loads settings; a missing or damaged file gives the defaults (a damaged
/// file is set aside).
pub fn load(path: &Path) -> Settings {
    match fsutil::read_limited(path) {
        Ok(Some(bytes)) => decode(&bytes).unwrap_or_else(|| {
            let _ = fsutil::set_aside(path);
            Settings::default()
        }),
        _ => Settings::default(),
    }
}

/// Parses settings; unknown or out-of-range values fall back to the defaults.
pub(crate) fn decode(bytes: &[u8]) -> Option<Settings> {
    serde_json::from_slice::<Settings>(bytes)
        .ok()
        .map(Settings::sanitized)
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let json =
        serde_json::to_vec_pretty(&settings.clone().sanitized()).map_err(std::io::Error::other)?;
    fsutil::write_with_backup(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_secure() {
        let s = Settings::default();
        assert!(s.block_screenshots && s.confirm_saved_password && !s.clipboard_default);
        assert_eq!(s.idle_minutes, 30);
    }

    #[test]
    fn round_trip_and_partial_files() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("settings.json");
        assert_eq!(load(&p), Settings::default());
        let s = Settings {
            theme: Theme::Dark,
            idle_minutes: 60,
            ..Settings::default()
        };
        save(&p, &s)?;
        assert_eq!(load(&p), s);
        // Older/partial file: missing fields take defaults.
        std::fs::write(&p, br#"{"theme":"light"}"#)?;
        let l = load(&p);
        assert_eq!(l.theme, Theme::Light);
        assert!(l.confirm_saved_password);
        Ok(())
    }

    #[test]
    fn invalid_values_fall_back() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("settings.json");
        std::fs::write(&p, br#"{"idle_minutes": 7}"#)?;
        assert_eq!(load(&p).idle_minutes, 30);
        std::fs::write(&p, b"garbage")?;
        assert_eq!(load(&p), Settings::default());
        Ok(())
    }
}
