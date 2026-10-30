//! Install, remove and inspect the service. These need administrator rights;
//! the app runs them with a Windows elevation prompt. They print `KEY=value`
//! lines the app reads, plus plain text for a person running them by hand.

use std::ffi::OsString;

use anyhow::Context as _;
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::service::{DISPLAY_NAME, SERVICE_NAME};

/// Collects `KEY=value` lines, prints them (for a person) and writes them to
/// `last-result.txt` in the service folder. The app reads that file, because
/// a command run with an elevation prompt cannot send its output back directly.
#[derive(Default)]
struct Report {
    lines: Vec<String>,
}

impl Report {
    fn set(&mut self, key: &str, value: &str) {
        self.lines.push(format!("{key}={value}"));
    }

    /// Writes the result file and prints the lines, then any closing note.
    fn finish(self, note: &str) {
        if let Ok(dir) = crate::config::data_dir() {
            let _ = std::fs::write(dir.join("last-result.txt"), self.lines.join("\n"));
        }
        for line in &self.lines {
            println!("{line}");
        }
        if !note.is_empty() {
            println!("\n{note}");
        }
    }
}

fn manager(access: ServiceManagerAccess) -> anyhow::Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access)
        .map_err(|e| anyhow::anyhow!("open the service manager (need administrator): {e}"))
}

/// Installs the service (auto-start), makes sure a sharing password exists, and
/// prints the details the app shows on the Share screen page.
pub fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("find this program")?;
    let mgr = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;

    // Already installed? Leave it; just refresh the details below.
    let existing = mgr.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS);
    if existing.is_err() {
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec![OsString::from("run-service")],
            dependencies: vec![],
            account_name: None, // LocalSystem
            account_password: None,
        };
        let service = mgr
            .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
            .map_err(|e| anyhow::anyhow!("create the service: {e}"))?;
        let _ = service.set_description(
            "Lets Cipher Desk share this computer's screen, including the sign-in screen, \
             so you can sign in from another device. Remove it from Cipher Desk's Share screen page.",
        );
    }

    let password = crate::config::ensure_password()?;
    let (username, port) = crate::config::load_settings();
    crate::config::save_settings(&username, port)?;
    let fingerprint = rdpv_host::fingerprint(&crate::config::data_dir()?)
        .unwrap_or_else(|_| "unavailable".to_owned());

    start()?;

    let mut r = Report::default();
    r.set("INSTALLED", "1");
    r.set("USERNAME", &username);
    r.set("PORT", &port.to_string());
    r.set("FINGERPRINT", &fingerprint);
    r.set("PASSWORD", password.expose());
    r.finish(
        "Sign-in screen sharing is on. On the other device, connect with method \
         \"Share screen\" using the username, port and password above.",
    );
    Ok(())
}

/// Stops and removes the service. The sharing password file is left in place
/// unless `purge` is set.
pub fn uninstall(purge: bool) -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    if let Ok(service) = mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
    ) {
        if let Ok(status) = service.query_status() {
            if status.current_state != ServiceState::Stopped {
                let _ = service.stop();
            }
        }
        service
            .delete()
            .map_err(|e| anyhow::anyhow!("remove the service: {e}"))?;
    }
    if purge {
        if let Ok(dir) = crate::config::data_dir() {
            let _ = std::fs::remove_file(dir.join("password.bin"));
        }
    }
    let mut r = Report::default();
    r.set("REMOVED", "1");
    r.set("INSTALLED", "0");
    r.finish("Sign-in screen sharing has been turned off and the service removed.");
    Ok(())
}

pub fn start() -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let service = mgr
        .open_service(
            SERVICE_NAME,
            ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| anyhow::anyhow!("open the service: {e}"))?;
    if let Ok(status) = service.query_status() {
        if status.current_state == ServiceState::Running {
            return Ok(());
        }
    }
    service
        .start::<OsString>(&[])
        .map_err(|e| anyhow::anyhow!("start the service: {e}"))?;
    Ok(())
}

pub fn stop() -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    if let Ok(service) = mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    ) {
        let _ = service.stop();
    }
    let mut r = Report::default();
    r.set("STOPPED", "1");
    r.finish("");
    Ok(())
}

/// Prints whether the service is installed and running, and the details the app shows.
pub fn status() -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let installed = mgr.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS);
    let mut r = Report::default();
    match installed {
        Ok(service) => {
            let running = service
                .query_status()
                .map(|s| s.current_state == ServiceState::Running)
                .unwrap_or(false);
            r.set("INSTALLED", "1");
            r.set("RUNNING", &u8::from(running).to_string());
            let (username, port) = crate::config::load_settings();
            r.set("USERNAME", &username);
            r.set("PORT", &port.to_string());
            if let Ok(fp) = rdpv_host::fingerprint(&crate::config::data_dir()?) {
                r.set("FINGERPRINT", &fp);
            }
        }
        Err(_) => r.set("INSTALLED", "0"),
    }
    r.finish("");
    Ok(())
}

/// Replaces the service's sharing password and reports the new one.
pub fn new_password() -> anyhow::Result<()> {
    let password = rdpv_host::generate_password()?;
    crate::config::save_password(&password)?;
    let mut r = Report::default();
    r.set("PASSWORD", password.expose());
    r.finish("");
    Ok(())
}

/// Reports the current sharing password (the app shows it when asked).
pub fn show_password() -> anyhow::Result<()> {
    let mut r = Report::default();
    match crate::config::load_password()? {
        Some(p) => r.set("PASSWORD", p.expose()),
        None => r.set("PASSWORD", ""),
    }
    r.finish("");
    Ok(())
}
