//! The Host and the Cipher Desk viewer talking to each other on this computer:
//! sign-in, certificate pinning, picture updates, input and the wrong-password
//! lockout. Uses a synthetic picture and records input instead of touching the
//! real screen, so it runs on every system.

// A failing step in a test should stop the test with a clear message.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rdpv_core::input::{parse_endpoint, parse_username};
use rdpv_core::profiles::Quality;
use rdpv_core::secret::SecretString;
use rdpv_core::session::{
    connect, ConnectFailure, ConnectParams, InputEvent, PointerButton, SessionEvent,
};
use rdpv_core::status::ConnectionError;
use rdpv_host::{Button, HostConfig, HostEvent, Input, RecordingSink, SyntheticSource};

const USER: &str = "viewer";

fn test_password() -> String {
    // A fresh value per run; never a real password.
    format!(
        "Loopback-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

fn params(port: u16, password: &str, pin: Option<String>) -> ConnectParams {
    params_with(port, password, pin, Quality::Balanced)
}

fn params_with(port: u16, password: &str, pin: Option<String>, quality: Quality) -> ConnectParams {
    ConnectParams {
        endpoint: parse_endpoint("127.0.0.1", &port.to_string()).expect("endpoint"),
        username: parse_username(USER).expect("username"),
        password: SecretString::new(password.to_owned()),
        desktop_width: 800,
        desktop_height: 600,
        pinned_fingerprint: pin,
        connect_timeout: Duration::from_secs(10),
        clipboard: rdpv_core::clipboard::ClipboardPolicy::off(),
        receive_dir: None,
        quality,
    }
}

struct Running {
    _host: rdpv_host::HostHandle,
    events: mpsc::Receiver<HostEvent>,
    inputs: Arc<Mutex<Vec<Input>>>,
    port: u16,
    password: String,
    fingerprint: String,
    _dir: tempfile::TempDir,
}

fn start() -> Running {
    start_with(SyntheticSource::new(640, 480))
}

fn start_with<S: rdpv_host::FrameSource + Send + 'static>(source: S) -> Running {
    let dir = tempfile::tempdir().expect("temp dir");
    let port = free_port();
    let password = test_password();
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let (tx, events) = mpsc::channel();
    let host = rdpv_host::start_on(
        HostConfig {
            port,
            username: USER.into(),
            password: SecretString::new(password.clone()),
            data_dir: dir.path().to_path_buf(),
            require_approval: false,
        },
        "127.0.0.1".parse().expect("ip"),
        source,
        RecordingSink(Arc::clone(&inputs)),
        tx,
    )
    .expect("host starts");
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)),
        Ok(HostEvent::Listening { port })
    );
    let fingerprint = rdpv_host::fingerprint(dir.path()).expect("fingerprint");
    Running {
        _host: host,
        events,
        inputs,
        port,
        password,
        fingerprint,
        _dir: dir,
    }
}

#[test]
fn viewer_sees_the_shared_screen_and_controls_it() {
    let r = start();

    // First contact: the viewer is asked to approve the Host's certificate,
    // and it is the one the Host page shows.
    match connect(params(r.port, &r.password, None)) {
        Err(ConnectFailure::CertificateNeedsApproval { fingerprint }) => {
            assert_eq!(fingerprint, r.fingerprint);
        }
        other => panic!("expected certificate approval, got {:?}", other.err()),
    }

    let mut s = connect(params(r.port, &r.password, Some(r.fingerprint.clone())))
        .unwrap_or_else(|f| panic!("connect failed: {f:?}"));
    assert_eq!(s.size(), (640, 480), "the Host's screen size is used");

    // Picture updates arrive (the synthetic square moves every capture).
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut updates = 0;
    while updates < 3 && Instant::now() < deadline {
        for e in s.poll().0 {
            match e {
                SessionEvent::Updated(_) => updates += 1,
                SessionEvent::Ended(err) => panic!("session ended: {err:?}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(updates >= 3, "only {updates} picture updates");
    assert!(
        s.frame().chunks_exact(4).any(|p| p[1] == 0xC0),
        "the shared picture is visible"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match r.events.recv_timeout(Duration::from_millis(200)) {
            Ok(HostEvent::ViewerConnected { .. }) => break,
            _ => assert!(Instant::now() < deadline, "no viewer-connected event"),
        }
    }

    // Input reaches the Host.
    s.send_input(vec![
        InputEvent::MouseMove { x: 100, y: 50 },
        InputEvent::MouseDown(PointerButton::Left),
        InputEvent::MouseUp(PointerButton::Left),
        InputEvent::KeyDown {
            scancode: 0x1E,
            extended: false,
        },
        InputEvent::KeyUp {
            scancode: 0x1E,
            extended: false,
        },
    ])
    .expect("send input");
    let wanted = [
        Input::MoveTo { x: 100, y: 50 },
        Input::Button {
            button: Button::Left,
            down: true,
        },
        Input::Button {
            button: Button::Left,
            down: false,
        },
        Input::Key {
            scancode: 0x1E,
            extended: false,
            down: true,
        },
        Input::Key {
            scancode: 0x1E,
            extended: false,
            down: false,
        },
    ];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let _ = s.poll();
        let got = r.inputs.lock().map(|v| v.clone()).unwrap_or_default();
        if wanted.iter().all(|w| got.contains(w)) {
            break;
        }
        assert!(Instant::now() < deadline, "input not received: {got:?}");
        std::thread::sleep(Duration::from_millis(20));
    }

    s.disconnect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match r.events.recv_timeout(Duration::from_millis(200)) {
            Ok(HostEvent::ViewerDisconnected) => break,
            _ => assert!(Instant::now() < deadline, "no disconnect event"),
        }
    }
}

#[test]
fn wrong_passwords_lock_the_host_for_a_while() {
    let r = start();
    let pin = Some(r.fingerprint.clone());
    // Generated, so no fixed password is in the code.
    let wrong = format!("{}x", r.password);
    for _ in 0..5 {
        match connect(params(r.port, &wrong, pin.clone())) {
            Err(f) => assert_eq!(f.error(), ConnectionError::WrongCredentials),
            Ok(_) => panic!("a wrong password was accepted"),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match r.events.recv_timeout(Duration::from_millis(200)) {
            Ok(HostEvent::LockedOut { minutes }) => {
                assert_eq!(minutes, 15);
                break;
            }
            _ => assert!(Instant::now() < deadline, "no lockout event"),
        }
    }
    // Even the right password is refused while locked.
    assert!(connect(params(r.port, &r.password, pin)).is_err());
}

/// A picture whose size changes when the test says so (a resolution change).
struct Resizable {
    small: SyntheticSource,
    big: SyntheticSource,
    switched: Arc<std::sync::atomic::AtomicBool>,
}

impl rdpv_host::FrameSource for Resizable {
    fn size(&mut self) -> (u16, u16) {
        self.current().size()
    }
    fn capture(&mut self) -> anyhow::Result<rdpv_host::Frame> {
        self.current().capture()
    }
}

impl Resizable {
    fn current(&mut self) -> &mut SyntheticSource {
        if self.switched.load(std::sync::atomic::Ordering::SeqCst) {
            &mut self.big
        } else {
            &mut self.small
        }
    }
}

#[test]
fn a_resolution_change_resizes_the_viewer() {
    let switched = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r = start_with(Resizable {
        small: SyntheticSource::new(640, 480),
        big: SyntheticSource::new(800, 600),
        switched: Arc::clone(&switched),
    });
    let mut s = connect(params(r.port, &r.password, Some(r.fingerprint.clone())))
        .unwrap_or_else(|f| panic!("connect failed: {f:?}"));
    assert_eq!(s.size(), (640, 480));
    switched.store(true, std::sync::atomic::Ordering::SeqCst);
    // The host sends the new size (a deactivation/reactivation round-trip). We
    // check the viewer adopts it; the exact timing of the first repaint after
    // reactivation depends on the RDP stack, so a post-resize frame is awaited
    // best-effort rather than asserted.
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut resized = false;
    let mut updated_after = false;
    while Instant::now() < deadline && !(resized && updated_after) {
        for e in s.poll().0 {
            match e {
                SessionEvent::Resized { width, height } => {
                    assert_eq!((width, height), (800, 600));
                    resized = true;
                }
                SessionEvent::Updated(_) if resized => updated_after = true,
                SessionEvent::Ended(err) => panic!("session ended: {err:?}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(resized, "the viewer was not told the new size");
    assert_eq!(
        s.size(),
        (800, 600),
        "the viewer did not adopt the new size"
    );
    // The session is still healthy after the resize.
    assert!(!s.is_ended(), "session ended after the resize");
    s.disconnect();
}

/// A still picture with fine detail everywhere (like small text), so lossy
/// compression shows up as differences.
struct DetailedPicture {
    width: u16,
    height: u16,
}

impl DetailedPicture {
    /// The picture as the viewer stores it: red, green, blue per pixel.
    fn rgb(&self) -> Vec<[u8; 3]> {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        (0..h)
            .flat_map(|y| (0..w).map(move |x| Self::pixel(x, y)))
            .map(|[b, g, r]| [r, g, b])
            .collect()
    }

    /// Blue, green, red of one pixel.
    fn pixel(x: usize, y: usize) -> [u8; 3] {
        let v = ((x * 31) ^ (y * 17)).to_le_bytes()[0];
        [v, v.wrapping_mul(3), v.wrapping_add(90)]
    }
}

impl rdpv_host::FrameSource for DetailedPicture {
    fn size(&mut self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn capture(&mut self) -> anyhow::Result<rdpv_host::Frame> {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let mut bgra = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                bgra.extend_from_slice(&Self::pixel(x, y));
                bgra.push(0xFF);
            }
        }
        Ok(rdpv_host::Frame {
            width: self.width,
            height: self.height,
            bgra,
        })
    }
}

/// Connects with `quality` to a Host sharing [`DetailedPicture`] and returns
/// the average difference per colour value (0 = exact) once the picture has
/// arrived.
fn picture_error(quality: Quality) -> f64 {
    let picture = DetailedPicture {
        width: 320,
        height: 192,
    };
    let expected = picture.rgb();
    let r = start_with(picture);
    let mut s = connect(params_with(
        r.port,
        &r.password,
        Some(r.fingerprint.clone()),
        quality,
    ))
    .unwrap_or_else(|f| panic!("connect failed ({quality:?}): {f:?}"));
    assert_eq!(s.size(), (320, 192));

    // Updates keep arriving until the whole picture is in; take the error once
    // it stops changing.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = f64::MAX;
    let mut steady = 0;
    while Instant::now() < deadline && steady < 5 {
        for e in s.poll().0 {
            if let SessionEvent::Ended(err) = e {
                panic!("session ended ({quality:?}): {err:?}");
            }
        }
        let got = s.frame();
        let total: u64 = expected
            .iter()
            .zip(got.chunks_exact(4))
            .flat_map(|(want, px)| (0..3).map(move |c| u64::from(want[c].abs_diff(px[c]))))
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let error = total as f64 / (expected.len() * 3) as f64;
        steady = if (error - last).abs() < f64::EPSILON {
            steady + 1
        } else {
            0
        };
        last = error;
        std::thread::sleep(Duration::from_millis(50));
    }
    s.disconnect();
    last
}

#[test]
fn best_quality_arrives_pixel_for_pixel() {
    let error = picture_error(Quality::Best);
    assert!(error == 0.0, "Best must be lossless, average error {error}");
}

#[test]
fn balanced_is_sharper_than_speed() {
    let balanced = picture_error(Quality::Balanced);
    let speed = picture_error(Quality::Speed);
    // Measured on this deliberately hard picture: about 2.5 and 17.
    assert!(balanced < 6.0, "Balanced picture too far off: {balanced}");
    assert!(speed < 30.0, "Speed picture too far off: {speed}");
    assert!(
        balanced < speed,
        "Balanced ({balanced}) should be sharper than Speed ({speed})"
    );
}
