//! Cipher Desk Host: shares this computer's own screen (the "Share screen"
//! connection method).
//!
//! Unlike Windows Remote Desktop, the screen stays visible and usable at the
//! computer while a viewer watches and controls it. The protection is the same
//! as for Remote Desktop connections:
//!
//! - TLS with the Host's own certificate; the viewer checks and pins its
//!   fingerprint on first connection.
//! - Network Level Authentication (CredSSP/NTLM) with a sharing username and
//!   password chosen on the "Share this screen" page (kept in the OS vault).
//! - After 5 failed sign-ins in 10 minutes, new connections are refused for
//!   15 minutes.
//! - One viewer at a time; the UI shows a notice while someone is connected.
//!
//! Screen capture and input are platform parts ([`FrameSource`], [`InputSink`]);
//! Windows has real ones, tests use synthetic ones.

pub mod frames;
pub mod identity;
pub mod input;
mod limiter;
#[cfg(windows)]
pub mod win;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use async_trait::async_trait;
use ironrdp_server::{
    Credentials, DesktopSize, DisplayUpdate, RdpServer, RdpServerDisplay, RdpServerDisplayUpdates,
};
use rdpv_core::secret::SecretString;

pub use frames::{Frame, FrameSource, SyntheticSource};
pub use input::{Button, Input, InputSink, NoInput, RecordingSink};

/// How often the screen is checked for changes while a viewer is connected.
const CAPTURE_INTERVAL: Duration = Duration::from_millis(100);

/// What the Host reports to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// Listening for viewers on this port.
    Listening { port: u16 },
    /// A viewer signed in and is now seeing the screen.
    ViewerConnected { address: String },
    /// The viewer left (or the connection ended).
    ViewerDisconnected,
    /// Too many wrong passwords: new connections are refused for a while.
    LockedOut { minutes: u64 },
    /// The Host could not start or stopped with an error.
    Failed { message: String },
}

/// Settings for one sharing run.
pub struct HostConfig {
    pub port: u16,
    pub username: String,
    pub password: SecretString,
    /// Folder for the Host certificate (the app's private data folder).
    pub data_dir: PathBuf,
}

/// A running Host. Dropping it (or calling [`HostHandle::stop`]) stops sharing.
pub struct HostHandle {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HostHandle {
    /// Stops sharing at once and closes any viewer's connection.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for HostHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A new random sharing password: 4 groups of 4 letters and digits, easy to
/// type on a phone (about 92 bits; look-alike characters left out).
pub fn generate_password() -> anyhow::Result<SecretString> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let rng = ring::rand::SystemRandom::new();
    let mut out = String::with_capacity(19);
    let mut i = 0;
    while i < 16 {
        let mut b = [0u8; 1];
        ring::rand::SecureRandom::fill(&rng, &mut b)
            .map_err(|_| anyhow::anyhow!("no randomness"))?;
        // Reject values that would make some characters more likely.
        let limit = 256 - 256 % ALPHABET.len();
        if usize::from(b[0]) >= limit {
            continue;
        }
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(char::from(ALPHABET[usize::from(b[0]) % ALPHABET.len()]));
        i += 1;
    }
    Ok(SecretString::new(out))
}

/// Fingerprint of this computer's Host certificate (created if needed), for the UI.
pub fn fingerprint(data_dir: &std::path::Path) -> anyhow::Result<String> {
    Ok(identity::load_or_create(data_dir)?.fingerprint())
}

/// Starts sharing: listens on all network interfaces (the firewall decides who
/// can reach it; the app tells users to use it on their own network or a VPN).
pub fn start<S, I>(
    config: HostConfig,
    source: S,
    sink: I,
    events: mpsc::Sender<HostEvent>,
) -> anyhow::Result<HostHandle>
where
    S: FrameSource + Send + 'static,
    I: InputSink + Send + 'static,
{
    start_on(config, Ipv4Addr::UNSPECIFIED.into(), source, sink, events)
}

/// Like [`start`], on a chosen address (tests use 127.0.0.1).
pub fn start_on<S, I>(
    config: HostConfig,
    ip: std::net::IpAddr,
    source: S,
    sink: I,
    events: mpsc::Sender<HostEvent>,
) -> anyhow::Result<HostHandle>
where
    S: FrameSource + Send + 'static,
    I: InputSink + Send + 'static,
{
    if config.username.trim().is_empty() || config.password.expose().is_empty() {
        anyhow::bail!("a sharing username and password are needed");
    }
    let identity = identity::load_or_create(&config.data_dir)?;
    let acceptor = identity.acceptor()?;

    // Bound here so a busy port is reported at once, and viewers can connect
    // as soon as "Listening" is sent.
    let listener = std::net::TcpListener::bind(SocketAddr::new(ip, config.port))
        .context("the sharing port is in use")?;
    listener.set_nonblocking(true).context("listener")?;
    let port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(config.port);

    let display = ScreenDisplay {
        source: Arc::new(Mutex::new(source)),
        events: events.clone(),
    };
    let username = config.username.trim().to_owned();
    let password = config.password;
    let limiter = limiter::Limiter::new(events.clone());
    // The server's session future is not `Send`, so it runs on its own thread
    // with a single-threaded runtime.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("runtime")?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let ev = events;
    let thread = std::thread::Builder::new()
        .name("cipher-desk-host".into())
        .spawn(move || {
            // Built here: some server parts are not `Send`.
            let mut server = RdpServer::builder()
                .with_addr(SocketAddr::new(ip, port))
                .with_hybrid(acceptor, identity.pub_key.clone())
                .with_input_handler(input::Handler { sink })
                .with_display_handler(display)
                .with_connection_handler(Some(Box::new(limiter)))
                .build();
            server.set_credentials(Some(Credentials {
                username,
                password: password.expose().to_owned(),
                domain: None,
            }));
            drop(password);
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!(error = %e, "listener");
                        let _ = ev.send(HostEvent::Failed {
                            message: "Sharing could not start.".into(),
                        });
                        return;
                    }
                };
                let _ = ev.send(HostEvent::Listening { port });
                tokio::select! {
                    result = server.serve(listener) => {
                        if let Err(e) = result {
                            tracing::warn!(error = %e, "Host stopped");
                            let _ = ev.send(HostEvent::Failed {
                                message: "Sharing stopped because of a network error.".into(),
                            });
                        }
                    }
                    _ = stopped => {}
                }
            });
        })
        .context("host thread")?;
    Ok(HostHandle {
        stop: Some(stop),
        thread: Some(thread),
    })
}

// ---------------------------------------------------------------- display

struct ScreenDisplay<S: FrameSource> {
    source: Arc<Mutex<S>>,
    events: mpsc::Sender<HostEvent>,
}

#[async_trait]
impl<S: FrameSource + Send + 'static> RdpServerDisplay for ScreenDisplay<S> {
    async fn size(&mut self) -> DesktopSize {
        let (width, height) = self
            .source
            .lock()
            .map(|mut s| s.size())
            .unwrap_or((1024, 768));
        DesktopSize { width, height }
    }

    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        // Called once a viewer has signed in and the session starts.
        let _ = self.events.send(HostEvent::ViewerConnected {
            address: String::new(),
        });
        let (tx, rx) = tokio::sync::mpsc::channel::<DisplayUpdate>(16);
        let source = Arc::clone(&self.source);
        let size = self.size().await;
        // Capture on a plain thread: screen capture is blocking work. It ends by
        // itself when the session ends (the receiver is dropped).
        std::thread::Builder::new()
            .name("cipher-desk-capture".into())
            .spawn(move || capture_loop(&source, &tx, size))
            .context("capture thread")?;
        Ok(Box::new(Updates { rx }))
    }
}

fn capture_loop<S: FrameSource + Send>(
    source: &Mutex<S>,
    tx: &tokio::sync::mpsc::Sender<DisplayUpdate>,
    mut size: DesktopSize,
) {
    let mut differ = frames::Differ::default();
    loop {
        if tx.is_closed() {
            return;
        }
        let frame = match source.lock() {
            Ok(mut s) => s.capture(),
            Err(_) => return,
        };
        match frame {
            Ok(frame) => {
                // The screen resolution changed: the viewer is told the new size
                // first, then gets the whole new picture.
                if (frame.width, frame.height) != (size.width, size.height) {
                    size = DesktopSize {
                        width: frame.width,
                        height: frame.height,
                    };
                    if tx.blocking_send(DisplayUpdate::Resize(size)).is_err() {
                        return;
                    }
                }
                for update in differ.changes(frame) {
                    if tx.blocking_send(DisplayUpdate::Bitmap(update)).is_err() {
                        return;
                    }
                }
            }
            // The screen can be briefly unavailable (lock screen, UAC prompt):
            // keep the last picture and try again.
            Err(e) => tracing::debug!(error = %e, "capture failed"),
        }
        std::thread::sleep(CAPTURE_INTERVAL);
    }
}

struct Updates {
    rx: tokio::sync::mpsc::Receiver<DisplayUpdate>,
}

#[async_trait]
impl RdpServerDisplayUpdates for Updates {
    // Cancel-safe: `recv` loses nothing when another branch wins a select.
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        Ok(self.rx.recv().await)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_passwords_are_long_and_different() {
        let a = super::generate_password().map(|p| p.expose().to_owned());
        let b = super::generate_password().map(|p| p.expose().to_owned());
        let (Ok(a), Ok(b)) = (a, b) else {
            panic!("no password");
        };
        assert_eq!(a.len(), 19);
        assert_eq!(a.matches('-').count(), 3);
        assert_ne!(a, b);
    }
}
