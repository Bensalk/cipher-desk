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
    DuplicateTokenEx, SecurityIdentification, SetTokenInformation, TokenPrimary, TokenSessionId,
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
            SecurityIdentification,
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

        let app: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
        let quoted = format!("\"{}\" worker", exe.display());
        let mut cmd: Vec<u16> = quoted.encode_utf16().chain([0]).collect();
        // The interactive window station; the worker then follows the input desktop.
        let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain([0]).collect();

        let mut si = STARTUPINFOW {
            cb: u32::try_from(core::mem::size_of::<STARTUPINFOW>()).unwrap_or(0),
            lpDesktop: PWSTR(desktop.as_mut_ptr()),
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let created = CreateProcessAsUserW(
            Some(dup),
            PCWSTR(app.as_ptr()),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        );
        // Keep the string buffers alive until after the call used their pointers.
        let _ = (&app, &cmd, &desktop, &si);
        let _ = CloseHandle(dup);
        created.context("start the worker in the active session")?;

        Ok(Worker {
            process: pi.hProcess,
            thread: pi.hThread,
            session,
        })
    }
}
