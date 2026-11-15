//! Launching the worker into the active session as the system account.
//!
//! The service runs in "session 0", which has no screen. To reach the screen
//! (including the sign-in screen), it starts a **worker** copy of itself in the
//! session that is actually on the monitor, still as the system account so the
//! worker may follow the protected Winlogon desktop. The worker is given a
//! fresh copy of the service's own token with its session id changed to the
//! active one, which works even when nobody is signed in.
#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use anyhow::Context as _;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    DuplicateTokenEx, SecurityImpersonation, SetTokenInformation, TokenPrimary, TokenSessionId,
    TOKEN_ACCESS_MASK, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken, CREATE_NO_WINDOW,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

/// A running worker process. Dropping it does nothing; call [`Worker::stop`].
pub struct Worker {
    process: HANDLE,
    thread: HANDLE,
    pub session: u32,
}

impl Worker {
    /// Still running? (false once it has exited.)
    pub fn is_alive(&self) -> bool {
        use windows::Win32::Foundation::{STILL_ACTIVE, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        // SAFETY: `process` is a valid handle we own until `stop`.
        unsafe {
            if WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 {
                return false;
            }
            let mut code = 0u32;
            if GetExitCodeProcess(self.process, &mut code).is_ok() {
                return code == STILL_ACTIVE.0 as u32;
            }
            true
        }
    }

    /// The exit code once it has ended (`None` while running).
    pub fn exit_code(&self) -> Option<u32> {
        use windows::Win32::Foundation::STILL_ACTIVE;
        use windows::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0u32;
        // SAFETY: `process` is a valid handle we own until `stop`.
        unsafe { GetExitCodeProcess(self.process, &mut code) }.ok()?;
        (code != STILL_ACTIVE.0 as u32).then_some(code)
    }

    /// Ends the worker and closes its handles.
    pub fn stop(self) {
        use windows::Win32::System::Threading::TerminateProcess;
        // SAFETY: both handles are ours; after terminating we close them.
        unsafe {
            let _ = TerminateProcess(self.process, 1);
            let _ = CloseHandle(self.process);
            let _ = CloseHandle(self.thread);
        }
    }
}

/// The session currently shown on the monitor, or `None` if there isn't one.
pub fn active_session() -> Option<u32> {
    // SAFETY: no arguments; returns 0xFFFFFFFF when there is no active session.
    let id = unsafe { WTSGetActiveConsoleSessionId() };
    (id != u32::MAX).then_some(id)
}

/// Starts the worker (`exe worker`) in `session`, as the system account, on the
/// interactive window station so it can follow the input desktop.
pub fn spawn_worker(exe: &Path, session: u32) -> anyhow::Result<Worker> {
    let mut token = HANDLE::default();
    let mut dup = HANDLE::default();
    // SAFETY: each Win32 call is checked; handles are closed on every path, and
    // the duplicated token is used only for CreateProcessAsUserW.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ACCESS_MASK(
                TOKEN_DUPLICATE.0
                    | TOKEN_QUERY.0
                    | TOKEN_ASSIGN_PRIMARY.0
                    | TOKEN_ADJUST_DEFAULT.0
                    | TOKEN_ADJUST_SESSIONID.0,
            ),
            &mut token,
        )
        .context("open the service token")?;

        let dup_result = DuplicateTokenEx(
            token,
            TOKEN_ACCESS_MASK(
                TOKEN_DUPLICATE.0
                    | TOKEN_QUERY.0
                    | TOKEN_ASSIGN_PRIMARY.0
                    | TOKEN_ADJUST_DEFAULT.0
                    | TOKEN_ADJUST_SESSIONID.0,
            ),
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut dup,
        );
        let _ = CloseHandle(token);
        dup_result.context("duplicate the service token")?;

        // Move the copy into the session that is on the monitor.
        let set = SetTokenInformation(
            dup,
            TokenSessionId,
            core::ptr::addr_of!(session).cast(),
            u32::try_from(core::mem::size_of::<u32>()).unwrap_or(4),
        );
        if let Err(e) = set {
            let _ = CloseHandle(dup);
            return Err(anyhow::anyhow!("set the worker session: {e}"));
        }

        let started = start_as(dup, exe, "worker", None, session);
        let _ = CloseHandle(dup);
        started.context("start the worker in the active session")
    }
}

/// Starts the tray icon (`exe tray`) in `session` as the person signed in
/// there. Fails when nobody is signed in (the sign-in screen).
pub fn spawn_tray(exe: &Path, session: u32) -> anyhow::Result<Worker> {
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::RemoteDesktop::WTSQueryUserToken;

    let mut token = HANDLE::default();
    // SAFETY: valid out-pointers; the token and environment block are freed
    // on every path after the process has been created.
    unsafe {
        WTSQueryUserToken(session, &mut token).context("nobody is signed in")?;
        let mut env: *mut core::ffi::c_void = core::ptr::null_mut();
        let have_env = CreateEnvironmentBlock(&mut env, Some(token), false).is_ok();
        let started = start_as(
            token,
            exe,
            "tray",
            have_env.then_some(env.cast_const()),
            session,
        );
        if have_env {
            let _ = DestroyEnvironmentBlock(env);
        }
        let _ = CloseHandle(token);
        started.context("start the tray icon")
    }
}

/// `CreateProcessAsUserW` for `"exe" <arg>` with `token`, no window, on the
/// interactive desktop.
///
/// # Safety
/// `token` must be a valid primary token; `env`, if given, a valid
/// environment block that outlives the call.
unsafe fn start_as(
    token: HANDLE,
    exe: &Path,
    arg: &str,
    env: Option<*const core::ffi::c_void>,
    session: u32,
) -> anyhow::Result<Worker> {
    let app: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let quoted = format!("\"{}\" {arg}", exe.display());
    let mut cmd: Vec<u16> = quoted.encode_utf16().chain([0]).collect();
    // The interactive window station.
    let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain([0]).collect();
    let si = STARTUPINFOW {
        cb: u32::try_from(core::mem::size_of::<STARTUPINFOW>()).unwrap_or(0),
        lpDesktop: PWSTR(desktop.as_mut_ptr()),
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: the caller guarantees `token` and `env`; the string buffers
    // and `si` outlive the call.
    unsafe {
        CreateProcessAsUserW(
            Some(token),
            PCWSTR(app.as_ptr()),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            env,
            PCWSTR::null(),
            &si,
            &mut pi,
        )?;
    }
    let _ = (&app, &cmd, &desktop, &si);
    Ok(Worker {
        process: pi.hProcess,
        thread: pi.hThread,
        session,
    })
}
