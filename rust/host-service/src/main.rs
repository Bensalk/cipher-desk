//! Cipher Desk Host service (Windows): lets Share screen reach the Windows
//! sign-in screen, so you can sign in to this computer from another device.
//!
//! Commands (the app runs the admin ones with an elevation prompt):
//!   install | uninstall [--purge] | start | stop | status | new-password | show-password
//!   run-service   (used by Windows to start the service)
//!   worker        (started by the service in the active session)
//!
//! This whole binary is Windows-only; on other systems it just says so.
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[cfg(windows)]
mod config;
#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod manage;
#[cfg(windows)]
mod service;
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
    if matches!(command.as_str(), "run-service" | "worker") {
        init_file_logging(&command);
    }

    let result = match command.as_str() {
        "run-service" => service::run(),
        "worker" => worker::run(),
        "install" => manage::install(),
        "uninstall" | "remove" => {
            let purge = std::env::args().any(|a| a == "--purge");
            manage::uninstall(purge)
        }
        "start" => manage::start(),
        "stop" => manage::stop(),
        "status" => manage::status(),
        "new-password" => manage::new_password(),
        "show-password" => manage::show_password(),
        other => {
            eprintln!("unknown command: {other:?}");
            eprintln!(
                "use: install | uninstall | start | stop | status | new-password | show-password"
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
    use std::io::Write as _;
    use std::sync::{Arc, Mutex};

    let Ok(dir) = config::data_dir() else { return };
    let path = dir.join(format!("{which}.log"));
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
            self.0.lock().map(|mut f| f.write(buf)).unwrap_or(Ok(0))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.lock().map(|mut f| f.flush()).unwrap_or(Ok(()))
        }
    }

    let writer = FileWriter(Arc::new(Mutex::new(file)));
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .try_init();
}
