//! Saved connections and connection history (Stage 7).
//!
//! Stored as `connections.json` in the app's private data folder. The file
//! never contains passwords (those live only in the OS vault) and is written
//! atomically with a backup of the previous version. If the file is damaged
//! the backup is used; if both are damaged the damaged file is set aside (not
//! overwritten) and the app starts with an empty list and tells the user.
//!
//! Export files contain only what is needed to recreate a connection on
//! another device: no ids, no "password saved" flag, no history.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::input;

const FORMAT_VERSION: u32 = 1;
const EXPORT_FORMAT: &str = "secure-rdp-viewer-connections";
/// Most connections kept or imported.
pub const MAX_CONNECTIONS: usize = 500;
/// Most history entries kept (oldest are dropped).
pub const MAX_HISTORY: usize = 100;

/// Clipboard sharing setting of a connection (mirrors [`crate::policy::ClipboardDirection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardSetting {
    #[default]
    Off,
    Both,
    ToRemote,
    FromRemote,
}

/// One saved connection. Contains no secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// The user chose to save the password (it lives in the OS vault).
    #[serde(default)]
    pub save_password: bool,
    #[serde(default)]
    pub clipboard: ClipboardSetting,
    /// Unix seconds of the last connection attempt.
    #[serde(default)]
    pub last_used: Option<i64>,
    /// MAC address for Wake-on-LAN, e.g. "00:11:22:33:44:55".
    #[serde(default)]
    pub wake_mac: Option<String>,
}

/// How a connection attempt ended (no details, no secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub connection_id: String,
    /// Unix seconds.
    pub time: i64,
    /// "connected", "disconnected", or an error id such as "WrongCredentials".
    pub outcome: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionsFile {
    version: u32,
    #[serde(default)]
    connections: Vec<Connection>,
    #[serde(default)]
    history: Vec<HistoryEntry>,
}

/// Result of loading the connections file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub connections: Vec<Connection>,
    pub history: Vec<HistoryEntry>,
    /// The main file was damaged and the backup was used.
    pub restored_from_backup: bool,
    /// Main file and backup were damaged; they were set aside and the list is empty.
    pub damaged_and_reset: bool,
    /// Entries skipped because they were invalid.
    pub skipped: usize,
}

/// Validates the fields of one connection.
pub fn is_valid(c: &Connection) -> bool {
    let id_ok = !c.id.is_empty()
        && c.id.len() <= 64
        && c.id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    let mac_ok = c
        .wake_mac
        .as_deref()
        .is_none_or(|m| crate::wol::parse_mac(m).is_ok());
    id_ok
        && input::validate_profile_name(&c.name).is_ok()
        && input::parse_endpoint(&c.host, &c.port.to_string()).is_ok()
        && input::parse_username(&c.username).is_ok()
        && mac_ok
}

fn parse(bytes: &[u8]) -> Option<ConnectionsFile> {
    let f: ConnectionsFile = serde_json::from_slice(bytes).ok()?;
    (f.version == FORMAT_VERSION).then_some(f)
}

fn clean(f: ConnectionsFile) -> (Vec<Connection>, Vec<HistoryEntry>, usize) {
    let total = f.connections.len();
    let mut seen = std::collections::HashSet::new();
    let connections: Vec<Connection> = f
        .connections
        .into_iter()
        .filter(|c| is_valid(c) && seen.insert(c.id.clone()))
        .take(MAX_CONNECTIONS)
        .collect();
    let skipped = total - connections.len();
    let mut history = f.history;
    history.retain(|h| seen.contains(&h.connection_id) && h.outcome.len() <= 64);
    let start = history.len().saturating_sub(MAX_HISTORY);
    (connections, history.split_off(start), skipped)
}

/// Parses and validates the connections file's contents:
/// (valid connections, their history, number of entries skipped).
pub(crate) fn decode(bytes: &[u8]) -> Option<(Vec<Connection>, Vec<HistoryEntry>, usize)> {
    parse(bytes).map(clean)
}

/// Loads `connections.json` from `path`, using the backup if needed.
pub fn load(path: &Path) -> std::io::Result<Loaded> {
    let main = match fsutil::read_limited(path) {
        Ok(Some(b)) => Some(decode(&b)),
        Ok(None) => return Ok(Loaded::default()),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Some(None),
        Err(e) => return Err(e),
    };
    if let Some(Some((connections, history, skipped))) = main {
        return Ok(Loaded {
            connections,
            history,
            skipped,
            ..Loaded::default()
        });
    }
    // Main file damaged: try the backup.
    let bak = fsutil::backup_path(path);
    if let Ok(Some(b)) = fsutil::read_limited(&bak) {
        if let Some((connections, history, skipped)) = decode(&b) {
            fsutil::set_aside(path)?;
            return Ok(Loaded {
                connections,
                history,
                skipped,
                restored_from_backup: true,
                ..Loaded::default()
            });
        }
    }
    fsutil::set_aside(path)?;
    Ok(Loaded {
        damaged_and_reset: true,
        ..Loaded::default()
    })
}

/// Saves connections and history (atomic, private, with backup).
pub fn save(
    path: &Path,
    connections: &[Connection],
    history: &[HistoryEntry],
) -> std::io::Result<()> {
    let valid: Vec<Connection> = connections
        .iter()
        .filter(|c| is_valid(c))
        .take(MAX_CONNECTIONS)
        .cloned()
        .collect();
    let start = history.len().saturating_sub(MAX_HISTORY);
    let file = ConnectionsFile {
        version: FORMAT_VERSION,
        connections: valid,
        history: history[start..].to_vec(),
    };
    let json = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
    fsutil::write_with_backup(path, &json)
}

// ---------------------------------------------------------------- export / import

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedConnection {
    #[serde(default)]
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub clipboard: ClipboardSetting,
    #[serde(default)]
    pub wake_mac: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    connections: Vec<ExportedConnection>,
}

/// Builds an export file. Contains no passwords, ids, saved-password flags or history.
pub fn export(connections: &[Connection]) -> String {
    let file = ExportFile {
        format: EXPORT_FORMAT.to_owned(),
        version: FORMAT_VERSION,
        connections: connections
            .iter()
            .map(|c| ExportedConnection {
                name: c.name.clone(),
                host: c.host.clone(),
                port: c.port,
                username: c.username.clone(),
                clipboard: c.clipboard,
                wake_mac: c.wake_mac.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Result of reading an export file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    /// Valid connections, without ids (the app assigns new ones).
    pub connections: Vec<ExportedConnection>,
    pub skipped: usize,
    /// The text was not an export file of this app.
    pub not_an_export: bool,
}

/// Reads an export file. Every entry is validated; clipboard sharing is
/// switched off on import so a file from elsewhere cannot turn it on silently.
pub fn import(text: &str) -> Imported {
    if text.len() as u64 > fsutil::MAX_FILE_BYTES {
        return Imported {
            not_an_export: true,
            ..Imported::default()
        };
    }
    let Ok(file) = serde_json::from_str::<ExportFile>(text) else {
        return Imported {
            not_an_export: true,
            ..Imported::default()
        };
    };
    if file.format != EXPORT_FORMAT || file.version != FORMAT_VERSION {
        return Imported {
            not_an_export: true,
            ..Imported::default()
        };
    }
    let total = file.connections.len();
    let connections: Vec<ExportedConnection> = file
        .connections
        .into_iter()
        .filter(|e| {
            is_valid(&Connection {
                id: "import".into(),
                name: e.name.clone(),
                host: e.host.clone(),
                port: e.port,
                username: e.username.clone(),
                save_password: false,
                clipboard: e.clipboard,
                last_used: None,
                wake_mac: e.wake_mac.clone(),
            })
        })
        .map(|mut e| {
            e.clipboard = ClipboardSetting::Off;
            e
        })
        .take(MAX_CONNECTIONS)
        .collect();
    Imported {
        skipped: total - connections.len(),
        connections,
        not_an_export: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, host: &str) -> Connection {
        Connection {
            id: id.into(),
            name: "Office PC".into(),
            host: host.into(),
            port: 3389,
            username: "alice".into(),
            save_password: true,
            clipboard: ClipboardSetting::Both,
            last_used: Some(1_800_000_000),
            wake_mac: Some("00:11:22:33:44:55".into()),
        }
    }

    fn hist(id: &str, t: i64) -> HistoryEntry {
        HistoryEntry {
            connection_id: id.into(),
            time: t,
            outcome: "connected".into(),
        }
    }

    #[test]
    fn save_and_load_round_trip() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        assert_eq!(load(&p)?, Loaded::default());
        let list = vec![conn("a1", "192.0.2.10"), conn("b2", "pc.example.com")];
        save(&p, &list, &[hist("a1", 1)])?;
        let l = load(&p)?;
        assert_eq!(l.connections, list);
        assert_eq!(l.history, vec![hist("a1", 1)]);
        assert!(!l.restored_from_backup && !l.damaged_and_reset);
        Ok(())
    }

    #[test]
    fn file_never_contains_a_password_field() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        save(&p, &[conn("a1", "192.0.2.10")], &[])?;
        let text = std::fs::read_to_string(&p)?.to_ascii_lowercase();
        assert!(text.contains("save_password")); // the flag only
        assert!(!text.contains("\"password\""));
        Ok(())
    }

    #[test]
    fn damaged_file_uses_backup() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        let list = vec![conn("a1", "192.0.2.10")];
        save(&p, &list, &[])?;
        save(&p, &list, &[])?; // creates the backup
        std::fs::write(&p, b"{ half-written")?;
        let l = load(&p)?;
        assert!(l.restored_from_backup);
        assert_eq!(l.connections, list);
        Ok(())
    }

    #[test]
    fn damaged_file_without_backup_is_set_aside_not_lost() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        std::fs::write(&p, b"not json")?;
        let l = load(&p)?;
        assert!(l.damaged_and_reset && l.connections.is_empty());
        let kept = std::fs::read_dir(dir.path())?
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".damaged-"));
        assert!(kept, "damaged file should be kept for inspection");
        Ok(())
    }

    #[test]
    fn invalid_and_duplicate_entries_are_skipped() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        let mut bad = conn("c3", "rdp://bad");
        bad.name = "x".into();
        let json = serde_json::json!({
            "version": 1,
            "connections": [conn("a1", "192.0.2.10"), conn("a1", "192.0.2.11"), bad],
            "history": [hist("a1", 1), hist("gone", 2)]
        });
        std::fs::write(
            &p,
            serde_json::to_vec(&json).map_err(std::io::Error::other)?,
        )?;
        let l = load(&p)?;
        assert_eq!(l.connections.len(), 1);
        assert_eq!(l.skipped, 2);
        assert_eq!(l.history, vec![hist("a1", 1)]);
        Ok(())
    }

    #[test]
    fn history_is_capped() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("connections.json");
        let h: Vec<HistoryEntry> = (0..250).map(|t| hist("a1", t)).collect();
        save(&p, &[conn("a1", "192.0.2.10")], &h)?;
        let l = load(&p)?;
        assert_eq!(l.history.len(), MAX_HISTORY);
        assert_eq!(l.history.last().map(|e| e.time), Some(249));
        Ok(())
    }

    #[test]
    fn export_has_no_secrets_ids_or_history() {
        let text = export(&[conn("idzq7x", "192.0.2.10")]);
        for forbidden in [
            "idzq7x",
            "save_password",
            "password",
            "last_used",
            "history",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} in export");
        }
        assert!(text.contains("192.0.2.10"));
    }

    #[test]
    fn import_validates_and_switches_clipboard_off() {
        let text = export(&[conn("a1", "192.0.2.10"), conn("b2", "pc.example.com")]);
        let i = import(&text);
        assert!(!i.not_an_export);
        assert_eq!(i.connections.len(), 2);
        assert!(i
            .connections
            .iter()
            .all(|c| c.clipboard == ClipboardSetting::Off));

        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        json["connections"][0]["host"] = "rdp://evil".into();
        let i = import(&json.to_string());
        assert_eq!((i.connections.len(), i.skipped), (1, 1));

        assert!(import("{\"format\":\"other\",\"version\":1,\"connections\":[]}").not_an_export);
        assert!(import("garbage").not_an_export);
    }
}
