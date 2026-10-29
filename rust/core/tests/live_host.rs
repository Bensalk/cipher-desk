//! Integration tests against a real RDP host (Stage 4 acceptance, spike tests T1–T7).
//!
//! Ignored by default. Run with a test host and credentials in environment
//! variables — never put real credentials in files:
//!
//! ```sh
//! RDPV_TEST_HOST=127.0.0.1 RDPV_TEST_PORT=3389 \
//! RDPV_TEST_USER=<user> RDPV_TEST_PASSWORD=<password> \
//! RDPV_TEST_NONLA_PORT=3390 \
//! cargo test -p rdpv-core --test live_host -- --ignored --test-threads=1
//! ```
//!
//! CI starts a FreeRDP shadow server (NLA required) and an xrdp server (no NLA)
//! with a random password for each run (see .github/workflows/ci.yml).

use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rdpv_core::input::{parse_endpoint, parse_username};
use rdpv_core::secret::SecretString;
use rdpv_core::session::{
    connect, ConnectFailure, ConnectParams, InputEvent, PointerButton, Session, SessionEvent,
};
use rdpv_core::status::ConnectionError;

type TestResult = Result<(), String>;

struct Env {
    host: String,
    port: String,
    user: String,
    password: String,
}

fn env() -> Result<Env, String> {
    let get = |k: &str| std::env::var(k).map_err(|_| format!("set {k} to run live-host tests"));
    Ok(Env {
        host: get("RDPV_TEST_HOST")?,
        port: get("RDPV_TEST_PORT").unwrap_or_else(|_| "3389".into()),
        user: get("RDPV_TEST_USER")?,
        password: get("RDPV_TEST_PASSWORD")?,
    })
}

fn params(
    e: &Env,
    port: &str,
    password: &str,
    pin: Option<String>,
    w: u16,
    h: u16,
) -> Result<ConnectParams, String> {
    Ok(ConnectParams {
        endpoint: parse_endpoint(&e.host, port).map_err(|x| format!("{x:?}"))?,
        username: parse_username(&e.user).map_err(|x| format!("{x:?}"))?,
        password: SecretString::new(password.to_owned()),
        desktop_width: w,
        desktop_height: h,
        pinned_fingerprint: pin,
        connect_timeout: Duration::from_secs(5),
    })
}

/// First connection: returns the fingerprint the host presented (T1).
fn learn_fingerprint(e: &Env) -> Result<String, String> {
    match connect(params(e, &e.port, &e.password, None, 1280, 800)?) {
        Err(ConnectFailure::CertificateNeedsApproval { fingerprint }) => Ok(fingerprint),
        Err(other) => Err(format!("expected approval request, got {other:?}")),
        Ok(_) => Err("connected to an unapproved host".into()),
    }
}

fn open(e: &Env, w: u16, h: u16) -> Result<Session, String> {
    let fp = learn_fingerprint(e)?;
    connect(params(e, &e.port, &e.password, Some(fp), w, h)?)
        .map_err(|f| format!("connect failed: {f:?}"))
}

/// Polls until `pred` matches an event or the time runs out.
fn poll_until(
    s: &mut Session,
    secs: u64,
    mut pred: impl FnMut(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let ev = s.poll();
        let hit = ev.iter().any(&mut pred);
        all.extend(ev);
        if hit || s.is_ended() {
            break;
        }
    }
    all
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t1_unknown_certificate_is_not_trusted_automatically() -> TestResult {
    let e = env()?;
    let fp = learn_fingerprint(&e)?;
    assert_eq!(fp.len(), 95, "SHA-256 fingerprint as hex pairs: {fp}");
    Ok(())
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t2_t3_approved_certificate_connects_and_shows_the_screen() -> TestResult {
    let e = env()?;
    let mut s = open(&e, 1280, 800)?;
    let events = poll_until(&mut s, 10, |ev| matches!(ev, SessionEvent::Updated(_)));
    let update = events.iter().find_map(|ev| match ev {
        SessionEvent::Updated(r) => Some(*r),
        _ => None,
    });
    let r = update.ok_or_else(|| format!("no screen update; events: {events:?}"))?;
    assert!(!s.region_pixels(r).is_empty());
    let (w, h) = s.size();
    assert_eq!(s.frame().len(), usize::from(w) * usize::from(h) * 4);
    assert!(s.frame().iter().any(|b| *b != 0), "screen is blank");
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t4_changed_certificate_is_blocked() -> TestResult {
    let e = env()?;
    let wrong_pin = ["00"; 32].join(":");
    match connect(params(
        &e,
        &e.port,
        &e.password,
        Some(wrong_pin.clone()),
        1280,
        800,
    )?) {
        Err(ConnectFailure::CertificateChanged { pinned, presented }) => {
            assert_eq!(pinned, wrong_pin);
            assert_ne!(presented, wrong_pin);
            Ok(())
        }
        other => Err(format!(
            "expected CertificateChanged, got {:?}",
            other.map(|_| "session")
        )),
    }
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t5_wrong_password_is_reported_clearly() -> TestResult {
    let e = env()?;
    let fp = learn_fingerprint(&e)?;
    let wrong = format!("{}-wrong", e.password);
    match connect(params(&e, &e.port, &wrong, Some(fp), 1280, 800)?) {
        Err(ConnectFailure::Failed(ConnectionError::WrongCredentials)) => Ok(()),
        other => Err(format!(
            "expected WrongCredentials, got {:?}",
            other.map(|_| "session")
        )),
    }
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t6_closed_port_and_unreachable_host() -> TestResult {
    let e = env()?;
    // A local port with nothing listening.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").map_err(|x| x.to_string())?;
    let port = closed
        .local_addr()
        .map_err(|x| x.to_string())?
        .port()
        .to_string();
    drop(closed);
    let local = Env {
        host: "127.0.0.1".into(),
        ..env()?
    };
    match connect(params(&local, &port, &e.password, None, 1280, 800)?) {
        Err(ConnectFailure::Failed(ConnectionError::HostRefused)) => {}
        other => {
            return Err(format!(
                "expected HostRefused, got {:?}",
                other.map(|_| "session")
            ))
        }
    }
    // TEST-NET-1 (RFC 5737): never routed, so the connect must time out.
    let unroutable = Env {
        host: "192.0.2.1".into(),
        ..env()?
    };
    let mut p = params(&unroutable, "3389", &e.password, None, 1280, 800)?;
    p.connect_timeout = Duration::from_secs(2);
    let started = Instant::now();
    match connect(p) {
        Err(ConnectFailure::Failed(ConnectionError::Timeout | ConnectionError::HostRefused)) => {}
        other => {
            return Err(format!(
                "expected Timeout, got {:?}",
                other.map(|_| "session")
            ))
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "timeout not respected"
    );
    Ok(())
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t8_host_without_nla_is_refused() -> TestResult {
    let e = env()?;
    let Ok(port) = std::env::var("RDPV_TEST_NONLA_PORT") else {
        return Err("set RDPV_TEST_NONLA_PORT (an RDP host without NLA, e.g. xrdp)".into());
    };
    let p = params(&e, &port, &e.password, None, 1280, 800)?;
    match connect(p) {
        Err(ConnectFailure::Failed(ConnectionError::SecurityNotSupported)) => Ok(()),
        other => Err(format!(
            "expected SecurityNotSupported, got {:?}",
            other.map(|_| "session")
        )),
    }
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t9_input_and_graceful_disconnect() -> TestResult {
    let e = env()?;
    let mut s = open(&e, 1280, 800)?;
    poll_until(&mut s, 5, |ev| matches!(ev, SessionEvent::Updated(_)));
    let input = vec![
        InputEvent::MouseMove { x: 400, y: 300 },
        InputEvent::MouseDown(PointerButton::Left),
        InputEvent::MouseUp(PointerButton::Left),
        InputEvent::Wheel {
            vertical: true,
            delta: -120,
        },
        InputEvent::KeyDown {
            scancode: 0x1E,
            extended: false,
        }, // A
        InputEvent::KeyUp {
            scancode: 0x1E,
            extended: false,
        },
        InputEvent::Unicode {
            ch: 'é',
            down: true,
        },
        InputEvent::Unicode {
            ch: 'é',
            down: false,
        },
    ];
    s.send_input(input)
        .map_err(|x| format!("input failed: {x:?}"))?;
    let events = poll_until(&mut s, 2, |_| false);
    assert!(
        !events.iter().any(|ev| matches!(ev, SessionEvent::Ended(_))),
        "session ended after input: {events:?}"
    );
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t10_mismatched_desktop_size_is_handled() -> TestResult {
    // In the spike, asking for a size the host does not use made the server send a new
    // Demand Active PDU, which the simple client could not handle (SF-4).
    let e = env()?;
    let mut s = open(&e, 1024, 768)?;
    let events = poll_until(&mut s, 8, |ev| matches!(ev, SessionEvent::Updated(_)));
    assert!(
        !events
            .iter()
            .any(|ev| matches!(ev, SessionEvent::Ended(Some(_)))),
        "session failed: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev, SessionEvent::Updated(_))),
        "{events:?}"
    );
    s.disconnect();
    Ok(())
}

/// Collects log output in memory.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut v) = self.0.lock() {
            v.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn utf16_hex(s: &str) -> String {
    s.encode_utf16()
        .flat_map(u16::to_le_bytes)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn logs_for_connection(filter: tracing_subscriber::EnvFilter) -> Result<String, String> {
    let e = env()?;
    let cap = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(cap.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || -> TestResult {
        let mut s = open(&e, 1280, 800)?;
        poll_until(&mut s, 2, |_| false);
        s.disconnect();
        Ok(())
    })?;
    let bytes = cap
        .0
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "lock".to_string())?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    text.make_ascii_lowercase();
    let _ = std::io::stdout().flush();
    Ok(text)
}

#[test]
#[ignore = "needs a live RDP host (see module docs)"]
fn t7_password_never_reaches_the_logs() -> TestResult {
    let e = env()?;
    let plain = e.password.to_ascii_lowercase();
    let hex = utf16_hex(&e.password);

    // Our policy at the most detailed level this build allows.
    let ours = logs_for_connection(rdpv_core::logging::env_filter("trace"))?;
    assert!(
        !ours.is_empty(),
        "no logs captured; the test would prove nothing"
    );
    assert!(
        !ours.contains(&plain),
        "password found in logs (plain text)"
    );
    assert!(!ours.contains(&hex), "password found in logs (UTF-16 hex)");

    // Control: without the sspi rule, debug logs DO contain the password (SF-2).
    // This proves the check above is able to detect a leak. Only meaningful in
    // debug builds (release builds compile out debug logging entirely).
    if cfg!(debug_assertions) {
        let unsafe_filter = tracing_subscriber::EnvFilter::new("debug");
        let leaky = logs_for_connection(unsafe_filter)?;
        assert!(
            leaky.contains(&hex) || leaky.contains(&plain),
            "control failed: expected the unfiltered sspi logs to contain the password"
        );
    }
    Ok(())
}
