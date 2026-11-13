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

use crate::launch::{active_session, spawn_worker, Worker};

pub const SERVICE_NAME: &str = "CipherDeskHost";
pub const DISPLAY_NAME: &str = "Cipher Desk Host (screen sharing)";

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
    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::clone(&running);

    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop_flag.store(false, Ordering::SeqCst);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
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
    supervise(&running);
    set_state(ServiceState::Stopped, ServiceControlAccept::empty());
    Ok(())
}

/// Keeps a worker running in the active session until asked to stop.
fn supervise(running: &AtomicBool) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "cannot find the service program");
            return;
        }
    };
    let mut worker: Option<Worker> = None;

    while running.load(Ordering::SeqCst) {
        let active = active_session();
        let need_restart = match (&worker, active) {
            // No worker but a session is on screen: start one.
            (None, Some(_)) => true,
            // Worker in the wrong session, or it has exited: restart.
            (Some(w), Some(s)) => w.session != s || !w.is_alive(),
            // No session on screen (fully signed out): stop the worker.
            (Some(_), None) => {
                if let Some(w) = worker.take() {
                    w.stop();
                }
                false
            }
            (None, None) => false,
        };

        if need_restart {
            if let Some(old) = worker.take() {
                old.stop();
            }
            if let Some(session) = active {
                match spawn_worker(&exe, session) {
                    Ok(w) => {
                        tracing::info!(session, "started the sharing worker");
                        worker = Some(w);
                    }
                    Err(e) => tracing::warn!(error = %e, "could not start the worker; will retry"),
                }
            }
        }

        std::thread::sleep(Duration::from_secs(2));
    }

    if let Some(w) = worker.take() {
        w.stop();
    }
}
