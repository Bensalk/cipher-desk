//! A short, readable record of who connected to this computer and when:
//! `connections.log` beside the service logs. One line per event, local
//! time, the connecting computer's IP address, and which way it came in.
//! Opened from the tray icon ("Connection history"). Holds no passwords or
//! user names: only times, addresses and what happened.
#![allow(unsafe_code)]

use std::io::Write as _;
use std::path::PathBuf;

/// Above this size the file is renamed to `connections.old.log` (replacing
/// an older one) and a new file is started, so it never grows without end.
const MAX_BYTES: u64 = 512 * 1024;

pub const FILE_NAME: &str = "connections.log";

pub fn path() -> Option<PathBuf> {
    crate::config::data_dir().ok().map(|d| d.join(FILE_NAME))
}

/// Appends one line: `<local time>  <what>  <who>  (<door>)`. `time` from
/// [`local_time`], taken when it happened.
pub fn record(time: &str, what: &str, who: &str, door: &str) {
    let Some(path) = path() else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, path.with_file_name("connections.old.log"));
    }
    let line = format!("{time}  {what:<22} {who}  ({door})\r\n");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path);
    match file {
        Ok(mut f) => {
            let _ = f.write_all(line.as_bytes());
        }
        Err(e) => tracing::warn!(error = %e, "could not write the connection history"),
    }
}

/// This computer's local date and time, `YYYY-MM-DD HH:MM:SS`.
pub fn local_time() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only fills and returns a SYSTEMTIME value.
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// The connecting device's name as the network knows it (reverse DNS:
/// the Tailscale device name, or the name your router gave it), or `None`.
/// Inside your own Tailscale network the names are the ones your account
/// gives your devices. From the internet the name is set by whoever owns
/// that address, so it can be anything: treat it as a hint, not proof.
/// Gives up after 3 seconds.
pub fn device_name(ip: &str) -> Option<String> {
    let ip: std::net::IpAddr = ip.parse().ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("cipher-desk-name-lookup".into())
        .spawn(move || {
            let _ = tx.send(reverse_lookup(ip));
        })
        .ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(3))
        .ok()
        .flatten()
}

fn reverse_lookup(ip: std::net::IpAddr) -> Option<String> {
    use windows::Win32::Networking::WinSock::{
        socklen_t, GetNameInfoW, WSAStartup, AF_INET, AF_INET6, IN6_ADDR, IN6_ADDR_0, IN_ADDR,
        IN_ADDR_0, NI_MAXHOST, NI_NAMEREQD, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, WSADATA,
    };

    let mut node = vec![0u16; NI_MAXHOST as usize];
    // SAFETY: WSAStartup may be called more than once; the socket address
    // structs are plain values that outlive the GetNameInfoW call, and the
    // length passed matches the struct given.
    let rc = unsafe {
        let mut data = WSADATA::default();
        let _ = WSAStartup(0x0202, &mut data);
        match ip {
            std::net::IpAddr::V4(v4) => {
                let sa = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: 0,
                    sin_addr: IN_ADDR {
                        S_un: IN_ADDR_0 {
                            S_addr: u32::from_ne_bytes(v4.octets()),
                        },
                    },
                    sin_zero: [0; 8],
                };
                GetNameInfoW(
                    (&raw const sa).cast::<SOCKADDR>(),
                    socklen_t(i32::try_from(core::mem::size_of::<SOCKADDR_IN>()).unwrap_or(16)),
                    Some(&mut node),
                    None,
                    NI_NAMEREQD as i32,
                )
            }
            std::net::IpAddr::V6(v6) => {
                let sa = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_addr: IN6_ADDR {
                        u: IN6_ADDR_0 { Byte: v6.octets() },
                    },
                    ..Default::default()
                };
                GetNameInfoW(
                    (&raw const sa).cast::<SOCKADDR>(),
                    socklen_t(i32::try_from(core::mem::size_of::<SOCKADDR_IN6>()).unwrap_or(28)),
                    Some(&mut node),
                    None,
                    NI_NAMEREQD as i32,
                )
            }
        }
    };
    if rc != 0 {
        return None;
    }
    let len = node.iter().position(|&c| c == 0).unwrap_or(node.len());
    let name = String::from_utf16_lossy(&node[..len]);
    // Keep only safe, printable characters (names go into pop-ups and the log).
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(100)
        .collect();
    (!name.is_empty()).then_some(name)
}
