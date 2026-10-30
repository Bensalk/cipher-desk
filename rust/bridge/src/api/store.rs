//! Saved connections, history, settings, trusted computers and Wake-on-LAN
//! for the UI (Stage 7). Files live in the app's private data folder; none of
//! them contain passwords.

use std::path::Path;
use std::time::Duration;

use rdpv_core::pins::PinStore;
use rdpv_core::profiles::{
    self, ClipboardSetting, Connection, ConnectionMethod as CoreMethod, HistoryEntry,
};
use rdpv_core::settings::{self, Settings, Theme};
use rdpv_core::wol;

use crate::api::session::ClipboardMode;

/// How a connection is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMethod {
    /// Windows Remote Desktop: the computer's own screen locks while you work.
    PrivateRemoteDesktop,
    /// Cipher Desk Host on the computer: the same screen at both ends.
    ShareScreen,
    /// A VNC server on the computer: the same screen at both ends (local network or VPN only).
    Vnc,
}

pub(crate) fn method_to_core(m: ConnectionMethod) -> CoreMethod {
    match m {
        ConnectionMethod::PrivateRemoteDesktop => CoreMethod::PrivateRemoteDesktop,
        ConnectionMethod::ShareScreen => CoreMethod::ShareScreen,
        ConnectionMethod::Vnc => CoreMethod::Vnc,
    }
}

fn method_from_core(m: CoreMethod) -> ConnectionMethod {
    match m {
        CoreMethod::PrivateRemoteDesktop => ConnectionMethod::PrivateRemoteDesktop,
        CoreMethod::ShareScreen => ConnectionMethod::ShareScreen,
        CoreMethod::Vnc => ConnectionMethod::Vnc,
    }
}

/// Port used when the port field is left empty, for each method.
#[flutter_rust_bridge::frb(sync)]
pub fn default_port_for(method: ConnectionMethod) -> u16 {
    method_to_core(method).default_port()
}

/// A saved connection (no password).
#[derive(Debug, Clone)]
pub struct ConnectionData {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub save_password: bool,
    pub clipboard: ClipboardMode,
    /// Unix seconds.
    pub last_used: Option<i64>,
    pub wake_mac: Option<String>,
    pub method: ConnectionMethod,
}

#[derive(Debug, Clone)]
pub struct HistoryData {
    pub connection_id: String,
    pub time: i64,
    pub outcome: String,
}

#[derive(Debug, Clone)]
pub struct LoadResult {
    pub connections: Vec<ConnectionData>,
    pub history: Vec<HistoryData>,
    /// The file was damaged and the previous good version was used.
    pub restored_from_backup: bool,
    /// The file and its backup were damaged; they were kept aside and the list is empty.
    pub damaged_and_reset: bool,
    /// Invalid entries that were skipped.
    pub skipped: u32,
    /// The file could not be read at all (e.g. permissions).
    pub read_error: bool,
}

fn to_setting(m: ClipboardMode) -> ClipboardSetting {
    match m {
        ClipboardMode::Off => ClipboardSetting::Off,
        ClipboardMode::Both => ClipboardSetting::Both,
        ClipboardMode::ToRemoteOnly => ClipboardSetting::ToRemote,
        ClipboardMode::FromRemoteOnly => ClipboardSetting::FromRemote,
    }
}

fn to_mode(s: ClipboardSetting) -> ClipboardMode {
    match s {
        ClipboardSetting::Off => ClipboardMode::Off,
        ClipboardSetting::Both => ClipboardMode::Both,
        ClipboardSetting::ToRemote => ClipboardMode::ToRemoteOnly,
        ClipboardSetting::FromRemote => ClipboardMode::FromRemoteOnly,
    }
}

fn to_core(c: &ConnectionData) -> Connection {
    Connection {
        id: c.id.clone(),
        name: c.name.clone(),
        host: c.host.clone(),
        port: c.port,
        username: c.username.clone(),
        save_password: c.save_password,
        clipboard: to_setting(c.clipboard),
        last_used: c.last_used,
        wake_mac: c.wake_mac.clone(),
        method: method_to_core(c.method),
    }
}

fn from_core(c: Connection) -> ConnectionData {
    ConnectionData {
        id: c.id,
        name: c.name,
        host: c.host,
        port: c.port,
        username: c.username,
        save_password: c.save_password,
        clipboard: to_mode(c.clipboard),
        last_used: c.last_used,
        wake_mac: c.wake_mac,
        method: method_from_core(c.method),
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn connections_load(path: String) -> LoadResult {
    match profiles::load(Path::new(&path)) {
        Ok(l) => LoadResult {
            connections: l.connections.into_iter().map(from_core).collect(),
            history: l
                .history
                .into_iter()
                .map(|h| HistoryData {
                    connection_id: h.connection_id,
                    time: h.time,
                    outcome: h.outcome,
                })
                .collect(),
            restored_from_backup: l.restored_from_backup,
            damaged_and_reset: l.damaged_and_reset,
            skipped: u32::try_from(l.skipped).unwrap_or(u32::MAX),
            read_error: false,
        },
        Err(_) => LoadResult {
            connections: Vec::new(),
            history: Vec::new(),
            restored_from_backup: false,
            damaged_and_reset: false,
            skipped: 0,
            read_error: true,
        },
    }
}

/// Saves connections and history. Returns false if the file could not be written.
#[flutter_rust_bridge::frb(sync)]
pub fn connections_save(
    path: String,
    connections: Vec<ConnectionData>,
    history: Vec<HistoryData>,
) -> bool {
    let list: Vec<Connection> = connections.iter().map(to_core).collect();
    let hist: Vec<HistoryEntry> = history
        .into_iter()
        .map(|h| HistoryEntry {
            connection_id: h.connection_id,
            time: h.time,
            outcome: h.outcome,
        })
        .collect();
    profiles::save(Path::new(&path), &list, &hist).is_ok()
}

/// Export text (no passwords, ids, saved-password flags or history).
#[flutter_rust_bridge::frb(sync)]
pub fn connections_export(connections: Vec<ConnectionData>) -> String {
    let list: Vec<Connection> = connections.iter().map(to_core).collect();
    profiles::export(&list)
}

#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Valid connections with empty ids (the app assigns new ones).
    pub connections: Vec<ConnectionData>,
    pub skipped: u32,
    pub not_an_export: bool,
}

#[flutter_rust_bridge::frb(sync)]
pub fn connections_import(text: String) -> ImportResult {
    let i = profiles::import(&text);
    ImportResult {
        connections: i
            .connections
            .into_iter()
            .map(|e| ConnectionData {
                id: String::new(),
                name: e.name,
                host: e.host,
                port: e.port,
                username: e.username,
                save_password: false,
                clipboard: to_mode(e.clipboard),
                last_used: None,
                wake_mac: e.wake_mac,
                method: method_from_core(e.method),
            })
            .collect(),
        skipped: u32::try_from(i.skipped).unwrap_or(u32::MAX),
        not_an_export: i.not_an_export,
    }
}

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTheme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct SettingsData {
    pub theme: AppTheme,
    pub block_screenshots: bool,
    pub clipboard_default: bool,
    pub confirm_saved_password: bool,
    pub idle_minutes: u32,
}

#[flutter_rust_bridge::frb(sync)]
pub fn settings_load(path: String) -> SettingsData {
    let s = settings::load(Path::new(&path));
    SettingsData {
        theme: match s.theme {
            Theme::System => AppTheme::System,
            Theme::Light => AppTheme::Light,
            Theme::Dark => AppTheme::Dark,
        },
        block_screenshots: s.block_screenshots,
        clipboard_default: s.clipboard_default,
        confirm_saved_password: s.confirm_saved_password,
        idle_minutes: s.idle_minutes,
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn settings_save(path: String, data: SettingsData) -> bool {
    let s = Settings {
        theme: match data.theme {
            AppTheme::System => Theme::System,
            AppTheme::Light => Theme::Light,
            AppTheme::Dark => Theme::Dark,
        },
        block_screenshots: data.block_screenshots,
        clipboard_default: data.clipboard_default,
        confirm_saved_password: data.confirm_saved_password,
        idle_minutes: data.idle_minutes,
    };
    settings::save(Path::new(&path), &s).is_ok()
}

/// Idle-disconnect choices in minutes (0 = never).
#[flutter_rust_bridge::frb(sync)]
pub fn idle_minute_choices() -> Vec<u32> {
    settings::IDLE_CHOICES.to_vec()
}

// ---------------------------------------------------------------- trusted computers

#[derive(Debug, Clone)]
pub struct TrustedComputer {
    /// "host:port".
    pub endpoint: String,
    pub fingerprint: String,
}

/// Computers whose certificate the user has approved.
#[flutter_rust_bridge::frb(sync)]
pub fn trusted_computers(pins_path: String) -> Vec<TrustedComputer> {
    PinStore::open(pins_path)
        .map(|s| {
            s.entries()
                .into_iter()
                .map(|(endpoint, fingerprint)| TrustedComputer {
                    endpoint,
                    fingerprint,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Forgets a trusted computer by its "host:port" key.
#[flutter_rust_bridge::frb(sync)]
pub fn forget_trusted_computer(pins_path: String, endpoint: String) -> bool {
    PinStore::open(pins_path)
        .and_then(|mut s| s.forget(&endpoint))
        .is_ok()
}

// ---------------------------------------------------------------- portable mode (SA-10)

/// The portable data folder when running as a portable copy (Windows portable
/// ZIP with `portable.txt`, or an AppImage with a `.data` folder next to it);
/// `None` for an installed copy, which uses the normal per-user folders.
#[flutter_rust_bridge::frb(sync)]
pub fn portable_data_dir() -> Option<String> {
    rdpv_core::portable::current().map(|p| p.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- wake-on-lan / reachability

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeResult {
    Sent,
    InvalidMac,
    SendFailed,
}

#[flutter_rust_bridge::frb(sync)]
pub fn is_valid_mac(text: String) -> bool {
    wol::parse_mac(&text).is_ok()
}

/// Sends a Wake-on-LAN packet on the local network.
#[flutter_rust_bridge::frb(sync)]
pub fn wake_on_lan(mac: String) -> WakeResult {
    match wol::wake(&mac, wol::default_target()) {
        Ok(()) => WakeResult::Sent,
        Err(wol::WolError::InvalidMac) => WakeResult::InvalidMac,
        Err(wol::WolError::SendFailed) => WakeResult::SendFailed,
    }
}

/// Whether the computer accepts connections on its RDP port (sends no data).
pub fn check_reachable(host: String, port: u16, timeout_ms: u32) -> bool {
    wol::reachable(
        &host,
        port,
        Duration::from_millis(u64::from(timeout_ms.min(10_000))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ConnectionData {
        ConnectionData {
            id: "abc123".into(),
            name: "Office".into(),
            host: "192.0.2.10".into(),
            port: 3389,
            username: "alice".into(),
            save_password: true,
            clipboard: ClipboardMode::FromRemoteOnly,
            last_used: Some(5),
            wake_mac: None,
            method: ConnectionMethod::ShareScreen,
        }
    }

    #[test]
    fn connections_round_trip() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = dir
            .path()
            .join("connections.json")
            .to_string_lossy()
            .into_owned();
        let h = HistoryData {
            connection_id: "abc123".into(),
            time: 7,
            outcome: "connected".into(),
        };
        assert!(connections_save(path.clone(), vec![sample()], vec![h]));
        let l = connections_load(path);
        assert_eq!(l.connections.len(), 1);
        assert_eq!(l.connections[0].clipboard, ClipboardMode::FromRemoteOnly);
        assert_eq!(l.history.len(), 1);
        assert!(!l.read_error && !l.damaged_and_reset);
        Ok(())
    }

    #[test]
    fn import_assigns_no_ids_and_turns_clipboard_off() {
        let text = connections_export(vec![sample()]);
        let i = connections_import(text);
        assert_eq!(i.connections.len(), 1);
        assert!(i.connections[0].id.is_empty());
        assert!(!i.connections[0].save_password);
        assert_eq!(i.connections[0].clipboard, ClipboardMode::Off);
    }

    #[test]
    fn settings_round_trip() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = dir
            .path()
            .join("settings.json")
            .to_string_lossy()
            .into_owned();
        let mut s = settings_load(path.clone());
        assert!(s.confirm_saved_password);
        s.theme = AppTheme::Dark;
        s.idle_minutes = 15;
        assert!(settings_save(path.clone(), s));
        let back = settings_load(path);
        assert_eq!(back.theme, AppTheme::Dark);
        assert_eq!(back.idle_minutes, 15);
        Ok(())
    }

    #[test]
    fn mac_check() {
        assert!(is_valid_mac("00:11:22:33:44:55".into()));
        assert!(!is_valid_mac("nope".into()));
        assert_eq!(wake_on_lan("bad".into()), WakeResult::InvalidMac);
    }
}
