//! Live reliability tests (Stage 8) against the local test hosts started by
//! `scripts/start-test-hosts.sh`. Ignored by default; run with
//! `cargo test -p rust_lib_cipher_desk --lib live_ -- --ignored --test-threads=1`.
//!
//! A small TCP relay sits between the session and the host so the connection
//! can be cut (reset) or redirected. L3 also black-holes it with iptables to
//! simulate a silently vanished network (needs root and RDPV_TEST_IPTABLES=1).
//! Credentials come only from environment variables (SA-9). A test whose
//! settings are missing fails (it is never reported as passed without running);
//! in CI, L3 is skipped with `--skip live_l3`.

// Timing and memory figures are printed for the test log; no secrets are printed.
#![allow(clippy::print_stderr)]

use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs as _};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::{run, ClipboardMode, Command, SessionUpdate, Target, UpdateKind};
use rdpv_core::secret::SecretString;
use rdpv_core::status::ConnectionError;

type TestResult = Result<(), String>;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

struct Host {
    addr: SocketAddr,
    user: String,
    password: String,
}

fn host(prefix: &str) -> Result<Host, String> {
    let get = |k: &str| {
        env(&format!("{prefix}_{k}")).ok_or(format!("set {prefix}_{k} to run live tests"))
    };
    let h = get("HOST")?;
    let p: u16 = get("PORT")?
        .parse()
        .map_err(|_| format!("{prefix}_PORT is not a port"))?;
    let addr = (h.as_str(), p)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or(format!("{prefix}_HOST does not resolve"))?;
    Ok(Host {
        addr,
        user: get("USER")?,
        password: get("PASSWORD")?,
    })
}

fn io_err(what: &'static str) -> impl Fn(io::Error) -> String {
    move |e| format!("{what}: {e}")
}

// ------------------------------------------------------------------ relay

/// Forwards 127.0.0.1:<port> to a target that can be changed; `cut` closes
/// every forwarded connection at once (the client sees a reset/EOF).
struct Relay {
    port: u16,
    target: Arc<Mutex<SocketAddr>>,
    conns: Arc<Mutex<Vec<TcpStream>>>,
}

impl Relay {
    fn start(target: SocketAddr) -> io::Result<Relay> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let target = Arc::new(Mutex::new(target));
        let conns = Arc::new(Mutex::new(Vec::new()));
        let (t, c) = (Arc::clone(&target), Arc::clone(&conns));
        thread::spawn(move || {
            for inbound in listener.incoming().flatten() {
                let Ok(to) = t.lock().map(|g| *g) else { return };
                let Ok(outbound) = TcpStream::connect(to) else {
                    let _ = inbound.shutdown(Shutdown::Both);
                    continue;
                };
                if let (Ok(a), Ok(b), Ok(mut list)) =
                    (inbound.try_clone(), outbound.try_clone(), c.lock())
                {
                    list.push(a);
                    list.push(b);
                }
                pipe(&inbound, &outbound);
                pipe(&outbound, &inbound);
            }
        });
        Ok(Relay {
            port,
            target,
            conns,
        })
    }

    fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }

    fn redirect(&self, to: SocketAddr) {
        if let Ok(mut t) = self.target.lock() {
            *t = to;
        }
    }

    fn cut(&self) {
        if let Ok(mut list) = self.conns.lock() {
            for s in list.drain(..) {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }
}

fn pipe(from: &TcpStream, to: &TcpStream) {
    if let (Ok(mut r), Ok(mut w)) = (from.try_clone(), to.try_clone()) {
        thread::spawn(move || {
            let _ = io::copy(&mut r, &mut w);
            let _ = w.shutdown(Shutdown::Write);
        });
    }
}

// ------------------------------------------------------------------ session harness

struct Live {
    commands: Sender<Command>,
    updates: Receiver<SessionUpdate>,
    _pins: tempfile::TempDir,
}

/// Starts `run` on a thread against `addr` with the given credentials.
/// Certificates are approved automatically by [`Live::wait_for`].
fn start(addr: SocketAddr, h: &Host) -> io::Result<Live> {
    start_logged(addr, h, None)
}

/// Like [`start`]; with `capture`, everything the session thread logs (at the
/// most detailed level the app allows) is collected for inspection.
fn start_logged(addr: SocketAddr, h: &Host, capture: Option<Capture>) -> io::Result<Live> {
    let pins = tempfile::tempdir()?;
    let target = Target {
        host: addr.ip().to_string(),
        port: addr.port(),
        username: h.user.clone(),
        width: 1024,
        height: 768,
        pins_path: pins.path().join("pins.json").to_string_lossy().into_owned(),
        clipboard: ClipboardMode::Off,
        idle_minutes: 0,
        vnc: false,
    };
    let password = SecretString::new(h.password.clone());
    let (ctx, crx) = mpsc::channel();
    let (utx, urx) = mpsc::channel::<SessionUpdate>();
    thread::spawn(move || {
        let send = |u: SessionUpdate| utx.send(u).is_ok();
        match capture {
            Some(cap) => {
                let subscriber = tracing_subscriber::fmt()
                    .with_env_filter(rdpv_core::logging::env_filter("trace"))
                    .with_writer(cap)
                    .with_ansi(false)
                    .finish();
                tracing::subscriber::with_default(subscriber, || {
                    run(&target, password, &send, &crx);
                });
            }
            None => run(&target, password, &send, &crx),
        }
    });
    Ok(Live {
        commands: ctx,
        updates: urx,
        _pins: pins,
    })
}

impl Live {
    /// Waits for the first update matching `pred` (frames are skipped, and the
    /// first-connection certificate prompt is approved).
    fn wait_for(
        &self,
        what: &str,
        within: Duration,
        pred: impl Fn(&SessionUpdate) -> bool,
    ) -> Result<SessionUpdate, String> {
        let until = Instant::now() + within;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let u = self
                .updates
                .recv_timeout(left)
                .map_err(|_| format!("timed out after {within:?} waiting for: {what}"))?;
            if u.kind == UpdateKind::CertificateApproval {
                let _ = self.commands.send(Command::ApproveCertificate(true));
            }
            if pred(&u) {
                return Ok(u);
            }
            if u.kind == UpdateKind::Ended && what != "end" {
                return Err(format!(
                    "session ended early ({:?}) while waiting for: {what}",
                    u.error.as_ref().map(|e| e.id.clone())
                ));
            }
        }
    }

    fn connected(&self, within: Duration) -> TestResult {
        self.wait_for("connected", within, |u| {
            u.kind == UpdateKind::Status && u.connected
        })
        .map(drop)
    }

    fn ended(&self, within: Duration) -> Result<Option<String>, String> {
        let u = self.wait_for("end", within, |u| u.kind == UpdateKind::Ended)?;
        Ok(u.error.map(|e| e.id))
    }

    fn disconnect(&self, reason: Option<ConnectionError>) {
        let _ = self.commands.send(Command::Disconnect(reason));
    }
}

fn is_reconnecting(u: &SessionUpdate) -> bool {
    u.kind == UpdateKind::Reconnecting
}

// ------------------------------------------------------------------ tests

#[test]
#[ignore = "needs the live test hosts"]
fn live_l1_reconnects_after_the_connection_is_cut() -> TestResult {
    let h = host("RDPV_TEST")?;
    let relay = Relay::start(h.addr).map_err(io_err("relay"))?;
    let s = start(relay.addr(), &h).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;

    relay.cut();
    let r = s.wait_for("reconnecting", Duration::from_secs(15), is_reconnecting)?;
    assert_eq!((r.x, r.y), (1, 3), "first of three attempts");
    // Same computer, same certificate: reconnects without asking anything.
    s.wait_for("resized after reconnect", Duration::from_secs(30), |u| {
        u.kind == UpdateKind::Resized
    })?;
    s.connected(Duration::from_secs(10))?;

    // A second drop is handled the same way (attempts start again at 1).
    relay.cut();
    let r = s.wait_for(
        "reconnecting again",
        Duration::from_secs(15),
        is_reconnecting,
    )?;
    assert_eq!(r.x, 1);
    s.connected(Duration::from_secs(30))?;

    s.disconnect(None);
    assert_eq!(
        s.ended(Duration::from_secs(10))?,
        None,
        "user disconnect has no error"
    );
    Ok(())
}

#[test]
#[ignore = "needs the live test hosts"]
fn live_l2_different_computer_after_a_drop_is_blocked() -> TestResult {
    let h = host("RDPV_TEST")?;
    let other = host("RDPV_CLIP")?;
    let relay = Relay::start(h.addr).map_err(io_err("relay"))?;
    let s = start(relay.addr(), &h).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;

    // Someone answers on the same address afterwards, with another certificate.
    relay.redirect(other.addr);
    relay.cut();
    let changed = s.wait_for("certificate changed", Duration::from_secs(30), |u| {
        u.kind == UpdateKind::CertificateChanged
    })?;
    assert_ne!(changed.pinned, changed.fingerprint);
    assert_eq!(
        s.ended(Duration::from_secs(10))?.as_deref(),
        Some("CertificateChanged")
    );
    Ok(())
}

#[test]
#[ignore = "needs the live test hosts, root and RDPV_TEST_IPTABLES=1"]
fn live_l3_silently_dropped_network_is_detected() -> TestResult {
    let h = host("RDPV_TEST")?;
    if env("RDPV_TEST_IPTABLES").as_deref() != Some("1") {
        return Err("set RDPV_TEST_IPTABLES=1 (needs root) to run this test".into());
    }
    let relay = Relay::start(h.addr).map_err(io_err("relay"))?;
    let cap = Capture::default();
    let s = start_logged(relay.addr(), &h, Some(cap.clone())).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;

    // Black-hole the relay port: packets vanish, no reset is ever sent.
    let wall = Firewall::drop_port(relay.port)?;
    let started = Instant::now();
    s.wait_for(
        "reconnecting after silent drop",
        Duration::from_secs(60),
        is_reconnecting,
    )?;
    let detected = started.elapsed();
    eprintln!("silent drop detected after {:.1} s", detected.as_secs_f64());
    // Which network error noticed it (no secrets: see L7).
    for line in cap.text().lines().filter(|l| l.contains(" failed")) {
        eprintln!("  {}", line.split_once("rdpv").map_or(line, |(_, r)| r));
    }
    assert!(detected <= Duration::from_secs(50), "took {detected:?}");

    drop(wall); // the network comes back
    s.connected(Duration::from_secs(45))?;
    s.disconnect(None);
    assert_eq!(s.ended(Duration::from_secs(10))?, None);
    Ok(())
}

#[test]
#[ignore = "needs the live test hosts"]
fn live_l4_background_disconnect_reports_its_reason() -> TestResult {
    let h = host("RDPV_TEST")?;
    let s = start(h.addr, &h).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;
    s.disconnect(Some(ConnectionError::AppInBackground));
    assert_eq!(
        s.ended(Duration::from_secs(10))?.as_deref(),
        Some("AppInBackground")
    );
    Ok(())
}

#[test]
#[ignore = "needs the live test hosts"]
fn live_l5_wrong_password_is_not_retried() -> TestResult {
    let mut h = host("RDPV_TEST")?;
    h.password.push('x');
    let s = start(h.addr, &h).map_err(io_err("start"))?;
    let u = s.wait_for("end", Duration::from_secs(30), |u| {
        assert!(!is_reconnecting(u), "must never retry a wrong password");
        u.kind == UpdateKind::Ended
    })?;
    assert_eq!(u.error.map(|e| e.id).as_deref(), Some("WrongCredentials"));
    Ok(())
}

/// No false alarms: a healthy but completely idle session (no input, static
/// screen) must not be declared lost by the keepalive and time-out settings.
/// Duration from RDPV_IDLE_SECS (default 90, well past the 30 s detection time).
#[test]
#[ignore = "needs the live test hosts"]
fn live_l8_idle_healthy_session_is_not_dropped() -> TestResult {
    let h = host("RDPV_TEST")?;
    let secs: u64 = env("RDPV_IDLE_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(90);
    let s = start(h.addr, &h).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if let Ok(u) = s.updates.recv_timeout(Duration::from_millis(200)) {
            if is_reconnecting(&u) || u.kind == UpdateKind::Ended {
                return Err(format!(
                    "idle session was treated as lost ({:?})",
                    u.error.map(|e| e.id)
                ));
            }
        }
    }
    s.disconnect(None);
    assert_eq!(s.ended(Duration::from_secs(10))?, None);
    Ok(())
}

/// Long session: memory must stay flat while the screen keeps changing.
/// Duration from RDPV_SOAK_SECS (default 60).
#[test]
#[ignore = "needs the live test hosts"]
fn live_l6_long_session_memory_stays_flat() -> TestResult {
    let h = host("RDPV_TEST")?;
    let secs: u64 = env("RDPV_SOAK_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let s = start(h.addr, &h).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;

    let mut frames = 0u64;
    let mut baseline = None;
    let started = Instant::now();
    let mut n = 0u16;
    while started.elapsed() < Duration::from_secs(secs) {
        // Move the remote pointer around so the screen keeps changing.
        n = n.wrapping_add(1);
        let (x, y) = (200 + (n % 400), 200 + ((n / 2) % 300));
        let _ = s.commands.send(Command::Input(vec![
            rdpv_core::session::InputEvent::MouseMove { x, y },
        ]));
        while let Ok(u) = s.updates.recv_timeout(Duration::from_millis(20)) {
            if u.kind == UpdateKind::Ended {
                return Err(format!(
                    "session ended during the soak test: {:?}",
                    u.error.map(|e| e.id)
                ));
            }
            if u.kind == UpdateKind::Frame {
                frames += 1;
            }
        }
        if baseline.is_none() && started.elapsed() >= Duration::from_secs(10) {
            baseline = rss_kib();
        }
    }
    let end = rss_kib();
    eprintln!("frames: {frames}; memory after 10 s: {baseline:?} KiB; at end: {end:?} KiB");
    let (Some(b), Some(e)) = (baseline, end) else {
        return Err("could not read this process's memory use".into());
    };
    assert!(
        e <= b + 20 * 1024,
        "memory grew by more than 20 MiB: {b} -> {e} KiB"
    );
    s.disconnect(None);
    assert_eq!(s.ended(Duration::from_secs(10))?, None);
    Ok(())
}

/// Collects log output in memory.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        let bytes = self.0.lock().map(|v| v.clone()).unwrap_or_default();
        String::from_utf8_lossy(&bytes).to_ascii_lowercase()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut v) = self.0.lock() {
            v.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The password in every form a log might contain it: plain, hex of UTF-8,
/// hex of UTF-16 (how CredSSP buffers look, SF-2).
fn password_forms(p: &str) -> [String; 3] {
    let hex = |bytes: Vec<u8>| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    [
        p.to_ascii_lowercase(),
        hex(p.as_bytes().to_vec()),
        hex(p.encode_utf16().flat_map(u16::to_le_bytes).collect()),
    ]
}

/// SA-7 for the Stage 8 paths: reconnecting after a drop, and a wrong
/// password, at the most detailed log level the app allows.
#[test]
#[ignore = "needs the live test hosts"]
fn live_l7_reconnect_and_failures_never_log_the_password() -> TestResult {
    let h = host("RDPV_TEST")?;
    let cap = Capture::default();

    // Drop and reconnect.
    let relay = Relay::start(h.addr).map_err(io_err("relay"))?;
    let s = start_logged(relay.addr(), &h, Some(cap.clone())).map_err(io_err("start"))?;
    s.connected(Duration::from_secs(30))?;
    relay.cut();
    s.wait_for("reconnecting", Duration::from_secs(15), is_reconnecting)?;
    s.connected(Duration::from_secs(30))?;
    s.disconnect(None);
    s.ended(Duration::from_secs(10))?;

    // Wrong password (logged by the same capture).
    let mut wrong = host("RDPV_TEST")?;
    wrong.password.push_str("Wrong9");
    let w = start_logged(h.addr, &wrong, Some(cap.clone())).map_err(io_err("start"))?;
    w.ended(Duration::from_secs(30))?;
    thread::sleep(Duration::from_millis(200)); // let the thread finish logging

    let logs = cap.text();
    assert!(
        logs.len() > 200,
        "no logs captured; the test would prove nothing"
    );
    assert!(
        logs.contains("reconnecting"),
        "the reconnect was not logged"
    );
    for p in [&h.password, &wrong.password] {
        for (form, needle) in ["plain", "UTF-8 hex", "UTF-16 hex"]
            .iter()
            .zip(password_forms(p))
        {
            assert!(!logs.contains(&needle), "password found in logs ({form})");
        }
    }
    Ok(())
}

/// Resident memory of this process (Linux), in KiB.
fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// iptables rules that drop all traffic to and from a local port; removed on drop.
struct Firewall {
    port: u16,
}

impl Firewall {
    fn drop_port(port: u16) -> Result<Firewall, String> {
        let wall = Firewall { port }; // removes whatever was added, even on error
        for rule in Self::rules(port) {
            let ok = std::process::Command::new("iptables")
                .arg("-I")
                .args(&rule)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                return Err("iptables failed (root needed)".into());
            }
        }
        Ok(wall)
    }

    fn rules(port: u16) -> [Vec<String>; 2] {
        let p = port.to_string();
        let r = |dir: &str| {
            ["INPUT", "-i", "lo", "-p", "tcp", dir, &p, "-j", "DROP"]
                .map(str::to_owned)
                .to_vec()
        };
        [r("--dport"), r("--sport")]
    }
}

impl Drop for Firewall {
    fn drop(&mut self) {
        for rule in Self::rules(self.port) {
            let _ = std::process::Command::new("iptables")
                .arg("-D")
                .args(&rule)
                .status();
        }
    }
}

/// The VNC method through the same session loop the app uses: connected,
/// screen size, pictures, then a clean end. Needs RDPV_VNC_PORT and
/// RDPV_VNC_PASSWORD (CI runs x11vnc).
#[test]
#[ignore = "needs a VNC server"]
fn live_vnc_session_streams_the_screen() -> TestResult {
    let (Some(port), Some(password)) = (env("RDPV_VNC_PORT"), env("RDPV_VNC_PASSWORD")) else {
        return Err("set RDPV_VNC_PORT and RDPV_VNC_PASSWORD".into());
    };
    let port: u16 = port.parse().map_err(|_| "bad RDPV_VNC_PORT".to_owned())?;
    let target = Target::vnc("127.0.0.1".into(), port, 0);
    let (ctx, crx) = mpsc::channel();
    let (utx, urx) = mpsc::channel::<SessionUpdate>();
    thread::spawn(move || {
        let send = |u: SessionUpdate| utx.send(u).is_ok();
        run(&target, SecretString::new(password), &send, &crx);
    });
    let live = Live {
        commands: ctx,
        updates: urx,
        _pins: tempfile::tempdir().map_err(|e| e.to_string())?,
    };
    let size = live.wait_for("size", Duration::from_secs(15), |u| {
        u.kind == UpdateKind::Resized
    })?;
    if size.width < 640 || size.height < 480 {
        return Err(format!("size {}x{}", size.width, size.height));
    }
    live.connected(Duration::from_secs(5))?;
    let frame = live.wait_for("picture", Duration::from_secs(10), |u| {
        u.kind == UpdateKind::Frame
    })?;
    if frame.rgba.is_empty() {
        return Err("empty picture".into());
    }
    live.disconnect(None);
    match live.ended(Duration::from_secs(5))? {
        None => Ok(()),
        Some(e) => Err(format!("ended with {e}")),
    }
}
