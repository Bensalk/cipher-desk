//! Remote desktop sessions for the UI (Stage 4).
//!
//! [`start_session`] runs the connection on a worker thread and streams
//! [`SessionUpdate`]s to Flutter. The returned [`RdpSession`] handle sends
//! input, answers the certificate prompt and disconnects.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use rdpv_core::input::{parse_endpoint, parse_username};
use rdpv_core::pins::PinStore;
use rdpv_core::policy::SecurityDefaults;
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
        }
    }

    fn status(label: &str, connected: bool) -> Self {
        Self {
            label: Some(label.to_owned()),
            connected,
            ..Self::new(UpdateKind::Status)
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
    ApproveCertificate(bool),
    Disconnect,
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
        sink: StreamSink<SessionUpdate>,
    ) {
        let target = Target {
            host,
            port,
            username,
            width,
            height,
            pins_path,
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
        sink: StreamSink<SessionUpdate>,
    ) {
        let target = Target {
            host,
            port,
            username,
            width,
            height,
            pins_path,
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
                run(&target, password, &sink, &rx);
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

    /// Answers the certificate prompt.
    #[flutter_rust_bridge::frb(sync)]
    pub fn approve_certificate(&self, approve: bool) {
        let _ = self.tx.send(Command::ApproveCertificate(approve));
    }

    /// Disconnects politely.
    #[flutter_rust_bridge::frb(sync)]
    pub fn disconnect(&self) {
        let _ = self.tx.send(Command::Disconnect);
    }

    fn send(&self, events: Vec<InputEvent>) {
        if !events.is_empty() {
            let _ = self.tx.send(Command::Input(events));
        }
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
}

fn run(
    target: &Target,
    password: SecretString,
    sink: &StreamSink<SessionUpdate>,
    rx: &Receiver<Command>,
) {
    let Target {
        host,
        port,
        username,
        width,
        height,
        pins_path,
    } = target;
    let (port, width, height) = (*port, *width, *height);
    let send = |u: SessionUpdate| {
        let _ = sink.add(u);
    };
    let fail = |e: ConnectionError| {
        let _ = sink.add(SessionUpdate::ended(Some(e)));
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

    let session = loop {
        let params = ConnectParams {
            endpoint: endpoint.clone(),
            username: user.clone(),
            password: SecretString::new(password.expose().to_owned()),
            desktop_width: width,
            desktop_height: height,
            pinned_fingerprint: pin.clone(),
            connect_timeout: policy.connect_timeout,
        };
        match connect(params) {
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
                        return send(SessionUpdate::ended(None));
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
    drop(password);
    drive(session, sink, rx, policy.idle_disconnect);
}

/// Waits for the user's answer. `None` means disconnect was requested.
fn wait_for_approval(rx: &Receiver<Command>) -> Option<bool> {
    loop {
        match rx.recv() {
            Ok(Command::ApproveCertificate(a)) => return Some(a),
            Ok(Command::Disconnect) | Err(_) => return None,
            Ok(Command::Input(_)) => {}
        }
    }
}

fn drive(
    mut s: Session,
    sink: &StreamSink<SessionUpdate>,
    rx: &Receiver<Command>,
    idle_limit: Duration,
) {
    let send = |u: SessionUpdate| sink.add(u).is_ok();
    let (w, h) = s.size();
    send(SessionUpdate::resized(w, h));
    send(SessionUpdate::status("Connected", true));
    let mut last_input = std::time::Instant::now();

    loop {
        // Input and control commands first, so typing stays responsive.
        loop {
            match rx.recv_timeout(Duration::ZERO) {
                Ok(Command::Input(events)) => {
                    last_input = std::time::Instant::now();
                    if let Err(e) = s.send_input(events) {
                        let _ = sink.add(SessionUpdate::ended(Some(e)));
                        return;
                    }
                }
                Ok(Command::ApproveCertificate(_)) => {}
                Ok(Command::Disconnect) | Err(RecvTimeoutError::Disconnected) => {
                    s.disconnect();
                    let _ = sink.add(SessionUpdate::ended(None));
                    return;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }

        // S7: idle disconnect.
        if last_input.elapsed() >= idle_limit {
            s.disconnect();
            let _ = sink.add(SessionUpdate::ended(Some(ConnectionError::IdleTimeout)));
            return;
        }

        let mut dirty: Option<Region> = None;
        for ev in s.poll() {
            match ev {
                SessionEvent::Updated(r) => dirty = Some(union(dirty, r)),
                SessionEvent::Resized { width, height } => {
                    if !send(SessionUpdate::resized(width, height)) {
                        s.disconnect();
                        return;
                    }
                    let full = Region {
                        left: 0,
                        top: 0,
                        right: width.saturating_sub(1),
                        bottom: height.saturating_sub(1),
                    };
                    dirty = Some(full);
                }
                SessionEvent::Ended(err) => {
                    let _ = sink.add(SessionUpdate::ended(err));
                    return;
                }
            }
        }
        if let Some(r) = dirty {
            let update = SessionUpdate::frame(r, s.region_pixels(r));
            // The UI went away: end the session.
            if !send(update) {
                s.disconnect();
                return;
            }
        }
    }
}

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
}
