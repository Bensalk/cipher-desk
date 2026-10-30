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

    println!("INSTALLED=1");
    println!("USERNAME={username}");
    println!("PORT={port}");
    println!("FINGERPRINT={fingerprint}");
    println!("PASSWORD={}", password.expose());
    println!("\nSign-in screen sharing is on. On the other device, connect with method");
    println!("\"Share screen\" using the username, port and password above.");
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
    println!("REMOVED=1");
    println!("Sign-in screen sharing has been turned off and the service removed.");
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
    println!("STOPPED=1");
    Ok(())
}

/// Prints whether the service is installed and running, and the details the app shows.
pub fn status() -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let installed = mgr.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS);
    match installed {
        Ok(service) => {
            let running = service
                .query_status()
                .map(|s| s.current_state == ServiceState::Running)
                .unwrap_or(false);
            println!("INSTALLED=1");
            println!("RUNNING={}", u8::from(running));
            let (username, port) = crate::config::load_settings();
            println!("USERNAME={username}");
            println!("PORT={port}");
            if let Ok(fp) = rdpv_host::fingerprint(&crate::config::data_dir()?) {
                println!("FINGERPRINT={fp}");
            }
        }
        Err(_) => println!("INSTALLED=0"),
    }
    Ok(())
}

/// Replaces the service's sharing password and prints the new one.
pub fn new_password() -> anyhow::Result<()> {
    let password = rdpv_host::generate_password()?;
    crate::config::save_password(&password)?;
    println!("PASSWORD={}", password.expose());
    Ok(())
}

/// Prints the current sharing password (the app shows it when asked).
pub fn show_password() -> anyhow::Result<()> {
    match crate::config::load_password()? {
        Some(p) => println!("PASSWORD={}", p.expose()),
        None => println!("PASSWORD="),
    }
    Ok(())
}
