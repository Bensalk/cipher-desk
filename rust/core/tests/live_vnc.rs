//! VNC against a real VNC server. Ignored by default. CI starts x11vnc on a
//! virtual screen with a random password for each run:
//!
//! ```sh
//! RDPV_VNC_PORT=5900 RDPV_VNC_PASSWORD=<password> \
//! cargo test -p rdpv-core --test live_vnc -- --ignored --test-threads=1
//! ```

use std::time::{Duration, Instant};

use rdpv_core::input::parse_endpoint;
use rdpv_core::secret::SecretString;
use rdpv_core::session::{InputEvent, PointerButton, SessionEvent};
use rdpv_core::status::ConnectionError;
use rdpv_core::vnc::{connect_vnc, VncParams, VncSession};

fn params(password: &str) -> Result<VncParams, String> {
    let port = std::env::var("RDPV_VNC_PORT").unwrap_or_else(|_| "5900".into());
    Ok(VncParams {
        endpoint: parse_endpoint("127.0.0.1", &port).map_err(|e| format!("{e:?}"))?,
        password: SecretString::new(password.to_owned()),
        connect_timeout: Duration::from_secs(10),
    })
}

fn password() -> Result<String, String> {
    std::env::var("RDPV_VNC_PASSWORD").map_err(|_| "set RDPV_VNC_PASSWORD".to_owned())
}

fn wait_for_picture(s: &mut VncSession) -> Result<usize, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut updates = 0;
    while Instant::now() < deadline && updates == 0 {
        for e in s.poll() {
            match e {
                SessionEvent::Updated(_) => updates += 1,
                SessionEvent::Ended(err) => return Err(format!("ended: {err:?}")),
                _ => {}
            }
        }
    }
    Ok(updates)
}

#[test]
#[ignore = "needs a VNC server"]
fn vnc_shows_the_screen_and_takes_input() -> Result<(), String> {
    let mut s = connect_vnc(params(&password()?)?).map_err(|e| format!("connect: {e:?}"))?;
    let (w, h) = s.size();
    assert!(w >= 640 && h >= 480, "size {w}x{h}");
    assert!(wait_for_picture(&mut s)? > 0, "no picture");
    assert!(
        s.frame().chunks_exact(4).all(|p| p[3] == 0xFF),
        "pixels are opaque"
    );
    s.send_input(vec![
        InputEvent::MouseMove { x: 10, y: 10 },
        InputEvent::MouseDown(PointerButton::Left),
        InputEvent::MouseUp(PointerButton::Left),
        InputEvent::Wheel {
            vertical: true,
            delta: 120,
        },
        InputEvent::KeyDown {
            scancode: 0x1E,
            extended: false,
        },
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
    ])
    .map_err(|e| format!("input: {e:?}"))?;
    // Still connected after input.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        for e in s.poll() {
            if let SessionEvent::Ended(err) = e {
                return Err(format!("ended after input: {err:?}"));
            }
        }
    }
    s.disconnect();
    Ok(())
}

#[test]
#[ignore = "needs a VNC server"]
fn vnc_wrong_password_is_reported() -> Result<(), String> {
    // Every character changed: VNC only uses the first 8 characters, so
    // adding to the end would not make it wrong.
    let wrong: String = password()?
        .chars()
        .map(|c| if c == 'q' { 'r' } else { 'q' })
        .collect();
    match connect_vnc(params(&wrong)?) {
        Err(ConnectionError::WrongCredentials) => Ok(()),
        Err(other) => Err(format!("expected WrongCredentials, got {other:?}")),
        Ok(_) => Err("a wrong password was accepted".into()),
    }
}
