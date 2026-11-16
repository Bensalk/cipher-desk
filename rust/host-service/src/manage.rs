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
/// the file named after `--result` (in the app's own folder for the person
/// who asked: the app reads it there, because a command run with an
/// elevation prompt cannot send its output back directly). Results can hold
/// the sharing password, so they never go to the shared service folder.
#[derive(Default)]
struct Report {
    lines: Vec<String>,
}

/// File name the app asks results to be written to.
const RESULT_NAME: &str = "service-result.txt";

/// The `--result` file, if given and safe: named [`RESULT_NAME`], in an
/// existing folder named "Cipher Desk", neither of them a link.
fn result_path() -> Option<std::path::PathBuf> {
    let mut args = std::env::args_os().skip_while(|a| a != "--result");
    args.next()?;
    let path = std::path::PathBuf::from(args.next()?);
    let parent = path.parent()?;
    let named = path.file_name().is_some_and(|n| n == RESULT_NAME)
        && parent.file_name().is_some_and(|n| n == "Cipher Desk");
    if !named || !parent.is_dir() || crate::secure::is_link(parent) || crate::secure::is_link(&path)
    {
        return None;
    }
    Some(path)
}

impl Report {
    fn set(&mut self, key: &str, value: &str) {
        self.lines.push(format!("{key}={value}"));
    }

    /// Writes the result file and prints the lines, then any closing note.
    ///
    /// `result_path()` already refused a link at the time it checked; this
    /// narrows the window between that check and actually writing by
    /// re-checking right before creating the file, and again right after —
    /// a normal user controls this folder, so a link that appears between
    /// the two checks, or even between `create_new` succeeding and the
    /// attributes check below, is refused rather than written to. This does
    /// not close the window entirely (that needs verifying the open
    /// handle's own final path, not just re-reading attributes by name),
    /// but it is a meaningful narrowing with only safe, already-used code.
    fn finish(self, note: &str) {
        if let Some(path) = result_path() {
            use std::io::Write as _;
            let _ = std::fs::remove_file(&path);
            if !crate::secure::is_link(&path)
                && path.parent().is_some_and(|p| !crate::secure::is_link(p))
            {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    if crate::secure::is_link(&path) {
                        // Something replaced it with a link in the instant
                        // between create_new succeeding and this check.
                        // Refuse it: remove whatever is there now and write
                        // nothing.
                        drop(f);
                        let _ = std::fs::remove_file(&path);
                        tracing::warn!(
                            "refused to write the result file: it became a link"
                        );
                    } else {
                        let _ = f.write_all(self.lines.join("\n").as_bytes());
                        drop(f);
                        // The file may hold the sharing password: lock it to
                        // Windows itself and administrators, the same as a
                        // secret, rather than relying only on the containing
                        // folder's permissions (which this command did not
                        // create).
                        if let Err(e) = crate::secure::lock_result_file(&path) {
                            tracing::warn!(
                                error = %e,
                                "could not restrict who may read the result file"
                            );
                        }
                    }
                }
            } else {
                tracing::warn!("refused to write the result file: a link appeared");
            }
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

/// Where the service program is kept: under Program Files, which only
/// administrators can change. (The app itself may be installed in the
/// person's own folder, which programs they run could change; the service
/// runs as the system account, so it must not run from there.)
pub fn installed_exe() -> anyhow::Result<std::path::PathBuf> {
    let base = std::env::var_os("ProgramFiles").context("ProgramFiles is not set")?;
    Ok(std::path::PathBuf::from(base)
        .join("Cipher Desk Host")
        .join("cipher-desk-host-service.exe"))
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Stops the service if it runs and waits (up to 20 s) until it has.
fn stop_and_wait(service: &windows_service::service::Service) {
    if service
        .query_status()
        .is_ok_and(|s| s.current_state == ServiceState::Stopped)
    {
        return;
    }
    let _ = service.stop();
    for _ in 0..40 {
        if service
            .query_status()
            .is_ok_and(|s| s.current_state == ServiceState::Stopped)
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Copies this program to [`installed_exe`] (unless it already runs from
/// there), retrying while the old copy is still closing.
fn copy_program(target: &std::path::Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("find this program")?;
    if same_file(&exe, target) {
        return Ok(());
    }
    let dir = target.parent().context("program folder")?;
    if crate::secure::is_link(dir) {
        anyhow::bail!("the program folder is a link; not installing there");
    }
    std::fs::create_dir_all(dir).context("create the program folder")?;
    let tmp = target.with_extension("new");
    std::fs::copy(&exe, &tmp).context("copy the service program")?;
    let mut last = None;
    for _ in 0..20 {
        match std::fs::rename(&tmp, target) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(anyhow::anyhow!(
        "replace the service program: {}",
        last.map_or_else(String::new, |e| e.to_string())
    ))
}

/// Only administrators and Windows itself may stop, change or remove the
/// service; everyone may see whether it runs.
/// The person signed in at the computer may also start it and send it "Go
/// online" (CR), so opening the app turns sharing back on after "Exit" with
/// no prompt. Stopping, changing or removing it needs an administrator.
const SERVICE_SDDL: &str = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWRPLOCRRC;;;IU)(A;;CCLCSWLORC;;;SU)(A;;CCLCSWLORC;;;AU)";

fn restrict_service() {
    let Some(windir) = std::env::var_os("SystemRoot") else {
        return;
    };
    let sc = std::path::Path::new(&windir)
        .join("System32")
        .join("sc.exe");
    let ok = std::process::Command::new(sc)
        .args(["sdset", SERVICE_NAME, SERVICE_SDDL])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        tracing::warn!("could not restrict who may control the service");
    }
}

/// Installs the service (auto-start), or updates it to this version, makes
/// sure a sharing password exists, and prints the details the app shows on
/// the Share screen page. Safe to run again (that is how the app updates
/// the service after the app itself was updated).
pub fn install() -> anyhow::Result<()> {
    let dir = crate::config::data_dir()?;
    crate::secure::lock_down(&dir)?;
    let target = installed_exe()?;
    let mgr = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: target.clone(),
        launch_arguments: vec![OsString::from("run-service")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    match mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::CHANGE_CONFIG,
    ) {
        Ok(service) => {
            // Already installed: replace the program (it must be stopped
            // first) and point the service at the safe copy.
            stop_and_wait(&service);
            copy_program(&target)?;
            service
                .change_config(&info)
                .map_err(|e| anyhow::anyhow!("update the service: {e}"))?;
        }
        Err(_) => {
            copy_program(&target)?;
            let service = mgr
                .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
                .map_err(|e| anyhow::anyhow!("create the service: {e}"))?;
            let _ = service.set_description(
                "Lets Cipher Desk share this computer's screen, including the sign-in screen, \
                 so you can sign in from another device. Remove it from Cipher Desk's Share screen page.",
            );
        }
    }
    restrict_service();

    let password = crate::config::ensure_password()?;
    let settings = crate::config::load_settings();
    crate::config::save_settings(&settings)?;
    // Makes the certificate the first time; then lock its private key.
    let _ = rdpv_host::fingerprint(&dir);
    crate::secure::lock_down(&dir)?;
    let fingerprint =
        rdpv_host::cert_fingerprint(&dir).unwrap_or_else(|_| "unavailable".to_owned());
    let _ = std::fs::write(dir.join("version.txt"), env!("CARGO_PKG_VERSION"));

    start()?;

    let mut r = Report::default();
    r.set("INSTALLED", "1");
    r.set("USERNAME", &settings.username);
    r.set("PORT", &settings.port.to_string());
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
    // The program copy under Program Files (a moment after the service
    // stopped, so the file is free).
    if let Ok(target) = installed_exe() {
        let exe = std::env::current_exe().ok();
        if !exe.is_some_and(|e| same_file(&e, &target)) {
            for _ in 0..10 {
                if std::fs::remove_file(&target).is_ok() || !target.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if let Some(dir) = target.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
    if purge {
        if let Ok(dir) = crate::config::data_dir() {
            if let Ok(secrets) = crate::secure::secrets_dir(&dir) {
                let _ = std::fs::remove_file(secrets.join("password.bin"));
            }
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

/// "Go online…" from the tray icon or the Start menu: turns sharing back on
/// after "Go offline" or "Exit", or starts the service if it is not
/// running. Needs an administrator; asks for one if run without.
pub fn go_online() -> anyhow::Result<()> {
    use windows_service::service::UserEventCode;

    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let service = match mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::START | ServiceAccess::QUERY_STATUS | ServiceAccess::USER_DEFINED_CONTROL,
    ) {
        Ok(s) => s,
        Err(e) => {
            let quiet = std::env::args().any(|a| a == "--quiet");
            if quiet && e.to_string().contains("1060") {
                // Not installed (opening the app on any PC): nothing to do.
                return Ok(());
            }
            if quiet || std::env::args().any(|a| a == "--elevated") {
                return Err(anyhow::anyhow!("open the service: {e}"));
            }
            // Not an administrator yet: ask Windows for the prompt.
            return elevate_self("go-online --elevated");
        }
    };
    let running = service
        .query_status()
        .is_ok_and(|s| s.current_state == ServiceState::Running);
    if !running {
        return service
            .start::<OsString>(&[])
            .map_err(|e| anyhow::anyhow!("start the service: {e}"));
    }
    let code = UserEventCode::from_raw(crate::service::GO_ONLINE_CONTROL)
        .map_err(|e| anyhow::anyhow!("control code: {e:?}"))?;
    service
        .notify(code)
        .map_err(|e| anyhow::anyhow!("turn sharing back on: {e}"))?;
    Ok(())
}

/// Runs this program again with `args`, through the administrator prompt.
#[allow(unsafe_code)]
pub fn elevate_self(args: &str) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = std::env::current_exe().context("find this program")?;
    let exe: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let args: Vec<u16> = args.encode_utf16().chain([0]).collect();
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe.as_ptr()),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    // Values above 32 mean it started; otherwise refused or cancelled.
    if (result.0 as isize) <= 32 {
        anyhow::bail!("the administrator prompt was declined");
    }
    Ok(())
}

/// Changes one setting (`ask=on|off` or `networks=tailscale|any`) and
/// restarts sharing so it applies (a connected viewer is dropped).
pub fn set(change: Option<String>) -> anyhow::Result<()> {
    let change = change.context("missing setting")?;
    let mut settings = crate::config::load_settings();
    match change.as_str() {
        "ask=on" => settings.ask = true,
        "ask=off" => {
            settings.ask = false;
            // Turning it off also forgets devices that were always allowed.
            crate::config::forget_allowed_devices()?;
        }
        "networks=tailscale" => settings.tailnet_only = true,
        "networks=any" => settings.tailnet_only = false,
        other => anyhow::bail!("unknown setting: {other}"),
    }
    crate::secure::lock_down(&crate::config::data_dir()?)?;
    crate::config::save_settings(&settings)?;
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    if let Ok(service) = mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    ) {
        let was_running = service
            .query_status()
            .is_ok_and(|s| s.current_state == ServiceState::Running);
        if was_running {
            stop_and_wait(&service);
            let _ = service.start::<OsString>(&[]);
        }
    }
    let mut r = Report::default();
    r.set("ASK", if settings.ask { "1" } else { "0" });
    r.set(
        "TAILSCALE_ONLY",
        if settings.tailnet_only { "1" } else { "0" },
    );
    r.finish("");
    Ok(())
}

pub fn status() -> anyhow::Result<()> {
    let mgr = manager(ServiceManagerAccess::CONNECT)?;
    let installed = mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
    );
    let mut r = Report::default();
    match installed {
        Ok(service) => {
            let running = service
                .query_status()
                .map(|s| s.current_state == ServiceState::Running)
                .unwrap_or(false);
            r.set("INSTALLED", "1");
            r.set("RUNNING", &u8::from(running).to_string());
            let settings = crate::config::load_settings();
            let dir = crate::config::data_dir()?;
            r.set("USERNAME", &settings.username);
            r.set("PORT", &settings.port.to_string());
            // The certificate only: the private key is not readable here.
            if let Ok(fp) = rdpv_host::cert_fingerprint(&dir) {
                r.set("FINGERPRINT", &fp);
            }
            r.set("TWOSTEP", if settings.twostep { "1" } else { "0" });
            r.set("ASK", if settings.ask { "1" } else { "0" });
            r.set(
                "TAILSCALE_ONLY",
                if settings.tailnet_only { "1" } else { "0" },
            );
            // The installed service is older than this app (or still runs
            // from the app's own folder): the app offers to update it.
            let installed = std::fs::read_to_string(dir.join("version.txt")).unwrap_or_default();
            let safe_place = service
                .query_config()
                .ok()
                .zip(installed_exe().ok())
                .is_some_and(|(c, t)| {
                    // The stored command line: "<program>" run-service.
                    let line = c.executable_path.to_string_lossy().to_lowercase();
                    line.contains(&t.to_string_lossy().to_lowercase())
                });
            let current = installed.trim() == env!("CARGO_PKG_VERSION");
            r.set(
                "NEEDS_UPDATE",
                if current && safe_place { "0" } else { "1" },
            );
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

/// Imports a user-chosen password the app wrote (machine-encrypted) to
/// `path`, storing it as the service's sharing password and deleting the
/// pending file. The password is never passed on the command line — only the
/// file path is.
pub fn import_password(path: Option<String>) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let path = path.context("missing pending-password file path")?;
    let path = std::path::Path::new(&path);
    if path.file_name().is_none_or(|n| n != "pending-password.bin") || crate::secure::is_link(path)
    {
        anyhow::bail!("not a pending-password file");
    }
    let plain = rdpv_host::winsecret::read_pending(path)?;
    crate::config::save_password(&rdpv_core::secret::SecretString::new(plain))?;
    let mut r = Report::default();
    r.set("PASSWORD_SET", "1");
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

/// Starts setting up two-step verification: makes a new secret (kept pending
/// until confirmed) and reports the setup key and link for the authenticator
/// app. Nothing changes for viewers yet.
pub fn twostep_begin() -> anyhow::Result<()> {
    let totp = rdpv_host::totp::Totp::generate()?;
    crate::config::save_twostep_pending(&totp)?;
    let account = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this computer".to_owned());
    let mut r = Report::default();
    r.set("TWOSTEP_KEY", &totp.setup_key());
    r.set("TWOSTEP_URI", &totp.uri(&account));
    r.set("TWOSTEP_ACCOUNT", &account);
    // The QR code to scan, as rows of 1 (dark) / 0 (light) separated by '|'.
    if let Some(rows) = totp.qr_rows(&account) {
        r.set("TWOSTEP_QR", &rows.join("|"));
    }
    r.finish("Add the key to your authenticator app, then confirm with a code.");
    Ok(())
}

/// Confirms the pending secret with a code from the app and turns two-step
/// verification on. The code is a short-lived number, not a secret to keep.
pub fn twostep_finish(code: Option<String>) -> anyhow::Result<()> {
    let code = code.context("missing code")?;
    let pending = crate::config::load_twostep_pending().context("no two-step setup in progress")?;
    let mut r = Report::default();
    if pending.verify(&code, std::time::SystemTime::now()) {
        crate::config::activate_twostep(&pending)?;
        r.set("TWOSTEP", "1");
    } else {
        r.set("TWOSTEP", "0");
        r.set("TWOSTEP_WRONG_CODE", "1");
    }
    r.finish("");
    Ok(())
}

/// Turns two-step verification off.
pub fn twostep_off() -> anyhow::Result<()> {
    crate::config::remove_twostep()?;
    let mut r = Report::default();
    r.set("TWOSTEP", "0");
    r.finish("Two-step verification is off.");
    Ok(())
}
