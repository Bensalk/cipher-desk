//! The Windows service: a thin supervisor that keeps one worker running in the
//! session that is on the monitor, moving it when Windows switches sessions
//! (fast user switching, sign-out). The worker does the actual sharing.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use crate::launch::{active_session, spawn_tray, spawn_worker, Worker};
use crate::link::{EXIT_OFFLINE, EXIT_QUIT, EXIT_RESTART};

pub const SERVICE_NAME: &str = "CipherDeskHost";
pub const DISPLAY_NAME: &str = "Cipher Desk Host (screen sharing)";
/// Service control sent by `go-online` (run as administrator from the tray):
/// turn sharing back on after "Go offline".
pub const GO_ONLINE_CONTROL: u32 = 130;

define_windows_service!(ffi_service_main, service_main);

/// Called by Windows when the service starts (the `run-service` command hands
/// control here through the service dispatcher).
pub fn run() -> anyhow::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .map_err(|e| anyhow::anyhow!("service dispatcher: {e}"))
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!(error = %e, "service stopped with an error");
    }
}

fn run_service() -> anyhow::Result<()> {
    // Lock the service folder (and move secrets from older versions into
    // the locked part) before anything reads them.
    match crate::config::data_dir().and_then(|d| crate::secure::lock_down(&d)) {
        Ok(()) => {
            if let Ok(d) = crate::config::data_dir() {
                let _ = std::fs::write(d.join("version.txt"), env!("CARGO_PKG_VERSION"));
            }
        }
        Err(e) => tracing::error!(error = %format!("{e:#}"), "could not lock the service folder"),
    }
    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::clone(&running);
    // "Go offline" from the tray: kept only in memory, so a restart of the
    // computer (or the service) brings sharing back on.
    let offline = Arc::new(AtomicBool::new(false));
    let online_flag = Arc::clone(&offline);
    // "Exit" from the tray: also no icon. Same rules as "Go offline".
    let hidden = Arc::new(AtomicBool::new(false));
    let shown_flag = Arc::clone(&hidden);

    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop_flag.store(false, Ordering::SeqCst);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::UserEvent(code) if code.to_raw() == GO_ONLINE_CONTROL => {
            online_flag.store(false, Ordering::SeqCst);
            shown_flag.store(false, Ordering::SeqCst);
            tracing::info!("sharing turned back on (Go online)");
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status_handle = service_control_handler::register(SERVICE_NAME, handler)
        .map_err(|e| anyhow::anyhow!("register control handler: {e}"))?;

    let set_state = |state: ServiceState, controls: ServiceControlAccept| {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: controls,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(5),
            process_id: None,
        });
    };

    set_state(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    );
    supervise(&running, &offline, &hidden);
    set_state(ServiceState::Stopped, ServiceControlAccept::empty());
    Ok(())
}

/// Keeps a worker (sharing) and a tray icon running in the session on the
/// monitor until asked to stop. While `offline`, there is no worker; while
/// `hidden`, no tray icon either.
fn supervise(running: &AtomicBool, offline: &AtomicBool, hidden: &AtomicBool) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "cannot find the service program");
            return;
        }
    };
    let mut worker = Slot::new("sharing worker");
    let mut tray = Slot::new("tray icon");

    while running.load(Ordering::SeqCst) {
        // A worker that ended by itself: did the tray ask for it?
        if let Some(code) = worker.ended() {
            match code {
                Some(EXIT_OFFLINE) => {
                    offline.store(true, Ordering::SeqCst);
                    tracing::info!(
                        "sharing turned off from the tray icon (until Go online or a restart)"
                    );
                }
                Some(EXIT_RESTART) => tracing::info!("viewer disconnected from the tray icon"),
                code => tracing::warn!(?code, "the sharing worker ended"),
            }
        }
        if tray.ended() == Some(Some(EXIT_QUIT)) {
            offline.store(true, Ordering::SeqCst);
            hidden.store(true, Ordering::SeqCst);
            tracing::info!(
                "Exit chosen from the tray icon: sharing and icon off until Go online or a restart"
            );
        }
        let active = active_session();
        let sharing = if offline.load(Ordering::SeqCst) {
            None
        } else {
            active
        };
        worker.keep(sharing, |s| spawn_worker(&exe, s));
        let icon = if hidden.load(Ordering::SeqCst) {
            None
        } else {
            active
        };
        tray.keep(icon, |s| spawn_tray(&exe, s));

        std::thread::sleep(Duration::from_secs(1));
    }

    worker.keep(None, |s| spawn_worker(&exe, s));
    tray.keep(None, |s| spawn_tray(&exe, s));
}

/// One supervised process: kept running in the wanted session.
struct Slot {
    what: &'static str,
    process: Option<Worker>,
    /// A start failure was logged and nothing has started since (no repeats
    /// every second, e.g. no tray at the sign-in screen).
    failing: bool,
}

impl Slot {
    fn new(what: &'static str) -> Self {
        Self {
            what,
            process: None,
            failing: false,
        }
    }

    /// If the process has ended by itself: removes it and gives its exit code.
    fn ended(&mut self) -> Option<Option<u32>> {
        if self.process.as_ref().is_some_and(Worker::is_alive) {
            return None;
        }
        let p = self.process.take()?;
        let code = p.exit_code();
        p.stop();
        Some(code)
    }

    /// Makes sure it runs in `session` (`None`: not at all).
    fn keep(&mut self, session: Option<u32>, start: impl Fn(u32) -> anyhow::Result<Worker>) {
        let fine = match (&self.process, session) {
            (Some(p), Some(s)) => p.session == s && p.is_alive(),
            (None, None) => true,
            _ => false,
        };
        if fine {
            return;
        }
        if let Some(old) = self.process.take() {
            old.stop();
        }
        let Some(session) = session else { return };
        match start(session) {
            Ok(p) => {
                tracing::info!(session, "started the {}", self.what);
                self.process = Some(p);
                self.failing = false;
            }
            Err(e) => {
                if !self.failing {
                    tracing::warn!(error = %format!("{e:#}"), "could not start the {}; will retry", self.what);
                }
                self.failing = true;
            }
        }
    }
}
