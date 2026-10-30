//! The Host and the Cipher Desk viewer talking to each other on this computer:
//! sign-in, certificate pinning, picture updates, input and the wrong-password
//! lockout. Uses a synthetic picture and records input instead of touching the
//! real screen, so it runs on every system.

// A failing step in a test should stop the test with a clear message.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rdpv_core::input::{parse_endpoint, parse_username};
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
    ConnectParams {
        endpoint: parse_endpoint("127.0.0.1", &port.to_string()).expect("endpoint"),
        username: parse_username(USER).expect("username"),
        password: SecretString::new(password.to_owned()),
        desktop_width: 800,
        desktop_height: 600,
        pinned_fingerprint: pin,
        connect_timeout: Duration::from_secs(10),
        clipboard: rdpv_core::clipboard::ClipboardPolicy::off(),
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
        },
        "127.0.0.1".parse().expect("ip"),
        SyntheticSource::new(640, 480),
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
        for e in s.poll() {
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
