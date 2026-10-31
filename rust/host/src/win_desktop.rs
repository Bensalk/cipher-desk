//! Following the Windows **input desktop** (Share screen for the sign-in screen).
//!
//! Windows keeps the sign-in screen, the lock screen, UAC prompts and
//! Ctrl+Alt+Del on a separate, protected desktop called **Winlogon**, apart
//! from the normal **Default** desktop where your programs run. A program can
//! only capture or send input to the desktop its thread is attached to, and
//! only a process running as the system account may attach to Winlogon. So the
//! Host service's worker (which runs as the system account) calls
//! [`follow_input_desktop`] before each screen grab and each input event: it
//! attaches this thread to whichever desktop currently has the input, moving
//! with it as Windows switches between the sign-in screen and the normal
//! desktop.
//!
//! This is the one capability that needs the background service; nothing here
//! runs in the ordinary (in-session) Share screen.
#![allow(unsafe_code)]

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, SetThreadDesktop,
    DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, HDESK, UOI_NAME,
};

// Enough access to read the screen and inject input on the desktop.
const ACCESS: DESKTOP_ACCESS_FLAGS = DESKTOP_ACCESS_FLAGS(
    0x0001 // DESKTOP_READOBJECTS
        | 0x0080 // DESKTOP_WRITEOBJECTS
        | 0x0040 // DESKTOP_ENUMERATE
        | 0x0100 // DESKTOP_SWITCHDESKTOP
        | 0x0002 // DESKTOP_CREATEWINDOW
        | 0x0020000, // DESKTOP_READ_CONTROL
);

thread_local! {
    // The desktop this thread is currently attached to, kept alive so the
    // thread keeps it. One per thread (capture and input run on their own).
    static CURRENT: core::cell::RefCell<Option<Attached>> = const { core::cell::RefCell::new(None) };
}

struct Attached {
    handle: HDESK,
    name: String,
}

// SAFETY: HDESK is only ever used on the thread that stored it (thread_local),
// so it never crosses threads despite not being Send.
unsafe impl Send for Attached {}

/// Attaches this thread to the desktop that currently has the input (the
/// sign-in screen or the normal desktop), if it is not already. Returns the
/// desktop's name on success. Call before capturing the screen or sending
/// input from the Host service worker.
///
/// On the ordinary desktop the name is `Default`; on the sign-in screen, lock
/// screen, UAC prompt or Ctrl+Alt+Del it is `Winlogon`.
pub fn follow_input_desktop() -> anyhow::Result<String> {
    // SAFETY: OpenInputDesktop returns a desktop handle we own; we read its
    // name, hand it to SetThreadDesktop only on success, and otherwise close
    // it. The handle currently assigned to the thread is kept in CURRENT and
    // closed only after a new one has replaced it, so the thread always has a
    // valid desktop.
    unsafe {
        let desk = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, ACCESS)
            .map_err(|e| anyhow::anyhow!("open input desktop: {e}"))?;
        let name = desktop_name(desk).unwrap_or_default();

        let already =
            CURRENT.with(|c| c.borrow().as_ref().map(|a| a.name == name).unwrap_or(false));
        if already {
            // Same desktop as last time: nothing to change; drop the extra handle.
            let _ = CloseDesktop(desk);
            return Ok(name);
        }

        if let Err(e) = SetThreadDesktop(desk) {
            let _ = CloseDesktop(desk);
            return Err(anyhow::anyhow!("attach to desktop {name}: {e}"));
        }

        // Now attached to `desk`; release the previous one.
        CURRENT.with(|c| {
            if let Some(old) = c.borrow_mut().replace(Attached {
                handle: desk,
                name: name.clone(),
            }) {
                let _ = CloseDesktop(old.handle);
            }
        });
        Ok(name)
    }
}

/// Reads a desktop object's name (e.g. "Default" or "Winlogon").
///
/// # Safety
/// `desk` must be a valid desktop handle.
unsafe fn desktop_name(desk: HDESK) -> Option<String> {
    let handle = HANDLE(desk.0);
    let mut needed = 0u32;
    // SAFETY: first call asks for the needed length (Windows may return an
    // error here); then we read exactly that many bytes into a UTF-16 buffer.
    unsafe {
        let _ = GetUserObjectInformationW(handle, UOI_NAME, None, 0, Some(&mut needed));
        if needed == 0 || needed > 1024 {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            Some(buf.as_mut_ptr().cast()),
            needed,
            None,
        )
        .ok()?;
        // The buffer holds a null-terminated wide string.
        let wide: &[u16] =
            core::slice::from_raw_parts(buf.as_ptr().cast::<u16>(), (needed as usize) / 2);
        let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        Some(String::from_utf16_lossy(&wide[..end]))
    }
}
