//! Live clipboard tests (Stage 6, C1–C5) against the test-only host in
//! `tools/rdp-test-host` (NLA + scriptable clipboard). Ignored by default.
//!
//! ```sh
//! RDPV_CLIP_HOST=127.0.0.1 RDPV_CLIP_PORT=3391 RDPV_CLIP_USER=<user> RDPV_CLIP_PASSWORD=<password> \
//! RDPV_CLIP_HOST_TEXT=<text the host offers> RDPV_CLIP_OUT=<file the host writes received text to> \
//! cargo test -p rdpv-core --test live_clipboard -- --ignored --test-threads=1
//! ```

use std::time::{Duration, Instant};

use rdpv_core::clipboard::ClipboardPolicy;
use rdpv_core::input::{parse_endpoint, parse_username};
use rdpv_core::policy::ClipboardDirection;
use rdpv_core::secret::SecretString;
use rdpv_core::session::{
    connect, ClipboardOffer, ConnectFailure, ConnectParams, Session, SessionEvent,
};

type TestResult = Result<(), String>;

fn var(k: &str) -> Result<String, String> {
    std::env::var(k).map_err(|_| format!("set {k} to run live clipboard tests"))
}

fn open(direction: ClipboardDirection, max_bytes: usize) -> Result<Session, String> {
    let params = |pin: Option<String>| -> Result<ConnectParams, String> {
        Ok(ConnectParams {
            endpoint: parse_endpoint(&var("RDPV_CLIP_HOST")?, &var("RDPV_CLIP_PORT")?)
                .map_err(|e| format!("{e:?}"))?,
            username: parse_username(&var("RDPV_CLIP_USER")?).map_err(|e| format!("{e:?}"))?,
            password: SecretString::new(var("RDPV_CLIP_PASSWORD")?),
            desktop_width: 1024,
            desktop_height: 768,
            pinned_fingerprint: pin,
            connect_timeout: Duration::from_secs(5),
            clipboard: ClipboardPolicy::from_direction(direction, max_bytes),
        })
    };
    let fp = match connect(params(None)?) {
        Err(ConnectFailure::CertificateNeedsApproval { fingerprint }) => fingerprint,
        other => {
            return Err(format!(
                "expected approval request, got {:?}",
                other.map(|_| "session")
            ))
        }
    };
    connect(params(Some(fp))?).map_err(|f| format!("connect failed: {f:?}"))
}

fn collect(
    s: &mut Session,
    secs: u64,
    mut stop: impl FnMut(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut all = Vec::new();
    while Instant::now() < deadline && !s.is_ended() {
        let ev = s.poll();
        let hit = ev.iter().any(&mut stop);
        all.extend(ev);
        if hit {
            break;
        }
    }
    all
}

fn remote_texts(events: &[SessionEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::RemoteClipboardText(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Lines the host has received from the client so far.
fn host_received() -> Result<Vec<String>, String> {
    let path = var("RDPV_CLIP_OUT")?;
    Ok(std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect())
}

fn wait_host_received(s: &mut Session, expected: &str, secs: u64) -> Result<bool, String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        s.poll();
        if host_received()?.iter().any(|l| l == expected) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn unique(tag: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{tag}-{n}-ünïcødé-😀")
}

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c1_host_to_local() -> TestResult {
    let expected = var("RDPV_CLIP_HOST_TEXT")?;
    let mut s = open(ClipboardDirection::Both, 1024 * 1024)?;
    let events = collect(&mut s, 8, |e| {
        matches!(e, SessionEvent::RemoteClipboardText(_))
    });
    assert_eq!(remote_texts(&events), vec![expected], "{events:?}");
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c2_local_to_host() -> TestResult {
    let mut s = open(ClipboardDirection::Both, 1024 * 1024)?;
    collect(&mut s, 2, |_| false); // let the channel become ready
    let text = unique("local-to-host");
    assert_eq!(
        s.offer_local_clipboard(text.clone()),
        Ok(ClipboardOffer::Offered)
    );
    assert!(
        wait_host_received(&mut s, &text, 8)?,
        "host never received the text"
    );
    // Offering the same text again does nothing.
    assert_eq!(s.offer_local_clipboard(text), Ok(ClipboardOffer::Unchanged));
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c3_clipboard_off_shares_nothing() -> TestResult {
    let mut s = open(ClipboardDirection::Off, 1024 * 1024)?;
    let events = collect(&mut s, 4, |_| false);
    assert!(remote_texts(&events).is_empty(), "{events:?}");
    let text = unique("must-not-arrive-off");
    assert_eq!(
        s.offer_local_clipboard(text.clone()),
        Ok(ClipboardOffer::Disabled)
    );
    assert!(
        !wait_host_received(&mut s, &text, 3)?,
        "text reached the host with clipboard off"
    );
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c4_one_direction_only() -> TestResult {
    // Only host -> local: host text arrives, local text is not offered.
    let mut s = open(ClipboardDirection::HostToLocal, 1024 * 1024)?;
    let events = collect(&mut s, 8, |e| {
        matches!(e, SessionEvent::RemoteClipboardText(_))
    });
    assert_eq!(remote_texts(&events).len(), 1, "{events:?}");
    let text = unique("must-not-arrive-one-way");
    assert_eq!(
        s.offer_local_clipboard(text.clone()),
        Ok(ClipboardOffer::Disabled)
    );
    assert!(!wait_host_received(&mut s, &text, 3)?);
    s.disconnect();

    // Only local -> host: local text arrives at the host, host text is not taken.
    let mut s = open(ClipboardDirection::LocalToHost, 1024 * 1024)?;
    let events = collect(&mut s, 3, |_| false);
    assert!(remote_texts(&events).is_empty(), "{events:?}");
    let text = unique("local-only");
    assert_eq!(
        s.offer_local_clipboard(text.clone()),
        Ok(ClipboardOffer::Offered)
    );
    assert!(wait_host_received(&mut s, &text, 8)?);
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c5_size_limit() -> TestResult {
    // 8 bytes = 4 UTF-16 units; the host's text is longer.
    let mut s = open(ClipboardDirection::Both, 8)?;
    let events = collect(&mut s, 8, |e| {
        matches!(e, SessionEvent::RemoteClipboardTooLarge)
    });
    assert!(remote_texts(&events).is_empty(), "{events:?}");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::RemoteClipboardTooLarge)),
        "{events:?}"
    );
    assert_eq!(
        s.offer_local_clipboard("too long".into()),
        Ok(ClipboardOffer::TooLarge)
    );
    s.disconnect();
    Ok(())
}

#[derive(Clone, Default)]
struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

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

#[test]
#[ignore = "needs the clipboard test host (see module docs)"]
fn c6_clipboard_text_never_reaches_the_logs() -> TestResult {
    let host_text = var("RDPV_CLIP_HOST_TEXT")?;
    let local_text = unique("log-check");
    let cap = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(rdpv_core::logging::env_filter("trace"))
        .with_writer(cap.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || -> TestResult {
        let mut s = open(ClipboardDirection::Both, 1024 * 1024)?;
        let events = collect(&mut s, 8, |e| {
            matches!(e, SessionEvent::RemoteClipboardText(_))
        });
        assert_eq!(remote_texts(&events), vec![host_text.clone()]);
        s.offer_local_clipboard(local_text.clone())
            .map_err(|e| format!("{e:?}"))?;
        assert!(wait_host_received(&mut s, &local_text, 8)?);
        s.disconnect();
        Ok(())
    })?;
    let logs = cap
        .0
        .lock()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .map_err(|_| "lock")?;
    assert!(
        !logs.is_empty(),
        "no logs captured; the test would prove nothing"
    );
    let utf16_hex = |t: &str| -> String {
        t.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let lower = logs.to_ascii_lowercase();
    for t in [&host_text, &local_text] {
        assert!(!logs.contains(t.as_str()), "clipboard text found in logs");
        assert!(
            !lower.contains(&utf16_hex(t)),
            "clipboard text found in logs (UTF-16 hex)"
        );
    }
    Ok(())
}
