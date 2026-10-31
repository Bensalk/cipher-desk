//! Windows screen capture (GDI) and input (SendInput) for Cipher Desk Host.
//!
//! This is the one module in the project that uses `unsafe`: Win32 calls have
//! no safe wrapper. Every block says why it is sound. Limits that come from
//! Windows itself: the lock screen, UAC prompts and windows of programs running
//! as administrator cannot be captured or controlled by a normal program, and
//! Ctrl+Alt+Del cannot be sent.
#![allow(unsafe_code)]

use core::ffi::c_void;

use anyhow::Context as _;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC,
    SRCCOPY,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetIconInfo, GetSystemMetrics, CURSORINFO, CURSOR_SHOWING,
    DI_NORMAL, ICONINFO, SM_CXSCREEN, SM_CYSCREEN,
};

use crate::frames::{Frame, FrameSource};
use crate::input::{Button, Input, InputSink};

fn screen_size() -> (i32, i32) {
    // SAFETY: GetSystemMetrics only reads system values.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Captures the main screen with GDI (works on every Windows version, also in
/// a Remote Desktop session and without a graphics driver).
#[derive(Default)]
pub struct ScreenCapture;

impl FrameSource for ScreenCapture {
    fn size(&mut self) -> (u16, u16) {
        let (w, h) = screen_size();
        (
            u16::try_from(w).unwrap_or(1024),
            u16::try_from(h).unwrap_or(768),
        )
    }

    fn capture(&mut self) -> anyhow::Result<Frame> {
        let (w, h) = screen_size();
        let width = u16::try_from(w).context("screen width")?;
        let height = u16::try_from(h).context("screen height")?;
        if width == 0 || height == 0 {
            anyhow::bail!("no screen");
        }
        let mut bgra = vec![0u8; usize::from(width) * usize::from(height) * 4];
        // SAFETY: every handle created here is checked, used only inside this
        // block and released before it ends; `bgra` is large enough for the
        // top-down 32-bit bitmap GetDIBits writes (width × height × 4 bytes).
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                anyhow::bail!("no screen device context");
            }
            let mem = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bitmap.into());
            let blit = BitBlt(mem, 0, 0, w, h, Some(screen), 0, 0, SRCCOPY | CAPTUREBLT);
            if blit.is_ok() {
                draw_cursor(mem);
            }
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: u32::try_from(core::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
                    biWidth: w,
                    biHeight: -h, // negative: rows from the top
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let lines = if blit.is_ok() {
                GetDIBits(
                    mem,
                    bitmap,
                    0,
                    u32::try_from(h).unwrap_or(0),
                    Some(bgra.as_mut_ptr().cast::<c_void>()),
                    &mut info,
                    DIB_RGB_COLORS,
                )
            } else {
                0
            };
            SelectObject(mem, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            blit.context("screen copy")?;
            if lines != h {
                anyhow::bail!("screen copy incomplete");
            }
        }
        Ok(Frame {
            width,
            height,
            bgra,
        })
    }
}

/// Draws the mouse pointer into the captured picture (GDI copies leave it out).
///
/// # Safety
/// `dc` must be a valid memory device context.
unsafe fn draw_cursor(dc: HDC) {
    let mut ci = CURSORINFO {
        cbSize: u32::try_from(core::mem::size_of::<CURSORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY (caller + here): `ci` is a correctly sized CURSORINFO; the cursor
    // handle comes from Windows and is only drawn, not destroyed.
    unsafe {
        if GetCursorInfo(&mut ci).is_ok() && ci.flags == CURSOR_SHOWING {
            // The pointer's tip (hotspot) is at the position, not its corner.
            let mut info = ICONINFO::default();
            let (hx, hy) = if GetIconInfo(ci.hCursor.into(), &mut info).is_ok() {
                // GetIconInfo makes copies of the pointer's bitmaps; free them.
                if !info.hbmMask.is_invalid() {
                    let _ = DeleteObject(info.hbmMask.into());
                }
                if !info.hbmColor.is_invalid() {
                    let _ = DeleteObject(info.hbmColor.into());
                }
                (
                    i32::try_from(info.xHotspot).unwrap_or(0),
                    i32::try_from(info.yHotspot).unwrap_or(0),
                )
            } else {
                (0, 0)
            };
            let _ = DrawIconEx(
                dc,
                ci.ptScreenPos.x - hx,
                ci.ptScreenPos.y - hy,
                ci.hCursor.into(),
                0,
                0,
                0,
                None,
                DI_NORMAL,
            );
        }
    }
}

/// Performs the viewer's keyboard and mouse input with SendInput.
#[derive(Default)]
pub struct SendInputSink;

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a valid slice of fully initialised INPUT values and
    // the size argument is the size of one INPUT, as SendInput requires.
    let sent = unsafe {
        SendInput(
            inputs,
            i32::try_from(core::mem::size_of::<INPUT>()).unwrap_or(0),
        )
    };
    if sent as usize != inputs.len() {
        // SAFETY: reads the thread's last-error code only.
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        tracing::warn!(sent, wanted = inputs.len(), error = ?err, "SendInput rejected input");
    }
    sent
}

fn key(scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                // Wheel distance is a signed value carried in an unsigned field.
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

impl InputSink for SendInputSink {
    fn send(&mut self, input: Input) {
        match input {
            Input::Key {
                scancode,
                extended,
                down,
            } => {
                let mut flags = KEYEVENTF_SCANCODE;
                if extended {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                send(&[key(u16::from(scancode), flags)]);
            }
            Input::Unicode { unit, down } => {
                let mut flags = KEYEVENTF_UNICODE;
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                send(&[key(unit, flags)]);
            }
            Input::MoveTo { x, y } => {
                let (w, h) = screen_size();
                // Absolute positions are 0..=65535 across the main screen.
                let nx = i32::from(x) * 65535 / (w - 1).max(1);
                let ny = i32::from(y) * 65535 / (h - 1).max(1);
                send(&[mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)]);
            }
            Input::Button { button, down } => {
                let flags = match (button, down) {
                    (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (Button::Left, false) => MOUSEEVENTF_LEFTUP,
                    (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                };
                send(&[mouse(0, 0, 0, flags)]);
            }
            Input::Wheel { delta } => {
                send(&[mouse(0, 0, i32::from(delta), MOUSEEVENTF_WHEEL)]);
            }
        }
    }
}

// -------- Host service: capture and input that follow the input desktop --------

/// Screen capture for the Host **service** worker: before each grab it attaches
/// the thread to the current input desktop, so it also shows the sign-in and
/// lock screens. Falls back to a plain grab if the desktop can't be followed.
#[derive(Default)]
pub struct ServiceScreenCapture(ScreenCapture);

impl FrameSource for ServiceScreenCapture {
    fn size(&mut self) -> (u16, u16) {
        let _ = crate::win_desktop::follow_input_desktop();
        self.0.size()
    }

    fn capture(&mut self) -> anyhow::Result<Frame> {
        // A desktop switch (login → desktop, UAC) is normal; keep going even if
        // this attach momentarily fails.
        if let Err(e) = crate::win_desktop::follow_input_desktop() {
            tracing::debug!(error = %e, "could not follow the input desktop");
        }
        self.0.capture()
    }
}

/// Input for the Host **service** worker: attaches to the current input desktop
/// before each event, so typing and clicks reach the sign-in screen too.
#[derive(Default)]
pub struct ServiceSendInputSink(SendInputSink);

impl InputSink for ServiceSendInputSink {
    fn send(&mut self, input: Input) {
        let _ = crate::win_desktop::follow_input_desktop();
        self.0.send(input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies the INPUT structures we build are well-formed enough that
    // Windows accepts them (SendInput returns the number inserted). This runs
    // on the CI Windows runner. The events are harmless: a zero-distance
    // relative mouse move and a key-up for a key that was never pressed.
    #[test]
    fn send_input_accepts_our_events() {
        let mouse_move = mouse(0, 0, 0, MOUSEEVENTF_MOVE);
        assert_eq!(send(&[mouse_move]), 1, "SendInput rejected a mouse event");

        let key_up = key(0x39, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP); // space, up
        assert_eq!(send(&[key_up]), 1, "SendInput rejected a keyboard event");
    }
}
