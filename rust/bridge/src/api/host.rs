//! "Share this screen" (Cipher Desk Host) for the UI. Works on Windows only;
//! elsewhere [`host_supported`] is false and starting reports an error.
//!
//! The sharing password lives in the OS vault, like connection passwords. It is
//! handed to the UI only when the user asks to see it on the Share page.

use rdpv_core::secret::SecretString;
use rdpv_core::vault;

use super::vault::VaultResult;
use crate::frb_generated::StreamSink;

/// Vault entry for the sharing password.
const VAULT_ID: &str = "share-screen-host";

/// What happened, for the Share page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostUpdateKind {
    /// Sharing is on and waiting for a viewer on `port`.
    Listening,
    /// Someone is watching and controlling this screen.
    ViewerConnected,
    /// The viewer left; still sharing.
    ViewerDisconnected,
    /// Too many wrong passwords: refusing connections for `minutes`.
    LockedOut,
    /// Sharing stopped or could not start (`message`).
    Failed,
}

/// One update for the Share page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostUpdate {
    pub kind: HostUpdateKind,
    pub port: u16,
    pub minutes: u64,
    pub message: Option<String>,
}

impl HostUpdate {
    fn of(kind: HostUpdateKind) -> Self {
        Self {
            kind,
            port: 0,
            minutes: 0,
            message: None,
        }
    }

    fn failed(message: &str) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::of(HostUpdateKind::Failed)
        }
    }
}

/// True where this app can share its own screen.
#[flutter_rust_bridge::frb(sync)]
pub fn host_supported() -> bool {
    cfg!(windows)
}

/// The identity code of this computer's Host (created on first use), which the
/// viewer shows on first connection. `None` if it could not be created.
#[flutter_rust_bridge::frb(sync)]
pub fn host_fingerprint(data_dir: String) -> Option<String> {
    #[cfg(windows)]
    {
        rdpv_host::fingerprint(std::path::Path::new(&data_dir)).ok()
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        None
    }
}

/// Whether a sharing password is saved (`Ok` = yes, `NotFound` = no).
#[flutter_rust_bridge::frb(sync)]
pub fn host_has_password() -> VaultResult {
    super::vault::vault_check_password(VAULT_ID.into())
}

/// Saves a new random sharing password in the vault (replacing the old one).
#[flutter_rust_bridge::frb(sync)]
pub fn host_new_password() -> VaultResult {
    match new_password() {
        Some(p) => vault::save(VAULT_ID, &p).into(),
        None => VaultResult::Unavailable,
    }
}

/// The saved sharing password, to show on screen when the user asks.
#[flutter_rust_bridge::frb(sync)]
pub fn host_reveal_password() -> Option<String> {
    vault::load(VAULT_ID).ok().map(|p| p.expose().to_owned())
}

#[cfg(windows)]
fn new_password() -> Option<SecretString> {
    rdpv_host::generate_password().ok()
}

#[cfg(not(windows))]
fn new_password() -> Option<SecretString> {
    None
}

/// A sharing run. Create it, `start` it, `stop` it (or drop it).
#[flutter_rust_bridge::frb(opaque)]
pub struct ShareHost {
    #[cfg(windows)]
    handle: std::sync::Mutex<Option<rdpv_host::HostHandle>>,
}

impl ShareHost {
    #[flutter_rust_bridge::frb(sync)]
    pub fn create() -> ShareHost {
        ShareHost {
            #[cfg(windows)]
            handle: std::sync::Mutex::new(None),
        }
    }

    /// Starts sharing with the saved password. `data_dir` is the app's private
    /// data folder (the Host certificate is kept there).
    pub fn start(
        &self,
        port: u16,
        username: String,
        data_dir: String,
        sink: StreamSink<HostUpdate>,
    ) {
        #[cfg(windows)]
        self.start_windows(port, username, data_dir, sink);
        #[cfg(not(windows))]
        {
            let _ = (port, username, data_dir);
            let _ = sink.add(HostUpdate::failed(
                "Sharing this screen works on Windows only.",
            ));
        }
    }

    /// Stops sharing and disconnects the viewer.
    #[flutter_rust_bridge::frb(sync)]
    pub fn stop(&self) {
        #[cfg(windows)]
        if let Ok(mut h) = self.handle.lock() {
            if let Some(handle) = h.take() {
                handle.stop();
            }
        }
    }

    #[cfg(windows)]
    fn start_windows(
        &self,
        port: u16,
        username: String,
        data_dir: String,
        sink: StreamSink<HostUpdate>,
    ) {
        init_host_log(&data_dir);
        let fail = |sink: &StreamSink<HostUpdate>, message: &str| {
            let _ = sink.add(HostUpdate::failed(message));
        };
        let password = match vault::load(VAULT_ID) {
            Ok(p) => p,
            Err(_) => return fail(&sink, "Create a sharing password first."),
        };
        let Ok(mut slot) = self.handle.lock() else {
            return fail(&sink, "Sharing could not start.");
        };
        if let Some(old) = slot.take() {
            old.stop();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let config = rdpv_host::HostConfig {
            port,
            username,
            password,
            data_dir: data_dir.into(),
        };
        match rdpv_host::start(
            config,
            rdpv_host::win::ScreenCapture,
            rdpv_host::win::SendInputSink,
            tx,
        ) {
            Ok(handle) => *slot = Some(handle),
            Err(e) => {
                tracing::warn!(error = %e, "sharing did not start");
                let message = if format!("{e:#}").contains("port") {
                    "The sharing port is already in use. Choose another port."
                } else {
                    "Sharing could not start."
                };
                return fail(&sink, message);
            }
        }
        // Forward Host events to the page until sharing stops.
        let spawned = std::thread::Builder::new()
            .name("cipher-desk-host-events".into())
            .spawn(move || {
                for event in rx {
                    use rdpv_host::HostEvent as E;
                    let update = match event {
                        E::Listening { port } => HostUpdate {
                            port,
                            ..HostUpdate::of(HostUpdateKind::Listening)
                        },
                        E::ViewerConnected { .. } => {
                            HostUpdate::of(HostUpdateKind::ViewerConnected)
                        }
                        E::ViewerDisconnected => HostUpdate::of(HostUpdateKind::ViewerDisconnected),
                        E::LockedOut { minutes } => HostUpdate {
                            minutes,
                            ..HostUpdate::of(HostUpdateKind::LockedOut)
                        },
                        E::Failed { message } => HostUpdate::failed(&message),
                    };
                    if sink.add(update).is_err() {
                        return;
                    }
                }
            });
        if spawned.is_err() {
            tracing::warn!("event thread did not start");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_matches_the_platform() {
        assert_eq!(host_supported(), cfg!(windows));
    }

    #[test]
    fn vault_id_is_valid() {
        // Vault ids allow letters, digits, '-' and '_' only.
        assert!(VAULT_ID
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }
}

/// Writes the Share-screen host's log to `share-screen-log.txt` in the app data
/// folder (Windows), so connection and input diagnostics can be inspected. The
/// log never contains passwords, keystrokes or screen content; `sspi`/`picky`
/// are silenced by the logging policy. Safe to call more than once.
#[cfg(windows)]
fn init_host_log(data_dir: &str) {
    use std::io::Write as _;
    use std::sync::{Arc, Mutex, OnceLock};

    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return; // already initialised this run
    }
    let path = std::path::Path::new(data_dir).join("share-screen-log.txt");
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    #[derive(Clone)]
    struct FileWriter(Arc<Mutex<std::fs::File>>);
    impl std::io::Write for FileWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .map(|mut f| std::io::Write::write(&mut *f, buf))
                .unwrap_or(Ok(0))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0
                .lock()
                .map(|mut f| std::io::Write::flush(&mut *f))
                .unwrap_or(Ok(()))
        }
    }

    let writer = FileWriter(Arc::new(Mutex::new(file)));
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(rdpv_core::logging::env_filter("info"))
        .with_writer(move || writer.clone())
        .try_init();
    tracing::info!("share screen started");
}
