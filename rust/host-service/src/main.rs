//! Cipher Desk Host service (Windows): lets Share screen reach the Windows
//! sign-in screen, so you can sign in to this computer from another device.
//!
//! Commands (the app runs the admin ones with an elevation prompt):
//!   install | uninstall [--purge] | start | stop | status | new-password | show-password
//!   run-service   (used by Windows to start the service)
//!   worker        (started by the service in the active session)
//!   tray          (the status icon, started by the service as the signed-in person)
//!   go-online     (from the tray icon or the Start menu, with the administrator prompt)
//!   set ask=on|off  set networks=tailscale|any   (from the app, with the prompt)
//!
//! Admin commands the app runs take `--result <file>`: where to write the
//! `KEY=value` result (the app's own folder; see `manage`).
//!
//! This whole binary is Windows-only; on other systems it just says so.
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[cfg(windows)]
mod config;
#[cfg(windows)]
mod history;
#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod link;
#[cfg(windows)]
mod logfiles;
#[cfg(windows)]
mod manage;
// Not Windows-gated: this is pure path logic with no Windows dependency of
// its own, and its tests need to run on Linux CI too (manage.rs, which uses
// it, is Windows-only like the rest of this binary).
mod result_path;
#[cfg(windows)]
mod secure;
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

    // Admin commands first make sure the service folder is locked down (and
    // move secrets older versions kept in the open into the locked folder).
    let admin = matches!(
        command.as_str(),
        "install"
            | "uninstall"
            | "remove"
            | "new-password"
            | "import-password"
            | "show-password"
            | "twostep-begin"
            | "twostep-finish"
            | "twostep-off"
            | "set"
    );
    if admin {
        if let Err(e) = config::data_dir().and_then(|d| secure::lock_down(&d)) {
            eprintln!("ERROR={e:#}");
            std::process::exit(1);
        }
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
        "set" => manage::set(std::env::args().nth(2)),
        "import-password" => manage::import_password(std::env::args().nth(2)),
        "show-password" => manage::show_password(),
        "twostep-begin" => manage::twostep_begin(),
        "twostep-finish" => manage::twostep_finish(std::env::args().nth(2)),
        "twostep-off" => manage::twostep_off(),
        other => {
            eprintln!("unknown command: {other:?}");
            eprintln!(
                "use: install | uninstall | start | stop | status | new-password | import-password | show-password | twostep-begin | twostep-finish CODE | twostep-off | set ask=on|off | set networks=tailscale|any | go-online"
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

    // The tray runs as the signed-in person, who may read but not write the
    // service folder: its log goes to that person's own app data folder.
    let dir = if which == "tray" {
        let Some(base) = std::env::var_os("LOCALAPPDATA") else {
            return;
        };
        let dir = std::path::PathBuf::from(base).join("Cipher Desk");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        dir
    } else {
        let Ok(dir) = config::data_dir() else { return };
        dir
    };
    let path = dir.join(format!("{which}.log"));
    logfiles::tidy(&path);
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
                // Too big or too old: start afresh (see `logfiles`).
                let moved = logfiles::tidy(&log.path);
                if moved || !log.path.exists() {
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
