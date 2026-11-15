//! The Cipher Desk icon next to the clock on this computer (the one being
//! shared). It runs as the person signed in, started by the service.
//!
//! - Teal: online, waiting for a connection. Amber: someone is connected.
//!   Grey: offline (sharing is off, or starting).
//! - Right-click (or click) menu: "Disconnect viewer", "Go offline", and
//!   "Go online…" (asks for an administrator's OK, since it opens this
//!   computer to connections again).
//!
//! Its second job: because it runs as the signed-in person, it can read the
//! files that person copies in Explorer, which Windows hides from the
//! service. It does so only when the worker asks, and the worker asks only
//! while a viewer is connected (see [`crate::link`]).
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_MESSAGE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, HICON,
    ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SW_HIDE, TPM_BOTTOMALIGN,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use crate::link::{Client, Status};

/// Messages from the tray icon (clicks).
const WM_TRAY: u32 = WM_APP + 1;
const POLL_TIMER: usize = 1;
/// How often the icon asks the worker for the status (and pending requests).
const POLL_MS: u32 = 400;
/// Wait before trying again to reach a worker that is not there.
const RECONNECT: Duration = Duration::from_secs(1);

const CMD_DISCONNECT: usize = 10;
const CMD_OFFLINE: usize = 11;
const CMD_ONLINE: usize = 12;

/// Teal, amber and grey (the app's colours).
const TEAL: (u8, u8, u8) = (0x00, 0x89, 0x7B);
const AMBER: (u8, u8, u8) = (0xFF, 0xB3, 0x00);
const GREY: (u8, u8, u8) = (0x9E, 0x9E, 0x9E);

/// Explorer's "the taskbar was (re)created" message: add the icon again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct Tray {
    hwnd: HWND,
    /// Offline, online, connected.
    icons: [HICON; 3],
    status: Status,
    /// The icon is in the tray (adding fails until Explorer has started).
    shown: bool,
    link: Option<Client>,
    retry_at: Instant,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn icon_index(status: Status) -> usize {
    match status {
        Status::Offline => 0,
        Status::Online => 1,
        Status::Connected => 2,
    }
}

fn status_text(status: Status) -> &'static str {
    match status {
        Status::Offline => "Cipher Desk: offline",
        Status::Online => "Cipher Desk: online, waiting for a connection",
        Status::Connected => "Cipher Desk: someone is connected",
    }
}

pub fn run() -> anyhow::Result<()> {
    rdpv_host::win::make_dpi_aware();
    // SAFETY: standard window-class/window creation and message loop on this
    // thread; every pointer passed (class, names, message) outlives its call.
    unsafe {
        let instance = HINSTANCE::from(GetModuleHandleW(PCWSTR::null())?);
        let class = w!("CipherDeskTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            anyhow::bail!("register the tray window class");
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Cipher Desk"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?;
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::SeqCst,
        );

        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let icons = [
            make_icon(size, GREY),
            make_icon(size, TEAL),
            make_icon(size, AMBER),
        ];
        TRAY.with(|t| {
            *t.borrow_mut() = Some(Tray {
                hwnd,
                icons,
                status: Status::Offline,
                shown: false,
                link: None,
                retry_at: Instant::now(),
            });
        });
        poll();
        SetTimer(Some(hwnd), POLL_TIMER, POLL_MS, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        TRAY.with(|t| {
            if let Ok(guard) = t.try_borrow() {
                if let Some(tray) = guard.as_ref() {
                    notify(tray, NIM_DELETE, None);
                }
            }
        });
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == POLL_TIMER => {
            poll();
            LRESULT(0)
        }
        WM_TRAY => {
            let mouse = u32::try_from(lparam.0 & 0xFFFF).unwrap_or(0);
            if mouse == WM_RBUTTONUP || mouse == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            command(wparam.0 & 0xFFFF);
            LRESULT(0)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            // Explorer restarted: the icon is gone, add it again.
            with_tray(|tray| tray.shown = notify(tray, NIM_ADD, None));
            LRESULT(0)
        }
        // SAFETY: the default handling for every other message.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Runs `f` on the tray state, unless it is already in use further up this
/// thread (a dialog or OLE call can run messages while we wait on it).
fn with_tray<T>(f: impl FnOnce(&mut Tray) -> T) -> Option<T> {
    TRAY.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Asks the worker for the status; answers its request for copied files.
fn poll() {
    with_tray(|tray| {
        let status = ask_worker(tray);
        if !tray.shown {
            tray.status = status;
            tray.shown = notify(tray, NIM_ADD, None);
        } else if status != tray.status {
            let previous = tray.status;
            tray.status = status;
            notify(tray, NIM_MODIFY, popup_for(previous, status));
        }
    });
}

/// The pop-up text for a status change a person should be told about right
/// away (a viewer connecting or leaving), or `None` for a change that should
/// stay quiet (for example going offline, which the person just asked for).
fn popup_for(previous: Status, status: Status) -> Option<(&'static str, &'static str)> {
    match (previous, status) {
        (_, Status::Connected) => Some(("Cipher Desk", "Someone connected to this computer.")),
        (Status::Connected, Status::Online) => Some(("Cipher Desk", "The viewer disconnected.")),
        _ => None,
    }
}

fn ask_worker(tray: &mut Tray) -> Status {
    if tray.link.is_none() {
        if Instant::now() < tray.retry_at {
            return Status::Offline;
        }
        match Client::connect() {
            Ok(c) => tray.link = Some(c),
            Err(_) => {
                tray.retry_at = Instant::now() + RECONNECT;
                return Status::Offline;
            }
        }
    }
    let Some(link) = tray.link.as_mut() else {
        return Status::Offline;
    };
    let result = link.poll().and_then(|(status, want)| {
        if want > 0 {
            // A viewer is connected and something was just copied here.
            let files = rdpv_host::clipboard::copied_files().unwrap_or_default();
            tracing::info!(count = files.len(), "tray: sent the copied files");
            link.send_files(want, &files)?;
        }
        Ok(status)
    });
    match result {
        Ok(status) => status,
        Err(_) => {
            tray.link = None;
            tray.retry_at = Instant::now() + RECONNECT;
            Status::Offline
        }
    }
}

/// Adds, updates or removes the icon. Returns whether Windows accepted it.
/// `popup`: (title, text) to also pop up above the icon, like AnyDesk does
/// when someone connects — Windows shows this even while the Action Center
/// is closed and the tray icon is just sitting in the overflow tray.
fn notify(tray: &Tray, message: NOTIFY_ICON_MESSAGE, popup: Option<(&str, &str)>) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: u32::try_from(core::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: tray.hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: tray.icons[icon_index(tray.status)],
        ..Default::default()
    };
    let tip: Vec<u16> = status_text(tray.status)
        .encode_utf16()
        .take(nid.szTip.len() - 1)
        .collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    if let Some((title, text)) = popup {
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        let t: Vec<u16> = title
            .encode_utf16()
            .take(nid.szInfoTitle.len() - 1)
            .collect();
        nid.szInfoTitle[..t.len()].copy_from_slice(&t);
        let b: Vec<u16> = text.encode_utf16().take(nid.szInfo.len() - 1).collect();
        nid.szInfo[..b.len()].copy_from_slice(&b);
    }
    // SAFETY: `nid` is a fully initialised NOTIFYICONDATAW that outlives the call.
    unsafe { Shell_NotifyIconW(message, &nid) }.as_bool()
}

fn show_menu(hwnd: HWND) {
    let status = with_tray(|t| t.status).unwrap_or(Status::Offline);
    let title: Vec<u16> = status_text(status).encode_utf16().chain([0]).collect();
    // SAFETY: the menu is created, shown and destroyed here; the title buffer
    // outlives the AppendMenuW call that copies it.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(title.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        match status {
            Status::Connected => {
                let _ = AppendMenuW(menu, MF_STRING, CMD_DISCONNECT, w!("Disconnect viewer"));
                let _ = AppendMenuW(menu, MF_STRING, CMD_OFFLINE, w!("Go offline"));
            }
            Status::Online => {
                let _ = AppendMenuW(menu, MF_STRING, CMD_OFFLINE, w!("Go offline"));
            }
            Status::Offline => {
                let _ = AppendMenuW(menu, MF_STRING, CMD_ONLINE, w!("Go online…"));
            }
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Needed so the menu closes when you click elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

fn command(id: usize) {
    match id {
        CMD_DISCONNECT => {
            with_tray(|tray| {
                if let Some(link) = tray.link.as_mut() {
                    let _ = link.disconnect_viewer();
                }
                tray.link = None;
                tray.retry_at = Instant::now() + RECONNECT;
            });
        }
        CMD_OFFLINE => {
            with_tray(|tray| {
                if let Some(link) = tray.link.as_mut() {
                    let _ = link.go_offline();
                }
                tray.link = None;
                tray.retry_at = Instant::now() + RECONNECT;
                tray.status = Status::Offline;
                notify(tray, NIM_MODIFY, None);
            });
        }
        CMD_ONLINE => go_online(),
        _ => {}
    }
}

/// Starts `<this program> go-online` with the Windows administrator prompt.
fn go_online() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt as _;
        exe.as_os_str().encode_wide().chain([0]).collect()
    };
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe.as_ptr()),
            w!("go-online"),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    // Values above 32 mean it started; otherwise refused or cancelled.
    if (result.0 as isize) <= 32 {
        tracing::info!("tray: go online was cancelled or refused");
    }
}

/// A round icon in `rgb` with a small white screen in the middle.
fn make_icon(size: i32, (r, g, b): (u8, u8, u8)) -> HICON {
    let n = usize::try_from(size).unwrap_or(16);
    let mut px = vec![0u8; n * n * 4];
    let s = size as f32;
    let centre = s / 2.0;
    let radius = centre - 0.5;
    for y in 0..n {
        for x in 0..n {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let d = ((fx - centre).powi(2) + (fy - centre).powi(2)).sqrt();
            let alpha = (radius - d + 0.5).clamp(0.0, 1.0);
            let (u, v) = (fx / s, fy / s);
            let screen = (0.27..0.73).contains(&u) && (0.30..0.58).contains(&v);
            let stand = (0.45..0.55).contains(&u) && (0.58..0.68).contains(&v);
            let foot = (0.35..0.65).contains(&u) && (0.68..0.74).contains(&v);
            let (cr, cg, cb) = if screen || stand || foot {
                (255, 255, 255)
            } else {
                (r, g, b)
            };
            let i = (y * n + x) * 4;
            px[i] = cb;
            px[i + 1] = cg;
            px[i + 2] = cr;
            px[i + 3] = (alpha * 255.0).round() as u8;
        }
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(core::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: size,
            // Negative: rows top to bottom.
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // Monochrome mask rows are padded to 16 bits; all zero (the alpha decides).
    let mask_bits = vec![0u8; n.div_ceil(16) * 2 * n];
    // SAFETY: the DIB is created with room for n*n 32-bit pixels and we copy
    // exactly that many bytes into it; the bitmaps are freed after the icon
    // (which keeps its own copies) is made.
    unsafe {
        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return HICON::default();
        };
        if !bits.is_null() {
            core::ptr::copy_nonoverlapping(px.as_ptr(), bits.cast::<u8>(), px.len());
        }
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
