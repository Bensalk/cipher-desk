//! Remote desktop sessions for the UI (Stage 4).
//!
//! [`RdpSession::start`] runs the connection on a worker thread and streams
//! [`SessionUpdate`]s to Flutter. The [`RdpSession`] handle sends input,
//! answers the certificate prompt and disconnects.
//!
//! Stage 8: if the network drops during a session, the worker reconnects
//! automatically (up to 3 tries, 2 s / 5 s / 10 s apart) to the same computer,
//! which must present the same trusted certificate. It never retries after a
//! password, account or certificate problem (SA-8).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use rdpv_core::clipboard::ClipboardPolicy;
use rdpv_core::input::{parse_endpoint, parse_username};
use rdpv_core::pins::PinStore;
use rdpv_core::policy::{ClipboardDirection, SecurityDefaults};
use rdpv_core::secret::SecretString;
use rdpv_core::session::{
    connect, ConnectFailure, ConnectParams, InputEvent, PointerButton, Region, Session,
    SessionEvent,
};
use rdpv_core::status::ConnectionError;

use crate::api::errors::{error_info, ErrorInfo};
use crate::frb_generated::StreamSink;

/// What kind of [`SessionUpdate`] this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateKind {
    /// Status text for the status indicator (`label`, `connected`).
    Status,
    /// First connection to this computer: ask the user to compare and approve
    /// `fingerprint`. Nothing has been sent to the computer.
    CertificateApproval,
    /// The certificate changed since it was approved (`pinned`, `fingerprint` =
    /// presented). The connection was blocked and nothing was sent.
    CertificateChanged,
    /// The desktop size (`width`, `height`), before the first frame and after resizes.
    Resized,
    /// Changed pixels: `rgba` rows for the rectangle (`x`, `y`, `width`, `height`).
    Frame,
    /// The session ended. `error` is `None` when the user disconnected.
    Ended,
    /// Text copied on the remote computer (`text`), for this device's clipboard.
    RemoteClipboard,
    /// Text copied on the remote computer was larger than allowed and was dropped.
    RemoteClipboardTooLarge,
    /// The network dropped; reconnect attempt `x` of `y` is under way (`label`).
    /// Input is ignored until `Status` with `connected` arrives again.
    Reconnecting,
}

/// Something the UI should show. Only the fields named for its `kind` are set.
#[derive(Debug, Clone)]
pub struct SessionUpdate {
    pub kind: UpdateKind,
    pub label: Option<String>,
    pub connected: bool,
    pub fingerprint: Option<String>,
    pub pinned: Option<String>,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub error: Option<ErrorInfo>,
    pub text: Option<String>,
}

impl SessionUpdate {
    fn new(kind: UpdateKind) -> Self {
        Self {
            kind,
            label: None,
            connected: false,
            fingerprint: None,
            pinned: None,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            rgba: Vec::new(),
            error: None,
            text: None,
        }
    }

    fn remote_clipboard(text: String) -> Self {
        Self {
            text: Some(text),
            ..Self::new(UpdateKind::RemoteClipboard)
        }
    }

    fn status(label: &str, connected: bool) -> Self {
        Self {
            label: Some(label.to_owned()),
            connected,
            ..Self::new(UpdateKind::Status)
        }
    }

    fn reconnecting(attempt: u8, max: u8) -> Self {
        Self {
            label: Some(format!(
                "Connection lost. Reconnecting ({attempt} of {max})…"
            )),
            x: u16::from(attempt),
            y: u16::from(max),
            ..Self::new(UpdateKind::Reconnecting)
        }
    }

    fn approval(fingerprint: String) -> Self {
        Self {
            fingerprint: Some(fingerprint),
            ..Self::new(UpdateKind::CertificateApproval)
        }
    }

    fn changed(pinned: String, presented: String) -> Self {
        Self {
            pinned: Some(pinned),
            fingerprint: Some(presented),
            ..Self::new(UpdateKind::CertificateChanged)
        }
    }

    fn resized(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            ..Self::new(UpdateKind::Resized)
        }
    }

    fn frame(r: Region, rgba: Vec<u8>) -> Self {
        Self {
            x: r.left,
            y: r.top,
            width: r.right.saturating_sub(r.left).saturating_add(1),
            height: r.bottom.saturating_sub(r.top).saturating_add(1),
            rgba,
            ..Self::new(UpdateKind::Frame)
        }
    }

    fn ended(error: Option<ConnectionError>) -> Self {
        Self {
            error: error.map(error_info),
            ..Self::new(UpdateKind::Ended)
        }
    }
}

enum Command {
    Input(Vec<InputEvent>),
    OfferClipboard(String),
    ApproveCertificate(bool),
    /// End the session: `None` = the user disconnected; otherwise the reason
    /// shown (e.g. the Android background grace period ended).
    Disconnect(Option<ConnectionError>),
}

/// Handle to a session. Create it with [`RdpSession::create`], then call
/// [`RdpSession::start`] to connect and receive updates.
pub struct RdpSession {
    tx: Sender<Command>,
    rx: std::sync::Mutex<Option<Receiver<Command>>>,
}

impl RdpSession {
    /// Creates a handle (no connection yet).
    #[flutter_rust_bridge::frb(sync)]
    pub fn create() -> RdpSession {
        let (tx, rx) = mpsc::channel();
        RdpSession {
            tx,
            rx: std::sync::Mutex::new(Some(rx)),
        }
    }

    /// Connects on a worker thread; updates arrive on `sink` (a Dart stream).
    /// The password is used for this connection only and never stored here.
    /// `pins_path` is the certificate pin file in the app's private data folder.
    /// Can be called once per handle.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        host: String,
        port: u16,
        username: String,
        password: String,
        width: u16,
        height: u16,
        pins_path: String,
        clipboard_direction: ClipboardMode,
        idle_minutes: u32,
        sink: StreamSink<SessionUpdate>,
    ) {
        let target = Target {
            host,
            port,
            username,
            width,
            height,
            pins_path,
            clipboard: clipboard_direction,
            idle_minutes,
        };
        self.spawn(target, Some(SecretString::new(password)), None, sink);
    }

    /// Like [`RdpSession::start`], but reads the saved password from the OS
    /// vault inside Rust, so it never passes through the UI (Windows, Linux).
    /// The caller must have confirmed the user's identity first (S5).
    #[allow(clippy::too_many_arguments)]
    pub fn start_with_saved_password(
        &self,
        connection_id: String,
        host: String,
        port: u16,
        username: String,
        width: u16,
        height: u16,
        pins_path: String,
        clipboard_direction: ClipboardMode,
        idle_minutes: u32,
        sink: StreamSink<SessionUpdate>,
    ) {
        let target = Target {
            host,
            port,
            username,
            width,
            height,
            pins_path,
            clipboard: clipboard_direction,
            idle_minutes,
        };
        self.spawn(target, None, Some(connection_id), sink);
    }

    fn spawn(
        &self,
        target: Target,
        password: Option<SecretString>,
        saved_id: Option<String>,
        sink: StreamSink<SessionUpdate>,
    ) {
        let rx = self.rx.lock().ok().and_then(|mut g| g.take());
        let Some(rx) = rx else {
            let _ = sink.add(SessionUpdate::ended(Some(ConnectionError::Unexpected)));
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("rdp-session".into())
            .spawn(move || {
                let password = match (password, saved_id) {
                    (Some(p), _) => p,
                    (None, Some(id)) => match rdpv_core::vault::load(&id) {
                        Ok(p) => p,
                        Err(_) => {
                            let _ = sink.add(SessionUpdate::ended(Some(
                                ConnectionError::VaultUnavailable,
                            )));
                            return;
                        }
                    },
                    (None, None) => {
                        let _ = sink.add(SessionUpdate::ended(Some(ConnectionError::Unexpected)));
                        return;
                    }
                };
                let send = |u: SessionUpdate| sink.add(u).is_ok();
                run(&target, password, &send, &rx);
            });
        if spawned.is_err() {
            tracing::error!("could not start the session thread");
        }
    }

    /// Moves the remote mouse pointer (remote-desktop coordinates).
    #[flutter_rust_bridge::frb(sync)]
    pub fn mouse_move(&self, x: u16, y: u16) {
        self.send(vec![InputEvent::MouseMove { x, y }]);
    }

    /// Presses or releases a mouse button: 0 = left, 1 = right, 2 = middle.
    #[flutter_rust_bridge::frb(sync)]
    pub fn mouse_button(&self, x: u16, y: u16, button: u8, down: bool) {
        let b = match button {
            1 => PointerButton::Right,
            2 => PointerButton::Middle,
            _ => PointerButton::Left,
        };
        let press = if down {
            InputEvent::MouseDown(b)
        } else {
            InputEvent::MouseUp(b)
        };
        self.send(vec![InputEvent::MouseMove { x, y }, press]);
    }

    /// Scrolls; one notch is 120, positive scrolls up.
    #[flutter_rust_bridge::frb(sync)]
    pub fn wheel(&self, vertical: bool, delta: i16) {
        self.send(vec![InputEvent::Wheel { vertical, delta }]);
    }

    /// Presses or releases a key by PC scancode (set 1).
    #[flutter_rust_bridge::frb(sync)]
    pub fn key(&self, scancode: u8, extended: bool, down: bool) {
        let e = if down {
            InputEvent::KeyDown { scancode, extended }
        } else {
            InputEvent::KeyUp { scancode, extended }
        };
        self.send(vec![e]);
    }

    /// Types text (on-screen keyboards, IMEs, pasted text is NOT sent this way).
    #[flutter_rust_bridge::frb(sync)]
    pub fn type_text(&self, text: String) {
        let events = text
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .flat_map(|ch| {
                [
                    InputEvent::Unicode { ch, down: true },
                    InputEvent::Unicode { ch, down: false },
                ]
            })
            .collect();
        self.send(events);
    }

    /// Sends Ctrl+Alt+Del to the remote computer.
    #[flutter_rust_bridge::frb(sync)]
    pub fn ctrl_alt_del(&self) {
        let (ctrl, alt, del) = ((0x1D, false), (0x38, false), (0x53, true));
        let down = |(s, e): (u8, bool)| InputEvent::KeyDown {
            scancode: s,
            extended: e,
        };
        let up = |(s, e): (u8, bool)| InputEvent::KeyUp {
            scancode: s,
            extended: e,
        };
        self.send(vec![
            down(ctrl),
            down(alt),
            down(del),
            up(del),
            up(alt),
            up(ctrl),
        ]);
    }

    /// Offers this device's clipboard text to the remote computer. It is sent
    /// only if clipboard sharing to the remote is allowed for this connection,
    /// and only when the remote pastes.
    #[flutter_rust_bridge::frb(sync)]
    pub fn offer_clipboard_text(&self, text: String) {
        let _ = self.tx.send(Command::OfferClipboard(text));
    }

    /// Answers the certificate prompt.
    #[flutter_rust_bridge::frb(sync)]
    pub fn approve_certificate(&self, approve: bool) {
        let _ = self.tx.send(Command::ApproveCertificate(approve));
    }

    /// Disconnects politely.
    #[flutter_rust_bridge::frb(sync)]
    pub fn disconnect(&self) {
        let _ = self.tx.send(Command::Disconnect(None));
    }

    /// Disconnects politely because the app stayed in the background too long
    /// (Android, decision 8). The session ends with that reason, so the UI can
    /// explain it and offer to reconnect.
    #[flutter_rust_bridge::frb(sync)]
    pub fn disconnect_in_background(&self) {
        let _ = self
            .tx
            .send(Command::Disconnect(Some(ConnectionError::AppInBackground)));
    }

    fn send(&self, events: Vec<InputEvent>) {
        if !events.is_empty() {
            let _ = self.tx.send(Command::Input(events));
        }
    }
}

/// Clipboard sharing for one connection (S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Off,
    /// Both directions.
    Both,
    /// Only from this device to the remote computer.
    ToRemoteOnly,
    /// Only from the remote computer to this device.
    FromRemoteOnly,
}

impl ClipboardMode {
    fn policy(self) -> ClipboardPolicy {
        let d = match self {
            Self::Off => ClipboardDirection::Off,
            Self::Both => ClipboardDirection::Both,
            Self::ToRemoteOnly => ClipboardDirection::LocalToHost,
            Self::FromRemoteOnly => ClipboardDirection::HostToLocal,
        };
        ClipboardPolicy::from_direction(d, SecurityDefaults::secure().clipboard_max_bytes)
    }
}

/// Where to connect (everything except the password).
struct Target {
    host: String,
    port: u16,
    username: String,
    width: u16,
    height: u16,
    pins_path: String,
    clipboard: ClipboardMode,
    /// Disconnect after this many minutes without input; 0 = never (S7).
    idle_minutes: u32,
}

/// Sends an update to the UI; `false` means the UI has gone away.
type Send<'a> = &'a dyn Fn(SessionUpdate) -> bool;

fn run(target: &Target, password: SecretString, send: Send<'_>, rx: &Receiver<Command>) {
    let Target {
        host,
        port,
        username,
        width,
        height,
        pins_path,
        clipboard,
        idle_minutes,
    } = target;
    let (port, width, height) = (*port, *width, *height);
    let fail = |e: ConnectionError| {
        send(SessionUpdate::ended(Some(e)));
    };
    send(SessionUpdate::status("Connecting…", false));

    let (Ok(endpoint), Ok(user)) = (
        parse_endpoint(host, &port.to_string()),
        parse_username(username),
    ) else {
        return fail(ConnectionError::Unexpected);
    };
    let mut pins = match PinStore::open(pins_path) {
        Ok(p) => p,
        Err(_) => return fail(ConnectionError::Unexpected),
    };
    let key = endpoint.display();
    let policy = SecurityDefaults::secure();
    let mut pin = pins.get(&key).map(str::to_owned);
    // The password stays in this thread (wiped when dropped) for the whole
    // session, so a dropped network can be reconnected without asking again.
    let params = |pin: Option<String>| ConnectParams {
        endpoint: endpoint.clone(),
        username: user.clone(),
        password: SecretString::new(password.expose().to_owned()),
        desktop_width: width,
        desktop_height: height,
        pinned_fingerprint: pin,
        connect_timeout: policy.connect_timeout,
        clipboard: clipboard.policy(),
    };

    let mut session = loop {
        match connect(params(pin.clone())) {
            Ok(s) => break s,
            Err(ConnectFailure::CertificateNeedsApproval { fingerprint }) => {
                send(SessionUpdate::status("Check computer identity", false));
                send(SessionUpdate::approval(fingerprint.clone()));
                match wait_for_approval(rx) {
                    Some(true) => {
                        if pins.pin(&key, &fingerprint).is_err() {
                            return fail(ConnectionError::Unexpected);
                        }
                        pin = Some(fingerprint);
                        send(SessionUpdate::status("Connecting…", false));
                    }
                    Some(false) => return fail(ConnectionError::CertificateRejected),
                    None => {
                        send(SessionUpdate::ended(None));
                        return;
                    }
                }
            }
            Err(ConnectFailure::CertificateChanged { pinned, presented }) => {
                send(SessionUpdate::changed(pinned, presented));
                return fail(ConnectionError::CertificateChanged);
            }
            Err(ConnectFailure::Failed(e)) => return fail(e),
        }
    };
    let idle = (*idle_minutes > 0).then(|| Duration::from_secs(u64::from(*idle_minutes) * 60));

    loop {
        match drive(session, send, rx, idle) {
            Drive::Ended(err) => {
                send(SessionUpdate::ended(err));
                return;
            }
            Drive::UiGone => return,
            Drive::Lost(err) => {
                match reconnect(
                    &|| connect(params(pin.clone())),
                    send,
                    rx,
                    policy.max_auto_reconnects,
                    &ConnectionError::reconnect_delay,
                    err,
                ) {
                    Reconnect::Connected(s) => session = *s,
                    Reconnect::Stopped => return,
                }
            }
        }
    }
}

/// How [`drive`] finished.
enum Drive {
    /// The session is over: user disconnect (`None`) or a final error.
    Ended(Option<ConnectionError>),
    /// The network dropped; reconnecting is allowed.
    Lost(ConnectionError),
    /// The UI went away (screen closed); nothing more to send.
    UiGone,
}

enum Reconnect {
    Connected(Box<Session>),
    /// Gave up or the user disconnected; the final update was sent.
    Stopped,
}

/// Reconnects after a network drop (F13, SA-8): up to `max` attempts with
/// growing pauses. The computer must present the trusted certificate (the
/// connection is pinned); a changed certificate or any password/account error
/// stops at once. The user can disconnect during the pauses.
fn reconnect(
    connect_again: &dyn Fn() -> Result<Session, ConnectFailure>,
    send: Send<'_>,
    rx: &Receiver<Command>,
    max: u8,
    delay: &dyn Fn(u8) -> Duration,
    first: ConnectionError,
) -> Reconnect {
    let stop = |err: Option<ConnectionError>| {
        send(SessionUpdate::ended(err));
        Reconnect::Stopped
    };
    tracing::info!(reason = ?first, "connection lost; reconnecting");
    for attempt in 1..=max {
        if !send(SessionUpdate::reconnecting(attempt, max)) {
            return Reconnect::Stopped;
        }
        if let Some(reason) = pause(rx, delay(attempt)) {
            return stop(reason);
        }
        match connect_again() {
            Ok(s) => {
                // A disconnect pressed while connecting still wins.
                if let Some(reason) = pending_disconnect(rx) {
                    s.disconnect();
                    return stop(reason);
                }
                return Reconnect::Connected(Box::new(s));
            }
            Err(ConnectFailure::CertificateChanged { pinned, presented }) => {
                send(SessionUpdate::changed(pinned, presented));
                return stop(Some(ConnectionError::CertificateChanged));
            }
            Err(ConnectFailure::CertificateNeedsApproval { .. }) => {
                // Cannot happen with a pinned certificate; never prompt silently.
                return stop(Some(ConnectionError::CertificateUnknown));
            }
            Err(ConnectFailure::Failed(e)) if e.auto_reconnect_allowed() => {}
            // Wrong password, account locked, etc.: stop at once (SA-8).
            Err(ConnectFailure::Failed(e)) => return stop(Some(e)),
        }
    }
    // Out of attempts: report the drop itself, whose advice says what to check.
    stop(Some(ConnectionError::ConnectionLost))
}

/// Waits `d`, dropping input. Returns `Some(reason)` if a disconnect was
/// requested (or the UI went away) meanwhile.
fn pause(rx: &Receiver<Command>, d: Duration) -> Option<Option<ConnectionError>> {
    let until = std::time::Instant::now() + d;
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return None;
        }
        match rx.recv_timeout(left) {
            Ok(Command::Disconnect(reason)) => return Some(reason),
            Err(RecvTimeoutError::Disconnected) => return Some(None),
            Err(RecvTimeoutError::Timeout) => return None,
            // Typing, clicks and clipboard offers during the outage are dropped.
            Ok(_) => {}
        }
    }
}

/// A disconnect request already waiting in the queue, if any.
fn pending_disconnect(rx: &Receiver<Command>) -> Option<Option<ConnectionError>> {
    loop {
        match rx.try_recv() {
            Ok(Command::Disconnect(reason)) => return Some(reason),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => return Some(None),
        }
    }
}

/// Waits for the user's answer. `None` means disconnect was requested.
fn wait_for_approval(rx: &Receiver<Command>) -> Option<bool> {
    loop {
        match rx.recv() {
            Ok(Command::ApproveCertificate(a)) => return Some(a),
            Ok(Command::Disconnect(_)) | Err(_) => return None,
            // Input and clipboard offers before the session exists are dropped.
            Ok(Command::Input(_) | Command::OfferClipboard(_)) => {}
        }
    }
}

fn drive(
    mut s: Session,
    send: Send<'_>,
    rx: &Receiver<Command>,
    idle_limit: Option<Duration>,
) -> Drive {
    let (w, h) = s.size();
    send(SessionUpdate::resized(w, h));
    if !send(SessionUpdate::status("Connected", true)) {
        s.disconnect();
        return Drive::UiGone;
    }
    let mut last_input = std::time::Instant::now();
    // Changed screen area not yet sent, and when pixels were last sent: at most
    // ~30 updates a second, so a busy screen (video, scrolling) cannot queue up
    // more pixels than the UI can draw, which would make the session lag.
    let mut dirty: Option<Region> = None;
    let mut last_frame = std::time::Instant::now()
        .checked_sub(FRAME_INTERVAL)
        .unwrap_or_else(std::time::Instant::now);
    // A session error: network drops may be reconnected, the rest end it.
    let ended = |err: ConnectionError| {
        if err.auto_reconnect_allowed() {
            Drive::Lost(err)
        } else {
            Drive::Ended(Some(err))
        }
    };

    loop {
        // Input and control commands first, so typing stays responsive.
        loop {
            match rx.recv_timeout(Duration::ZERO) {
                Ok(Command::Input(events)) => {
                    last_input = std::time::Instant::now();
                    if let Err(e) = s.send_input(events) {
                        return ended(e);
                    }
                }
                Ok(Command::OfferClipboard(text)) => {
                    // Copying on this device is not user input to the session.
                    if let Err(e) = s.offer_local_clipboard(text) {
                        return ended(e);
                    }
                }
                Ok(Command::ApproveCertificate(_)) => {}
                Ok(Command::Disconnect(reason)) => {
                    s.disconnect();
                    return Drive::Ended(reason);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    s.disconnect();
                    return Drive::UiGone;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }

        // S7: idle disconnect.
        if idle_limit.is_some_and(|limit| last_input.elapsed() >= limit) {
            s.disconnect();
            return Drive::Ended(Some(ConnectionError::IdleTimeout));
        }

        for ev in s.poll() {
            match ev {
                SessionEvent::Updated(r) => dirty = Some(union(dirty, r)),
                SessionEvent::Resized { width, height } => {
                    if !send(SessionUpdate::resized(width, height)) {
                        s.disconnect();
                        return Drive::UiGone;
                    }
                    let full = Region {
                        left: 0,
                        top: 0,
                        right: width.saturating_sub(1),
                        bottom: height.saturating_sub(1),
                    };
                    dirty = Some(full);
                }
                SessionEvent::RemoteClipboardText(text) => {
                    send(SessionUpdate::remote_clipboard(text));
                }
                SessionEvent::RemoteClipboardTooLarge => {
                    send(SessionUpdate::new(UpdateKind::RemoteClipboardTooLarge));
                }
                SessionEvent::Ended(None) => return Drive::Ended(None),
                SessionEvent::Ended(Some(err)) => return ended(err),
            }
        }
        // The rectangle sent to the UI is the one the pixels were copied from.
        if dirty.is_some() && last_frame.elapsed() >= FRAME_INTERVAL {
            if let Some(r) = dirty.take().and_then(|r| s.clamped(r)) {
                last_frame = std::time::Instant::now();
                let update = SessionUpdate::frame(r, s.region_pixels(r));
                // The UI went away: end the session.
                if !send(update) {
                    s.disconnect();
                    return Drive::UiGone;
                }
            }
        }
    }
}

/// Shortest time between two screen updates sent to the UI (about 30 per second).
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

fn union(a: Option<Region>, b: Region) -> Region {
    match a {
        None => b,
        Some(a) => Region {
            left: a.left.min(b.left),
            top: a.top.min(b.top),
            right: a.right.max(b.right),
            bottom: a.bottom.max(b.bottom),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_covers_both_regions() {
        let a = Region {
            left: 10,
            top: 10,
            right: 20,
            bottom: 20,
        };
        let b = Region {
            left: 5,
            top: 15,
            right: 30,
            bottom: 18,
        };
        assert_eq!(
            union(Some(a), b),
            Region {
                left: 5,
                top: 10,
                right: 30,
                bottom: 20
            }
        );
        assert_eq!(union(None, b), b);
    }

    // ------------------------------------------------ reconnect rules (F13, SA-8)

    use std::cell::{Cell, RefCell};

    struct Run {
        updates: Vec<SessionUpdate>,
        attempts: u32,
        stopped: bool,
    }

    /// Runs `reconnect` with a fake connector that fails with `results` in turn.
    fn try_reconnect(results: Vec<ConnectFailure>, before: impl FnOnce(&Sender<Command>)) -> Run {
        let (tx, rx) = mpsc::channel();
        before(&tx);
        let updates = RefCell::new(Vec::new());
        let send = |u: SessionUpdate| {
            updates.borrow_mut().push(u);
            true
        };
        let attempts = Cell::new(0u32);
        let results = RefCell::new(results.into_iter());
        let connect_again = || {
            attempts.set(attempts.get() + 1);
            Err(results
                .borrow_mut()
                .next()
                .unwrap_or(ConnectFailure::Failed(ConnectionError::Timeout)))
        };
        let out = reconnect(
            &connect_again,
            &send,
            &rx,
            3,
            &|_| Duration::from_millis(1),
            ConnectionError::ConnectionLost,
        );
        drop(tx);
        Run {
            updates: updates.into_inner(),
            attempts: attempts.get(),
            stopped: matches!(out, Reconnect::Stopped),
        }
    }

    fn final_error(r: &Run) -> Option<String> {
        let Some(last) = r.updates.last() else {
            panic!("no updates were sent");
        };
        assert_eq!(last.kind, UpdateKind::Ended);
        last.error.as_ref().map(|e| e.id.clone())
    }

    fn fail(e: ConnectionError) -> ConnectFailure {
        ConnectFailure::Failed(e)
    }

    #[test]
    fn wrong_password_while_reconnecting_stops_at_once() {
        let r = try_reconnect(vec![fail(ConnectionError::WrongCredentials)], |_| {});
        assert!(r.stopped);
        assert_eq!(
            r.attempts, 1,
            "a wrong password must never be retried (SA-8)"
        );
        assert_eq!(final_error(&r).as_deref(), Some("WrongCredentials"));
    }

    #[test]
    fn account_problem_while_reconnecting_stops_at_once() {
        let r = try_reconnect(vec![fail(ConnectionError::AccountProblem)], |_| {});
        assert_eq!(r.attempts, 1);
        assert_eq!(final_error(&r).as_deref(), Some("AccountProblem"));
    }

    #[test]
    fn network_errors_are_retried_up_to_the_limit() {
        let r = try_reconnect(
            vec![
                fail(ConnectionError::Timeout),
                fail(ConnectionError::HostRefused),
                fail(ConnectionError::HostNotFound),
            ],
            |_| {},
        );
        assert_eq!(r.attempts, 3);
        let shown: Vec<_> = r
            .updates
            .iter()
            .filter(|u| u.kind == UpdateKind::Reconnecting)
            .map(|u| (u.x, u.y))
            .collect();
        assert_eq!(shown, [(1, 3), (2, 3), (3, 3)]);
        assert_eq!(final_error(&r).as_deref(), Some("ConnectionLost"));
    }

    #[test]
    fn changed_certificate_while_reconnecting_is_blocked() {
        let r = try_reconnect(
            vec![ConnectFailure::CertificateChanged {
                pinned: "AA".into(),
                presented: "BB".into(),
            }],
            |_| {},
        );
        assert_eq!(r.attempts, 1);
        assert!(r
            .updates
            .iter()
            .any(|u| u.kind == UpdateKind::CertificateChanged));
        assert_eq!(final_error(&r).as_deref(), Some("CertificateChanged"));
    }

    #[test]
    fn unpinned_certificate_while_reconnecting_never_prompts() {
        let r = try_reconnect(
            vec![ConnectFailure::CertificateNeedsApproval {
                fingerprint: "AA".into(),
            }],
            |_| {},
        );
        assert!(!r
            .updates
            .iter()
            .any(|u| u.kind == UpdateKind::CertificateApproval));
        assert_eq!(final_error(&r).as_deref(), Some("CertificateUnknown"));
    }

    #[test]
    fn user_can_disconnect_while_waiting() {
        let r = try_reconnect(vec![], |tx| {
            let _ = tx.send(Command::Input(vec![InputEvent::MouseMove { x: 1, y: 1 }]));
            let _ = tx.send(Command::Disconnect(None));
        });
        assert_eq!(
            r.attempts, 0,
            "no connection attempt after the user disconnected"
        );
        assert_eq!(final_error(&r), None);
    }

    #[test]
    fn background_disconnect_while_waiting_keeps_its_reason() {
        let r = try_reconnect(vec![], |tx| {
            let _ = tx.send(Command::Disconnect(Some(ConnectionError::AppInBackground)));
        });
        assert_eq!(r.attempts, 0);
        assert_eq!(final_error(&r).as_deref(), Some("AppInBackground"));
    }

    #[test]
    fn reconnect_messages_contain_no_user_data() {
        let u = SessionUpdate::reconnecting(2, 3);
        assert_eq!(
            u.label.as_deref(),
            Some("Connection lost. Reconnecting (2 of 3)…")
        );
    }
}

#[cfg(test)]
mod live_tests;
