//! The private link between the worker (system account) and the tray icon
//! (running as the person signed in), over a named pipe on this computer.
//!
//! Why it exists: Windows hides what the signed-in person copies in Explorer
//! from the system account, so the worker cannot see copied files itself.
//! The tray program runs as that person and can. The worker asks it for the
//! copied files only while a viewer is connected; the rest of the time the
//! tray only asks for the status (to colour its icon).
//!
//! The tray always starts the conversation, one line per request, one line
//! per answer:
//!
//! - `POLL` → `online <n>` or `connected <n>`; `n` > 0 means "please send the
//!   files copied right now, as answer number `n`".
//! - `FILES <n>` then the paths, each after a tab → `ok`. (Tabs cannot appear
//!   in Windows file names.)
//! - `OFFLINE` → `ok`, then the worker ends and the service keeps sharing off
//!   until "Go online" or the next restart.
//! - `DISCONNECT` → `ok`, then the worker ends; the service starts a fresh one
//!   at once, which drops the viewer.
//!
//! Only this computer can reach the pipe (remote clients are refused), only
//! the system account and signed-in people may open it, and each side checks
//! that the other is in the same Windows session. A path the tray sends is
//! never opened with the system account's rights: the worker reads files as
//! the signed-in person (see `rdpv_host::clipboard::read_files_as_signed_in_user`).
#![allow(unsafe_code)]

use std::fs::File;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientSessionId, GetNamedPipeServerSessionId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Worker exit code: "go offline" (the service then keeps sharing off).
pub const EXIT_OFFLINE: u32 = 75;
/// Worker exit code: "drop the viewer" (the service starts a fresh worker).
pub const EXIT_RESTART: u32 = 76;

/// Longest line either side accepts (a guard against garbage).
const MAX_LINE: u64 = 4 * 1024 * 1024;
/// How long the worker waits for the tray to send the copied files.
const ANSWER_WAIT: Duration = Duration::from_secs(3);
/// The tray asks about twice a second; older than this means it is gone.
const TRAY_GONE_AFTER_MS: u64 = 3_000;

/// Owner + system full access; signed-in (interactive) people read/write.
/// Protected, so nothing is inherited.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GRGW;;;IU)";

fn pipe_name(session: u32) -> String {
    format!(r"\\.\pipe\CipherDeskHost-{session}")
}

/// The Windows session this process runs in.
pub fn own_session() -> Option<u32> {
    let mut session = 0u32;
    // SAFETY: `session` is a valid out-pointer.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.ok()?;
    Some(session)
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The worker's side: what it tells the tray, and the tray's answers.
#[derive(Default)]
pub struct Link {
    viewers: AtomicU32,
    tray_seen_ms: AtomicU64,
    ask: Mutex<Ask>,
    answered: Condvar,
}

#[derive(Default)]
struct Ask {
    last_id: u64,
    /// The request the tray should answer next (0 = none).
    want: u64,
    answer: Option<(u64, Vec<PathBuf>)>,
}

impl Link {
    /// `door`: which listener (0 or 1); each serves one viewer at a time.
    pub fn viewer_connected(&self, door: u32) {
        self.viewers.fetch_or(1 << door, Ordering::SeqCst);
    }

    pub fn viewer_disconnected(&self, door: u32) {
        // Also sent for refused or failed attempts; a listener serves one
        // connection at a time, so any disconnect means nobody is on it.
        self.viewers.fetch_and(!(1 << door), Ordering::SeqCst);
    }

    /// The files the signed-in person has copied right now, asked from the
    /// tray. `None` if the tray is not running or does not answer in time.
    pub fn copied_files(&self) -> Option<Vec<PathBuf>> {
        let seen = self.tray_seen_ms.load(Ordering::SeqCst);
        if epoch_ms().saturating_sub(seen) > TRAY_GONE_AFTER_MS {
            tracing::info!("tray helper not running; cannot read files copied here");
            return None;
        }
        let mut ask = self.ask.lock().ok()?;
        ask.last_id += 1;
        let id = ask.last_id;
        ask.want = id;
        ask.answer = None;
        let deadline = Instant::now() + ANSWER_WAIT;
        loop {
            if let Some((got, _)) = &ask.answer {
                if *got == id {
                    break;
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                ask.want = 0;
                tracing::warn!("tray helper did not answer in time");
                return None;
            }
            ask = self.answered.wait_timeout(ask, left).ok()?.0;
        }
        ask.want = 0;
        let paths = ask.answer.take().map(|(_, p)| p).unwrap_or_default();
        (!paths.is_empty()).then_some(paths)
    }

    /// Answers one line from the tray. `Some(code)`: end the worker with it
    /// after the answer is sent.
    fn handle(&self, line: &str) -> (String, Option<u32>) {
        self.tray_seen_ms.store(epoch_ms(), Ordering::SeqCst);
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "POLL" {
            let state = if self.viewers.load(Ordering::SeqCst) > 0 {
                "connected"
            } else {
                "online"
            };
            let want = self.ask.lock().map(|a| a.want).unwrap_or(0);
            return (format!("{state} {want}"), None);
        }
        if let Some(rest) = line.strip_prefix("FILES ") {
            let mut parts = rest.split('\t');
            let id = parts
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let paths: Vec<PathBuf> = parts
                .filter(|p| !p.is_empty())
                .take(1000)
                .map(PathBuf::from)
                .collect();
            if let Ok(mut ask) = self.ask.lock() {
                ask.answer = Some((id, paths));
            }
            self.answered.notify_all();
            return ("ok".into(), None);
        }
        match line {
            "OFFLINE" => {
                tracing::info!("tray: go offline");
                ("ok".into(), Some(EXIT_OFFLINE))
            }
            "DISCONNECT" => {
                tracing::info!("tray: disconnect the viewer");
                ("ok".into(), Some(EXIT_RESTART))
            }
            _ => ("?".into(), None),
        }
    }
}

/// Starts the pipe the tray connects to; runs for the worker's lifetime.
pub fn serve(link: Arc<Link>) {
    let spawned = std::thread::Builder::new()
        .name("cipher-desk-tray-link".into())
        .spawn(move || serve_loop(&link));
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "tray link not started");
    }
}

fn serve_loop(link: &Link) {
    let Some(session) = own_session() else {
        tracing::warn!("tray link: unknown session");
        return;
    };
    let name: Vec<u16> = pipe_name(session).encode_utf16().chain([0]).collect();
    let wide_sddl: Vec<u16> = PIPE_SDDL.encode_utf16().chain([0]).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the SDDL buffer is NUL-terminated and outlives the call; `sd`
    // is a valid out-pointer. The descriptor is kept for the process's life.
    let made = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide_sddl.as_ptr()),
            1,
            &mut sd,
            None,
        )
    };
    if let Err(e) = made {
        tracing::warn!(error = %e, "tray link: security descriptor");
        return;
    }

    loop {
        match accept(&name, sd, session) {
            Ok(file) => {
                if let Some(code) = talk(link, file) {
                    std::process::exit(i32::try_from(code).unwrap_or(1));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "tray link: waiting for the tray failed");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

/// Creates the pipe, waits for the tray, and checks it is in our session.
fn accept(name: &[u16], sd: PSECURITY_DESCRIPTOR, session: u32) -> std::io::Result<File> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

    let sa = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(core::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: `name` is NUL-terminated; `sa` and its descriptor outlive the
    // call. A valid handle is owned by the File right after.
    let file = unsafe {
        let h = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            Some(&sa),
        );
        if h.is_invalid() {
            return Err(std::io::Error::last_os_error());
        }
        File::from(OwnedHandle::from_raw_handle(h.0))
    };
    let h = HANDLE(file.as_raw_handle());
    // SAFETY: `h` is the live pipe handle owned by `file`.
    if let Err(e) = unsafe { ConnectNamedPipe(h, None) } {
        if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
            return Err(std::io::Error::other(e));
        }
    }
    let mut client_session = u32::MAX;
    // SAFETY: `h` is a connected pipe; out-pointer is a valid local.
    unsafe { GetNamedPipeClientSessionId(h, &mut client_session) }
        .map_err(std::io::Error::other)?;
    if client_session != session {
        return Err(std::io::Error::other(format!(
            "refused a client from another session ({client_session})"
        )));
    }
    Ok(file)
}

/// Serves one tray connection until it closes. `Some(code)`: end the worker.
fn talk(link: &Link, file: File) -> Option<u32> {
    let mut writer = file.try_clone().ok()?;
    let mut reader = BufReader::new(file);
    loop {
        let mut line = String::new();
        match (&mut reader).take(MAX_LINE).read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let (answer, end) = link.handle(&line);
        if writer.write_all(format!("{answer}\n").as_bytes()).is_err() {
            return None;
        }
        let _ = writer.flush();
        if end.is_some() {
            return end;
        }
    }
}

/// The tray's side of the link.
pub struct Client {
    reader: BufReader<File>,
    writer: File,
}

/// What the worker says about sharing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No worker answering: sharing is off (or starting).
    Offline,
    /// Waiting for a viewer.
    Online,
    /// A viewer is connected.
    Connected,
}

impl Client {
    /// Connects to this session's worker, if it is running.
    pub fn connect() -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle as _;

        let session = own_session().ok_or_else(|| std::io::Error::other("unknown session"))?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_name(session))?;
        let mut server_session = u32::MAX;
        // SAFETY: the handle is the open pipe owned by `file`; valid out-pointer.
        unsafe { GetNamedPipeServerSessionId(HANDLE(file.as_raw_handle()), &mut server_session) }
            .map_err(std::io::Error::other)?;
        if server_session != session {
            return Err(std::io::Error::other("the pipe is not from this session"));
        }
        Ok(Self {
            reader: BufReader::new(file.try_clone()?),
            writer: file,
        })
    }

    fn request(&mut self, line: &str) -> std::io::Result<String> {
        self.writer.write_all(format!("{line}\n").as_bytes())?;
        self.writer.flush()?;
        let mut answer = String::new();
        let n = (&mut self.reader).take(MAX_LINE).read_line(&mut answer)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(answer.trim_end().to_owned())
    }

    /// The status, and the number of a pending request for the copied files
    /// (0 = none).
    pub fn poll(&mut self) -> std::io::Result<(Status, u64)> {
        let answer = self.request("POLL")?;
        let mut parts = answer.split(' ');
        let status = match parts.next() {
            Some("connected") => Status::Connected,
            _ => Status::Online,
        };
        let want = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok((status, want))
    }

    /// Answers request `id` with the copied files (empty: none).
    pub fn send_files(&mut self, id: u64, paths: &[PathBuf]) -> std::io::Result<()> {
        let mut line = format!("FILES {id}");
        for p in paths {
            let s = p.to_string_lossy();
            if s.contains(['\t', '\n']) {
                continue;
            }
            line.push('\t');
            line.push_str(&s);
        }
        self.request(&line).map(|_| ())
    }

    pub fn go_offline(&mut self) -> std::io::Result<()> {
        self.request("OFFLINE").map(|_| ())
    }

    pub fn disconnect_viewer(&mut self) -> std::io::Result<()> {
        self.request("DISCONNECT").map(|_| ())
    }
}
