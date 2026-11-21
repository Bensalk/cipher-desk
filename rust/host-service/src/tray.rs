//! The Cipher Desk icon next to the clock on this computer (the one being
//! shared). It runs as the person signed in, started by the service.
//!
//! - Green: online, waiting for a connection. Amber: someone is connected.
//!   Grey: offline (sharing is off, or starting).
//! - Right-click (or click) menu: "Disconnect viewer", "Go offline",
//!   "Go online…" (asks for an administrator's OK, since it opens this
//!   computer to connections again), "Exit" (sharing off and the icon
//!   gone until "Go online" from the Start menu, or a restart), and wrong
//!   passwords tried since sharing started.
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
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO,
    NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_MESSAGE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, HICON,
    ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, TPM_BOTTOMALIGN,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use crate::link::{Client, Report, Status};

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
const CMD_HISTORY: usize = 13;
const CMD_EXIT: usize = 14;
const CMD_UNBLOCK: usize = 15;

/// "Exit" was chosen: end with [`crate::link::EXIT_QUIT`].
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Announce a new connection once its device name is known, or after this
/// long without one.
const NAME_WAIT: Duration = Duration::from_secs(4);

/// Green (easy to spot), amber and grey.
const GREEN: (u8, u8, u8) = (0x2E, 0x7D, 0x32);
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
    /// Who is connected (device name and address), from the worker.
    who: Option<String>,
    /// When the current connection started, and whether the pop-up for it
    /// has been shown.
    connected_at: Option<Instant>,
    announced: bool,
    /// Wrong sign-ins already shown (`None` until the first report from the
    /// worker, so old ones are not shown again after the tray restarts).
    failures_seen: Option<u64>,
    /// Wrong sign-ins since sharing started, and who made the last one.
    failures: u64,
    last_failure: Option<String>,
    /// Wrong sign-ins already mentioned in a "Connected" pop-up.
    failures_told: u64,
    /// Who is blocked right now after too many wrong passwords, and the last
    /// block a pop-up was shown for.
    blocked: Option<String>,
    blocked_told: Option<String>,
}

/// A message popped up above the icon.
struct Popup {
    text: String,
    warning: bool,
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
            make_icon(size, GREEN),
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
                who: None,
                connected_at: None,
                announced: false,
                failures_seen: None,
                failures: 0,
                last_failure: None,
                failures_told: 0,
                blocked: None,
                blocked_told: None,
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
    if EXITING.load(Ordering::SeqCst) {
        // Tells the service "Exit was chosen": no icon until Go online.
        std::process::exit(i32::try_from(crate::link::EXIT_QUIT).unwrap_or(77));
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

/// Asks the worker for the status; answers its request for copied files;
/// pops up who connected, who left, and wrong-password attempts.
fn poll() {
    with_tray(|tray| {
        let report = ask_worker(tray);
        let status = report.as_ref().map_or(Status::Offline, |r| r.status);
        let mut popup = None;

        // Wrong passwords: always worth a warning.
        if let Some(r) = &report {
            match tray.failures_seen {
                Some(seen) if r.failures > seen => {
                    let who = r.last_failure.as_deref().unwrap_or("an unknown device");
                    let more = r.failures - seen;
                    popup = Some(Popup {
                        text: if more > 1 {
                            format!("{more} wrong password attempts. Last from: {who}")
                        } else {
                            format!("Wrong password attempt from: {who}")
                        },
                        warning: true,
                    });
                }
                _ => {}
            }
            if r.failures != tray.failures {
                tracing::info!(
                    failures = r.failures,
                    last = r.last_failure.as_deref().unwrap_or("-"),
                    "tray: wrong passwords reported by the worker"
                );
            }
            tray.failures_seen = Some(r.failures);
            tray.failures = r.failures;
            tray.last_failure.clone_from(&r.last_failure);
            // A device was just blocked: say so, and how to let it in again.
            if r.blocked != tray.blocked_told {
                if let Some(who) = &r.blocked {
                    popup = Some(Popup {
                        text: format!(
                            "Blocked for up to 15 minutes after too many wrong passwords: {who}\nTo let it try again now, right-click this icon and choose Unblock."
                        ),
                        warning: true,
                    });
                }
                tray.blocked_told.clone_from(&r.blocked);
            }
            tray.blocked.clone_from(&r.blocked);
        }

        let previous = tray.status;
        let was_who = tray.who.take();
        tray.who = report.and_then(|r| r.who);
        if status == Status::Connected && previous != Status::Connected {
            tray.connected_at = Some(Instant::now());
            tray.announced = false;
        }
        // Announce once the device name lookup is done (the worker then adds
        // "(...)"), or after a short wait.
        if status == Status::Connected && !tray.announced {
            let named = tray.who.as_deref().is_some_and(|w| w.contains('('));
            let waited = tray.connected_at.is_some_and(|t| t.elapsed() >= NAME_WAIT);
            if named || waited {
                tray.announced = true;
                let who = tray.who.as_deref().unwrap_or("an unknown device");
                // Wrong passwords tried while nobody was watching (for
                // example while you were away and could not see this PC).
                let missed = tray.failures.saturating_sub(tray.failures_told);
                tray.failures_told = tray.failures;
                let text = if missed > 0 {
                    let last = tray.last_failure.as_deref().unwrap_or("an unknown device");
                    format!(
                        "Connected to this computer: {who}\nNote: {missed} wrong password attempt(s) before this. Last from: {last}"
                    )
                } else {
                    format!("Connected to this computer: {who}")
                };
                popup = popup.or(Some(Popup {
                    text,
                    warning: missed > 0,
                }));
            }
        }
        if previous == Status::Connected && status != Status::Connected {
            let who = was_who.as_deref().unwrap_or("the viewer");
            popup = popup.or(Some(Popup {
                text: format!("Disconnected: {who}"),
                warning: false,
            }));
            tray.connected_at = None;
        }

        tray.status = status;
        if !tray.shown {
            tray.shown = notify(tray, NIM_ADD, None);
        } else if status != previous || popup.is_some() || tray.who != was_who {
            notify(tray, NIM_MODIFY, popup);
        }
    });
}

/// One report from the worker (`None` when it is not running: offline).
fn ask_worker(tray: &mut Tray) -> Option<Report> {
    if tray.link.is_none() {
        if Instant::now() < tray.retry_at {
            return None;
        }
        match Client::connect() {
            Ok(c) => {
                tray.link = Some(c);
                // A new worker counts wrong passwords from zero.
                tray.failures_seen = None;
                tray.failures_told = 0;
            }
            Err(_) => {
                tray.retry_at = Instant::now() + RECONNECT;
                return None;
            }
        }
    }
    let link = tray.link.as_mut()?;
    let result = link.poll().and_then(|report| {
        if report.want > 0 {
            // A viewer is connected and something was just copied here.
            let files = rdpv_host::clipboard::copied_files().unwrap_or_default();
            tracing::info!(count = files.len(), "tray: sent the copied files");
            link.send_files(report.want, &files)?;
        }
        Ok(report)
    });
    match result {
        Ok(report) => Some(report),
        Err(_) => {
            tray.link = None;
            tray.retry_at = Instant::now() + RECONNECT;
            None
        }
    }
}

/// The hover text: the status, and who is connected.
fn tip_text(tray: &Tray) -> String {
    match (&tray.who, tray.status) {
        (Some(who), Status::Connected) => format!("Cipher Desk: connected - {who}"),
        _ => status_text(tray.status).to_owned(),
    }
}

/// Adds, updates or removes the icon. Returns whether Windows accepted it.
/// `popup`: (title, text) to also pop up above the icon
/// when someone connects — Windows shows this even while the Action Center
/// is closed and the tray icon is just sitting in the overflow tray.
fn notify(tray: &Tray, message: NOTIFY_ICON_MESSAGE, popup: Option<Popup>) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: u32::try_from(core::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: tray.hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: tray.icons[icon_index(tray.status)],
        ..Default::default()
    };
    let tip: Vec<u16> = tip_text(tray)
        .encode_utf16()
        .take(nid.szTip.len() - 1)
        .collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    let shown_popup = popup.as_ref().map(|p| (p.warning, p.text.clone()));
    if let Some(Popup { text, warning }) = popup {
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        let title = if warning {
            "Cipher Desk: warning"
        } else {
            "Cipher Desk"
        };
        let t: Vec<u16> = title
            .encode_utf16()
            .take(nid.szInfoTitle.len() - 1)
            .collect();
        nid.szInfoTitle[..t.len()].copy_from_slice(&t);
        let b: Vec<u16> = text.encode_utf16().take(nid.szInfo.len() - 1).collect();
        nid.szInfo[..b.len()].copy_from_slice(&b);
    }
    // SAFETY: `nid` is a fully initialised NOTIFYICONDATAW that outlives the call.
    let ok = unsafe { Shell_NotifyIconW(message, &nid) }.as_bool();
    if let Some((warning, text)) = shown_popup {
        // Which pop-ups Windows was asked to show (to check one that was missed).
        tracing::info!(warning, ok, text = %text.replace('\n', " / "), "tray: pop-up");
    }
    ok
}

fn show_menu(hwnd: HWND) {
    let (status, who, failures, blocked) = with_tray(|t| {
        let failures = (t.failures > 0).then(|| {
            let last = t.last_failure.clone().unwrap_or_else(|| "unknown".into());
            format!("Wrong passwords: {} (last from {last})", t.failures)
        });
        (t.status, t.who.clone(), failures, t.blocked.is_some())
    })
    .unwrap_or((Status::Offline, None, None, false));
    let failures_line: Option<Vec<u16>> = failures.map(|f| f.encode_utf16().chain([0]).collect());
    let title: Vec<u16> = status_text(status).encode_utf16().chain([0]).collect();
    let who_line: Option<Vec<u16>> = who.filter(|_| status == Status::Connected).map(|w| {
        format!("Connected: {w}")
            .encode_utf16()
            .chain([0])
            .collect()
    });
    // SAFETY: the menu is created, shown and destroyed here; the title buffer
    // outlives the AppendMenuW call that copies it.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(title.as_ptr()));
        if let Some(line) = &who_line {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(line.as_ptr()));
        }
        if let Some(line) = &failures_line {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(line.as_ptr()));
        }
        if blocked && status != Status::Offline {
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                CMD_UNBLOCK,
                w!("Unblock (let blocked devices try again now)"),
            );
        }
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
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Exit (sharing off)"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_HISTORY, w!("Connection history"));
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
        CMD_UNBLOCK => {
            with_tray(|tray| {
                let done = tray
                    .link
                    .as_mut()
                    .is_some_and(|link| link.unblock_devices().is_ok());
                tracing::info!(done, "tray: unblock");
                if done {
                    tray.blocked = None;
                    tray.blocked_told = None;
                    notify(
                        tray,
                        NIM_MODIFY,
                        Some(Popup {
                            text: "Unblocked: blocked devices can try their password again now."
                                .into(),
                            warning: false,
                        }),
                    );
                }
            });
        }
        CMD_ONLINE => go_online(),
        CMD_EXIT => {
            with_tray(|tray| {
                if let Some(link) = tray.link.as_mut() {
                    let _ = link.go_offline();
                }
                tray.link = None;
                notify(tray, NIM_DELETE, None);
                tray.shown = false;
            });
            tracing::info!("tray: exit (sharing off)");
            EXITING.store(true, Ordering::SeqCst);
            // SAFETY: ends this thread's message loop.
            unsafe { windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0) };
        }
        CMD_HISTORY => open_history(),
        _ => {}
    }
}

/// Opens the connection history in Notepad (the Windows one, by full path).
fn open_history() {
    let Some(path) = crate::history::path() else {
        return;
    };
    if !path.exists() {
        // The service makes it when sharing starts.
        return;
    }
    let Some(windir) = std::env::var_os("SystemRoot") else {
        return;
    };
    let notepad = std::path::Path::new(&windir)
        .join("System32")
        .join("notepad.exe");
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt as _;
        s.encode_wide().chain([0]).collect()
    };
    let exe = wide(notepad.as_os_str());
    let arg = wide(format!("\"{}\"", path.display()).as_ref());
    // SAFETY: all strings are NUL-terminated and outlive the call.
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(exe.as_ptr()),
            PCWSTR(arg.as_ptr()),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
    }
}

/// Starts `<this program> go-online` with the Windows administrator prompt.
fn go_online() {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    // Works without a prompt for the person signed in here; asks for an
    // administrator only if needed (see `manage::go_online`).
    if let Err(e) = std::process::Command::new(exe)
        .arg("go-online")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        tracing::info!(error = %e, "tray: could not start go online");
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
