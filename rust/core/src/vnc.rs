//! VNC connections (the "VNC" connection method), with the same shape as an
//! RDP [`Session`](crate::session::Session) so the app can show both alike.
//!
//! VNC itself is not encrypted (only the password check is protected), so
//! Cipher Desk connects only to addresses on the user's own network, the
//! computer itself, or Tailscale (100.64.0.0/10), which encrypts the traffic.
//! Anything else is refused before a single byte is sent
//! ([`ConnectionError::VncNotPrivate`]).
//!
//! The network part runs on its own thread (the VNC library is async); the
//! picture is kept here as RGBA.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs as _};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vnc::{
    ClientKeyEvent, ClientMouseEvent, PixelFormat, VncConnector, VncEncoding, VncEvent, X11Event,
};

use crate::input::Endpoint;
use crate::secret::SecretString;
use crate::session::{InputEvent, PointerButton, Region, SessionEvent, POLL_BUDGET, POLL_INTERVAL};
use crate::status::ConnectionError;

/// Everything needed to open a VNC connection. VNC has no username.
#[derive(Debug)]
pub struct VncParams {
    pub endpoint: Endpoint,
    pub password: SecretString,
    pub connect_timeout: Duration,
}

/// How often the server is asked for screen changes.
const REFRESH_INTERVAL: Duration = Duration::from_millis(50);

/// True for addresses VNC may be used with: this computer, the local network
/// (private and link-local ranges) and Tailscale / carrier-grade NAT.
pub fn is_private_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // 100.64.0.0/10: Tailscale and carrier-grade NAT.
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_address(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                // fc00::/7 unique local (includes Tailscale's fd7a:115c:a1e0::/48).
                || (first & 0xfe00) == 0xfc00
                // fe80::/10 link-local.
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Resolves the endpoint and checks that every address it names is private.
fn private_target(endpoint: &Endpoint) -> Result<SocketAddr, ConnectionError> {
    let addrs: Vec<SocketAddr> = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| ConnectionError::HostNotFound)?
        .collect();
    let first = *addrs.first().ok_or(ConnectionError::HostNotFound)?;
    if addrs.iter().all(|a| is_private_address(a.ip())) {
        Ok(first)
    } else {
        Err(ConnectionError::VncNotPrivate)
    }
}

/// From the network thread to the session.
enum Msg {
    Resize(u16, u16),
    Pixels {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        data: Vec<u8>,
    },
    Copy {
        dst: (u16, u16),
        src: (u16, u16),
        w: u16,
        h: u16,
    },
    Ended(Option<ConnectionError>),
}

/// An open VNC connection. Call [`VncSession::poll`] regularly.
pub struct VncSession {
    width: u16,
    height: u16,
    image: Vec<u8>,
    rx: mpsc::Receiver<Msg>,
    input: tokio::sync::mpsc::UnboundedSender<X11Event>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    ended: bool,
    pointer: (u16, u16),
    buttons: u8,
    held_keys: HashSet<u32>,
}

/// Opens a VNC connection. Refuses addresses outside the own network or
/// Tailscale before connecting.
pub fn connect_vnc(params: VncParams) -> Result<VncSession, ConnectionError> {
    let addr = private_target(&params.endpoint)?;
    let timeout = params.connect_timeout;
    let password = params.password;
    let (msg_tx, rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), ConnectionError>>(1);
    let (input, input_rx) = tokio::sync::mpsc::unbounded_channel::<X11Event>();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ConnectionError::Unexpected)?;
    let thread = std::thread::Builder::new()
        .name("vnc-session".into())
        .spawn(move || {
            runtime.block_on(network(
                addr, timeout, password, msg_tx, ready_tx, input_rx, stopped,
            ));
        })
        .map_err(|_| ConnectionError::Unexpected)?;

    let result = ready_rx
        .recv_timeout(timeout + Duration::from_secs(5))
        .unwrap_or(Err(ConnectionError::Timeout));
    let mut session = VncSession {
        width: 0,
        height: 0,
        image: Vec::new(),
        rx,
        input,
        stop: Some(stop),
        thread: Some(thread),
        ended: false,
        pointer: (0, 0),
        buttons: 0,
        held_keys: HashSet::new(),
    };
    if let Err(e) = result {
        session.shut_down();
        return Err(e);
    }
    // The first message is always the screen size.
    let started = Instant::now();
    while session.width == 0 && started.elapsed() < timeout {
        let _ = session.poll();
        if session.ended {
            session.shut_down();
            return Err(ConnectionError::ConnectionLost);
        }
    }
    if session.width == 0 {
        session.shut_down();
        return Err(ConnectionError::Timeout);
    }
    Ok(session)
}

fn classify(e: &vnc::VncError) -> ConnectionError {
    use vnc::VncError as V;
    match e {
        V::WrongPassword | V::NoPassword => ConnectionError::WrongCredentials,
        V::InvalidSecurityTyep(_) => ConnectionError::SecurityNotSupported,
        V::IoError(io) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused => ConnectionError::HostRefused,
            std::io::ErrorKind::TimedOut => ConnectionError::Timeout,
            _ => ConnectionError::ConnectionLost,
        },
        // The server refused with its own reason (e.g. too many attempts).
        V::General(_) | V::ConnectError => ConnectionError::HostRefused,
        _ => ConnectionError::Unexpected,
    }
}

async fn network(
    addr: SocketAddr,
    timeout: Duration,
    password: SecretString,
    tx: mpsc::Sender<Msg>,
    ready: mpsc::SyncSender<Result<(), ConnectionError>>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<X11Event>,
    mut stopped: tokio::sync::oneshot::Receiver<()>,
) {
    let fail = |e: ConnectionError| {
        let _ = ready.send(Err(e));
    };
    let tcp = match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return fail(classify(&vnc::VncError::IoError(e))),
        Err(_) => return fail(ConnectionError::Timeout),
    };
    let _ = tcp.set_nodelay(true);
    let secret = password.expose().to_owned();
    drop(password);
    // Set once the server has asked for the password: a refusal after that
    // means the password was wrong (VNC 3.8 servers send a reason text).
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let asked_in_auth = std::sync::Arc::clone(&asked);
    let connector = VncConnector::new(tcp)
        .set_auth_method(async move {
            asked_in_auth.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(secret)
        })
        .add_encoding(VncEncoding::Zrle)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .allow_shared(true)
        .set_pixel_format(PixelFormat::rgba());
    let started = async move { connector.build()?.try_start().await?.finish() };
    let client = match tokio::time::timeout(timeout, started).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::info!(error = %e, "VNC connection failed");
            let err = match &e {
                vnc::VncError::General(_) if asked.load(std::sync::atomic::Ordering::SeqCst) => {
                    ConnectionError::WrongCredentials
                }
                // Only "None" and "VNC password" sign-in are supported.
                vnc::VncError::General(m) if m.starts_with("Security type") => {
                    ConnectionError::SecurityNotSupported
                }
                _ => classify(&e),
            };
            return fail(err);
        }
        Err(_) => return fail(ConnectionError::Timeout),
    };
    if ready.send(Ok(())).is_err() {
        let _ = client.close().await;
        return;
    }
    let _ = client.input(X11Event::FullRefresh).await;
    let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
    let end = loop {
        tokio::select! {
            _ = &mut stopped => break None,
            ev = client.recv_event() => match ev {
                Ok(ev) => {
                    let msg = match ev {
                        VncEvent::SetResolution(s) => Some(Msg::Resize(s.width, s.height)),
                        VncEvent::RawImage(r, data) => Some(Msg::Pixels {
                            x: r.x, y: r.y, w: r.width, h: r.height, data,
                        }),
                        VncEvent::Copy(dst, src) => Some(Msg::Copy {
                            dst: (dst.x, dst.y), src: (src.x, src.y), w: dst.width, h: dst.height,
                        }),
                        VncEvent::Error(e) => {
                            tracing::info!(error = %e, "VNC session error");
                            break Some(ConnectionError::ConnectionLost);
                        }
                        // Clipboard, bell, cursor shape and others are not used.
                        _ => None,
                    };
                    if let Some(m) = msg {
                        if tx.send(m).is_err() {
                            break None;
                        }
                    }
                }
                Err(e) => break Some(match classify(&e) {
                    ConnectionError::HostRefused => ConnectionError::DisconnectedByHost,
                    other => other,
                }),
            },
            Some(event) = input_rx.recv() => {
                if client.input(event).await.is_err() {
                    break Some(ConnectionError::ConnectionLost);
                }
            }
            _ = refresh.tick() => {
                if client.input(X11Event::Refresh).await.is_err() {
                    break Some(ConnectionError::ConnectionLost);
                }
            }
        }
    };
    let _ = client.close().await;
    let _ = tx.send(Msg::Ended(end));
}

impl VncSession {
    /// Current desktop size.
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// The whole remote screen as RGBA pixels, row by row.
    pub fn frame(&self) -> &[u8] {
        &self.image
    }

    /// `r` limited to the desktop, or `None` if nothing of it is on screen.
    pub fn clamped(&self, r: Region) -> Option<Region> {
        let (w, h) = self.size();
        if w == 0 || h == 0 {
            return None;
        }
        let c = Region {
            left: r.left.min(w - 1),
            right: r.right.min(w - 1),
            top: r.top.min(h - 1),
            bottom: r.bottom.min(h - 1),
        };
        (c.right >= c.left && c.bottom >= c.top).then_some(c)
    }

    /// RGBA pixels of `r` (clamped to the desktop), row by row.
    pub fn region_pixels(&self, r: Region) -> Vec<u8> {
        let Some(r) = self.clamped(r) else {
            return Vec::new();
        };
        let stride = usize::from(self.width) * 4;
        let (left, right) = (usize::from(r.left), usize::from(r.right));
        let mut out =
            Vec::with_capacity((right - left + 1) * usize::from(r.bottom - r.top + 1) * 4);
        for y in usize::from(r.top)..=usize::from(r.bottom) {
            if let Some(row) = self
                .image
                .get(y * stride + left * 4..y * stride + (right + 1) * 4)
            {
                out.extend_from_slice(row);
            }
        }
        out
    }

    /// Applies what the server sent. Waits up to [`POLL_INTERVAL`] for
    /// something to arrive and works at most [`POLL_BUDGET`].
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        if self.ended {
            return events;
        }
        let started = Instant::now();
        let mut first = true;
        while started.elapsed() < POLL_BUDGET {
            let msg = if first {
                first = false;
                self.rx
                    .recv_timeout(POLL_INTERVAL)
                    .map_err(|e| matches!(e, mpsc::RecvTimeoutError::Disconnected))
            } else {
                self.rx
                    .try_recv()
                    .map_err(|e| matches!(e, mpsc::TryRecvError::Disconnected))
            };
            match msg {
                Ok(m) => self.apply(m, &mut events),
                Err(true) => self.end(&mut events, Some(ConnectionError::ConnectionLost)),
                Err(false) => break,
            }
            if self.ended {
                break;
            }
        }
        events
    }

    fn apply(&mut self, msg: Msg, events: &mut Vec<SessionEvent>) {
        match msg {
            Msg::Resize(w, h) => {
                if (w, h) != (self.width, self.height) {
                    self.width = w;
                    self.height = h;
                    self.image = vec![0; usize::from(w) * usize::from(h) * 4];
                    // Opaque black until the first picture arrives.
                    for px in self.image.chunks_exact_mut(4) {
                        px[3] = 0xFF;
                    }
                    events.push(SessionEvent::Resized {
                        width: w,
                        height: h,
                    });
                }
            }
            Msg::Pixels { x, y, w, h, data } => {
                if let Some(r) = self.paste(x, y, w, h, &data) {
                    events.push(SessionEvent::Updated(r));
                }
            }
            Msg::Copy { dst, src, w, h } => {
                if let Some(r) = self.copy_rect(dst, src, w, h) {
                    events.push(SessionEvent::Updated(r));
                }
            }
            Msg::Ended(err) => self.end(events, err),
        }
    }

    /// Writes server pixels (RGBX) into the picture; returns the changed area.
    fn paste(&mut self, x: u16, y: u16, w: u16, h: u16, data: &[u8]) -> Option<Region> {
        let (fw, fh) = (usize::from(self.width), usize::from(self.height));
        let (x, y, w, h) = (
            usize::from(x),
            usize::from(y),
            usize::from(w),
            usize::from(h),
        );
        if w == 0 || h == 0 || x >= fw || y >= fh || data.len() < w * h * 4 {
            return None;
        }
        let cw = w.min(fw - x);
        let ch = h.min(fh - y);
        for row in 0..ch {
            let src = &data[row * w * 4..(row * w + cw) * 4];
            let start = ((y + row) * fw + x) * 4;
            let dst = &mut self.image[start..start + cw * 4];
            dst.copy_from_slice(src);
            // The fourth byte is padding in VNC; make every pixel opaque.
            for px in dst.chunks_exact_mut(4) {
                px[3] = 0xFF;
            }
        }
        region(x, y, cw, ch)
    }

    fn copy_rect(&mut self, dst: (u16, u16), src: (u16, u16), w: u16, h: u16) -> Option<Region> {
        let fw = usize::from(self.width);
        let fh = usize::from(self.height);
        let (dx, dy) = (usize::from(dst.0), usize::from(dst.1));
        let (sx, sy) = (usize::from(src.0), usize::from(src.1));
        let (w, h) = (usize::from(w), usize::from(h));
        if w == 0 || h == 0 || dx >= fw || dy >= fh || sx >= fw || sy >= fh {
            return None;
        }
        let cw = w.min(fw - dx).min(fw - sx);
        let ch = h.min(fh - dy).min(fh - sy);
        let rows: Vec<usize> = if dy > sy {
            (0..ch).rev().collect()
        } else {
            (0..ch).collect()
        };
        for row in rows {
            let s = ((sy + row) * fw + sx) * 4;
            let d = ((dy + row) * fw + dx) * 4;
            self.image.copy_within(s..s + cw * 4, d);
        }
        region(dx, dy, cw, ch)
    }

    /// True once the session has ended.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Sends keyboard and mouse input. Keys are PC scancodes, translated to
    /// VNC key codes for a US keyboard layout; text uses its characters.
    pub fn send_input(&mut self, ops: Vec<InputEvent>) -> Result<(), ConnectionError> {
        if self.ended {
            return Err(ConnectionError::ConnectionLost);
        }
        for op in ops {
            for event in self.translate(op) {
                self.input
                    .send(event)
                    .map_err(|_| ConnectionError::ConnectionLost)?;
            }
        }
        Ok(())
    }

    fn pointer_event(&self, buttons: u8) -> X11Event {
        X11Event::PointerEvent(ClientMouseEvent {
            position_x: self.pointer.0,
            position_y: self.pointer.1,
            bottons: buttons,
        })
    }

    fn key_event(&mut self, keysym: u32, down: bool) -> X11Event {
        if down {
            self.held_keys.insert(keysym);
        } else {
            self.held_keys.remove(&keysym);
        }
        X11Event::KeyEvent(ClientKeyEvent {
            keycode: keysym,
            down,
        })
    }

    fn translate(&mut self, op: InputEvent) -> Vec<X11Event> {
        let bit = |b: PointerButton| match b {
            PointerButton::Left => 1u8,
            PointerButton::Middle => 2,
            PointerButton::Right => 4,
        };
        match op {
            InputEvent::MouseMove { x, y } => {
                self.pointer = (
                    x.min(self.width.saturating_sub(1)),
                    y.min(self.height.saturating_sub(1)),
                );
                vec![self.pointer_event(self.buttons)]
            }
            InputEvent::MouseDown(b) => {
                self.buttons |= bit(b);
                vec![self.pointer_event(self.buttons)]
            }
            InputEvent::MouseUp(b) => {
                self.buttons &= !bit(b);
                vec![self.pointer_event(self.buttons)]
            }
            InputEvent::Wheel { vertical, delta } => {
                if delta == 0 {
                    return Vec::new();
                }
                // Buttons 4/5 scroll up/down, 6/7 left/right; one press per notch.
                let wheel = match (vertical, delta > 0) {
                    (true, true) => 8u8,
                    (true, false) => 16,
                    (false, true) => 64,
                    (false, false) => 32,
                };
                let notches = (delta.unsigned_abs() / 120).clamp(1, 10);
                let mut out = Vec::new();
                for _ in 0..notches {
                    out.push(self.pointer_event(self.buttons | wheel));
                    out.push(self.pointer_event(self.buttons));
                }
                out
            }
            InputEvent::KeyDown { scancode, extended } => keysym_for_scancode(scancode, extended)
                .map(|k| vec![self.key_event(k, true)])
                .unwrap_or_default(),
            InputEvent::KeyUp { scancode, extended } => keysym_for_scancode(scancode, extended)
                .map(|k| vec![self.key_event(k, false)])
                .unwrap_or_default(),
            InputEvent::Unicode { ch, down } => vec![self.key_event(keysym_for_char(ch), down)],
        }
    }

    /// Ends the connection, releasing held keys and buttons first.
    pub fn disconnect(mut self) {
        if !self.ended {
            let keys: Vec<u32> = self.held_keys.drain().collect();
            for k in keys {
                let _ = self.input.send(X11Event::KeyEvent(ClientKeyEvent {
                    keycode: k,
                    down: false,
                }));
            }
            if self.buttons != 0 {
                self.buttons = 0;
                let _ = self.input.send(self.pointer_event(0));
            }
            self.ended = true;
        }
        self.shut_down();
    }

    fn shut_down(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn end(&mut self, events: &mut Vec<SessionEvent>, err: Option<ConnectionError>) {
        if !self.ended {
            self.ended = true;
            events.push(SessionEvent::Ended(err));
        }
    }
}

impl Drop for VncSession {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn region(x: usize, y: usize, w: usize, h: usize) -> Option<Region> {
    Some(Region {
        left: u16::try_from(x).ok()?,
        top: u16::try_from(y).ok()?,
        right: u16::try_from(x + w - 1).ok()?,
        bottom: u16::try_from(y + h - 1).ok()?,
    })
}

/// X11 key code for a typed character.
pub fn keysym_for_char(ch: char) -> u32 {
    match ch {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        '\u{8}' => 0xff08,
        c if (' '..='~').contains(&c) || ('\u{a0}'..='\u{ff}').contains(&c) => u32::from(c),
        c => 0x0100_0000 | u32::from(c),
    }
}

/// X11 key code for a PC (set 1) scancode, US layout.
pub fn keysym_for_scancode(scancode: u8, extended: bool) -> Option<u32> {
    if extended {
        return Some(match scancode {
            0x1C => 0xff8d, // keypad Enter
            0x1D => 0xffe4, // right Ctrl
            0x35 => 0xffaf, // keypad /
            0x37 => 0xff61, // Print Screen
            0x38 => 0xffea, // right Alt
            0x47 => 0xff50, // Home
            0x48 => 0xff52, // Up
            0x49 => 0xff55, // Page Up
            0x4B => 0xff51, // Left
            0x4D => 0xff53, // Right
            0x4F => 0xff57, // End
            0x50 => 0xff54, // Down
            0x51 => 0xff56, // Page Down
            0x52 => 0xff63, // Insert
            0x53 => 0xffff, // Delete
            0x5B => 0xffeb, // left Windows
            0x5C => 0xffec, // right Windows
            0x5D => 0xff67, // Menu
            _ => return None,
        });
    }
    const LETTERS_1: &[u8] = b"qwertyuiop";
    const LETTERS_2: &[u8] = b"asdfghjkl";
    const LETTERS_3: &[u8] = b"zxcvbnm";
    const DIGITS: &[u8] = b"1234567890";
    let at = |table: &[u8], start: u8| {
        table
            .get(usize::from(scancode - start))
            .map(|&c| u32::from(c))
    };
    Some(match scancode {
        0x01 => 0xff1b,
        0x02..=0x0B => return at(DIGITS, 0x02),
        0x0C => u32::from(b'-'),
        0x0D => u32::from(b'='),
        0x0E => 0xff08,
        0x0F => 0xff09,
        0x10..=0x19 => return at(LETTERS_1, 0x10),
        0x1A => u32::from(b'['),
        0x1B => u32::from(b']'),
        0x1C => 0xff0d,
        0x1D => 0xffe3,
        0x1E..=0x26 => return at(LETTERS_2, 0x1E),
        0x27 => u32::from(b';'),
        0x28 => u32::from(b'\''),
        0x29 => u32::from(b'`'),
        0x2A => 0xffe1,
        0x2B => u32::from(b'\\'),
        0x2C..=0x32 => return at(LETTERS_3, 0x2C),
        0x33 => u32::from(b','),
        0x34 => u32::from(b'.'),
        0x35 => u32::from(b'/'),
        0x36 => 0xffe2,
        0x37 => 0xffaa,
        0x38 => 0xffe9,
        0x39 => u32::from(b' '),
        0x3A => 0xffe5,
        0x3B..=0x44 => 0xffbe + u32::from(scancode - 0x3B), // F1–F10
        0x45 => 0xff7f,
        0x46 => 0xff14,
        0x47 => 0xffb7,
        0x48 => 0xffb8,
        0x49 => 0xffb9,
        0x4A => 0xffad,
        0x4B => 0xffb4,
        0x4C => 0xffb5,
        0x4D => 0xffb6,
        0x4E => 0xffab,
        0x4F => 0xffb1,
        0x50 => 0xffb2,
        0x51 => 0xffb3,
        0x52 => 0xffb0,
        0x53 => 0xffae,
        0x57 => 0xffc8, // F11
        0x58 => 0xffc9, // F12
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_own_network_and_tailscale_addresses_are_private() {
        for ok in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "172.31.255.1",
            "192.168.1.20",
            "169.254.3.4",
            "100.64.0.1",
            "100.115.244.123",
            "100.127.255.254",
            "::1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(
                is_private_address(ok.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))),
                "{ok}"
            );
        }
        for bad in [
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "192.0.2.1",
            "2001:db8::1",
            "::ffff:8.8.8.8",
            "0.0.0.0",
        ] {
            assert!(
                !is_private_address(bad.parse().unwrap_or(IpAddr::from([8, 8, 8, 8]))),
                "{bad}"
            );
        }
    }

    #[test]
    fn public_addresses_are_refused_before_connecting() {
        let endpoint = crate::input::parse_endpoint("192.0.2.1", "5900")
            .map_err(|_| ())
            .ok();
        let Some(endpoint) = endpoint else {
            panic!("endpoint");
        };
        let r = connect_vnc(VncParams {
            endpoint,
            password: SecretString::new("x".into()),
            connect_timeout: Duration::from_secs(1),
        });
        assert!(matches!(r, Err(ConnectionError::VncNotPrivate)));
    }

    #[test]
    fn keys_map_to_x11_codes() {
        assert_eq!(keysym_for_scancode(0x1E, false), Some(u32::from(b'a')));
        assert_eq!(keysym_for_scancode(0x02, false), Some(u32::from(b'1')));
        assert_eq!(keysym_for_scancode(0x0B, false), Some(u32::from(b'0')));
        assert_eq!(keysym_for_scancode(0x32, false), Some(u32::from(b'm')));
        assert_eq!(keysym_for_scancode(0x3B, false), Some(0xffbe));
        assert_eq!(keysym_for_scancode(0x44, false), Some(0xffc7));
        assert_eq!(keysym_for_scancode(0x48, true), Some(0xff52));
        assert_eq!(keysym_for_scancode(0x53, true), Some(0xffff));
        assert_eq!(keysym_for_scancode(0x7F, false), None);
        assert_eq!(keysym_for_char('A'), 0x41);
        assert_eq!(keysym_for_char('é'), 0xe9);
        assert_eq!(keysym_for_char('€'), 0x0100_20ac);
        assert_eq!(keysym_for_char('\n'), 0xff0d);
    }
}
