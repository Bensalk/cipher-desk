//! Cipher Desk Host service (Windows): lets Share screen reach the Windows
//! sign-in screen, so you can sign in to this computer from another device.
//!
//! Commands (the app runs the admin ones with an elevation prompt):
//!   install | uninstall [--purge] | start | stop | status | new-password | show-password
//!   run-service   (used by Windows to start the service)
//!   worker        (started by the service in the active session)
//!   tray          (the status icon, started by the service as the signed-in person)
//!   go-online     (from the tray icon, with the administrator prompt)
//!
//! This whole binary is Windows-only; on other systems it just says so.
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[cfg(windows)]
mod config;
#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod link;
#[cfg(windows)]
mod manage;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod worker;

#[cfg(not(windows))]
fn main() {
    eprintln!("The Cipher Desk Host service runs on Windows only.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    let command = std::env::args().nth(1).unwrap_or_default();
    // The service and worker run without a console; log to a file. The admin
    // commands print to the console the app captures.
    if matches!(command.as_str(), "run-service" | "worker" | "tray") {
        init_file_logging(&command);
    }

    let result = match command.as_str() {
        "run-service" => service::run(),
        "worker" => worker::run(),
        "tray" => tray::run(),
        "go-online" => manage::go_online(),
        "install" => manage::install(),
        "uninstall" | "remove" => {
            let purge = std::env::args().any(|a| a == "--purge");
            manage::uninstall(purge)
        }
        "start" => manage::start(),
        "stop" => manage::stop(),
        "status" => manage::status(),
        "new-password" => manage::new_password(),
        "import-password" => manage::import_password(std::env::args().nth(2)),
        "show-password" => manage::show_password(),
        "twostep-begin" => manage::twostep_begin(),
        "twostep-finish" => manage::twostep_finish(std::env::args().nth(2)),
        "twostep-off" => manage::twostep_off(),
        other => {
            eprintln!("unknown command: {other:?}");
            eprintln!(
                "use: install | uninstall | start | stop | status | new-password | import-password | show-password | twostep-begin | twostep-finish CODE | twostep-off"
            );
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        tracing::error!(error = %format!("{e:#}"), "command failed");
        eprintln!("ERROR={e:#}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn init_file_logging(which: &str) {
    use std::sync::{Arc, Mutex};

    let Ok(dir) = config::data_dir() else { return };
    let path = dir.join(format!("{which}.log"));
    let open = |path: &std::path::Path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    };
    let Some(file) = open(&path) else {
        return;
    };

    /// Appends to the log file; if the file was deleted or moved while we
    /// run, starts a new one at the same place (checked every few seconds).
    struct Log {
        path: std::path::PathBuf,
        file: std::fs::File,
        checked: std::time::Instant,
    }

    #[derive(Clone)]
    struct FileWriter(Arc<Mutex<Log>>);
    impl std::io::Write for FileWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let Ok(mut log) = self.0.lock() else {
                return Ok(0);
            };
            if log.checked.elapsed() >= std::time::Duration::from_secs(5) {
                log.checked = std::time::Instant::now();
                if !log.path.exists() {
                    if let Ok(f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log.path)
                    {
                        log.file = f;
                    }
                }
            }
            std::io::Write::write(&mut log.file, buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0
                .lock()
                .map(|mut log| std::io::Write::flush(&mut log.file))
                .unwrap_or(Ok(()))
        }
    }

    let writer = FileWriter(Arc::new(Mutex::new(Log {
        path,
        file,
        checked: std::time::Instant::now(),
    })));
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .try_init();
}
