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

#[cfg(windows)]
pub mod clipboard;
pub mod frames;
pub mod identity;
pub mod input;
mod limiter;
#[cfg(windows)]
pub mod win;
#[cfg(windows)]
mod win_desktop;
#[cfg(windows)]
pub mod winsecret;

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

/// Shortest time between screen grabs while a viewer is connected (about 30
/// frames a second). A grab also waits until the previous one has been taken
/// for sending, so a slow link lowers the frame rate instead of queueing old
/// pictures: the viewer always gets the newest screen.
const CAPTURE_INTERVAL: Duration = Duration::from_millis(33);

/// How often the capture thread checks whether the previous grab has gone out.
const SEND_WAIT_STEP: Duration = Duration::from_millis(2);

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
    /// A viewer has signed in and is waiting for the person at this computer to
    /// allow or refuse it (only when "ask before each connection" is on).
    /// Answer with [`HostHandle::approve`].
    ApprovalRequested,
    /// A file sent by the viewer finished arriving in the Received folder.
    FileReceived { name: String },
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
    /// When true, each signed-in viewer must be allowed from this computer
    /// before it sees the screen (attended mode). When false, a correct
    /// password is enough (unattended).
    pub require_approval: bool,
}

/// How long to wait for the person at the computer to allow a viewer before
/// giving up and refusing the connection.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(40);

/// A running Host. Dropping it (or calling [`HostHandle::stop`]) stops sharing.
pub struct HostHandle {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    approvals: tokio::sync::mpsc::UnboundedSender<bool>,
    #[cfg(windows)]
    clip: Option<clipboard::ClipHandle>,
}

impl HostHandle {
    /// Allows (`true`) or refuses (`false`) a viewer waiting after
    /// [`HostEvent::ApprovalRequested`]. Ignored if nothing is waiting.
    pub fn approve(&self, allow: bool) {
        let _ = self.approvals.send(allow);
    }

    /// Offers these files to the connected viewer, which downloads them. Does
    /// nothing if no viewer is connected or the clipboard channel is not ready.
    #[cfg(windows)]
    pub fn send_files(&self, paths: Vec<PathBuf>) {
        if let Some(c) = &self.clip {
            c.offer_files(paths);
        }
    }

    /// No file sending off Windows (only Windows hosts).
    #[cfg(not(windows))]
    pub fn send_files(&self, _paths: Vec<PathBuf>) {}

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

/// Largest clipboard text accepted from the viewer, in bytes of UTF-16 (1 MiB).
#[cfg(windows)]
const CLIPBOARD_MAX_BYTES: usize = 1024 * 1024;

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

    // Approval channel: the UI answers ApprovalRequested through HostHandle.
    let (approve_tx, approve_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    let display = ScreenDisplay {
        source: Arc::new(Mutex::new(source)),
        events: events.clone(),
        require_approval: config.require_approval,
        approvals: Arc::new(tokio::sync::Mutex::new(approve_rx)),
    };
    let username = config.username.trim().to_owned();
    let password = config.password;
    let limiter = limiter::Limiter::new(events.clone());

    // Text + file clipboard bridge (Windows only). Files the viewer sends land
    // in a "Received" folder beside the app's data; `clip` lets the app offer
    // files to the viewer through [`HostHandle::send_files`]. The concrete
    // factory (which is `Send`) moves into the host thread and is boxed there,
    // since the boxed trait object itself is not `Send`.
    #[cfg(windows)]
    let host_clip = clipboard::HostClipboard::new(
        events.clone(),
        config.data_dir.join("Received"),
        CLIPBOARD_MAX_BYTES,
    );
    #[cfg(windows)]
    let clip = Some(host_clip.handle());

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
            let builder = RdpServer::builder()
                .with_addr(SocketAddr::new(ip, port))
                .with_hybrid(acceptor, identity.pub_key.clone())
                .with_input_handler(input::Handler { sink })
                .with_display_handler(display)
                .with_connection_handler(Some(Box::new(limiter)));
            // Clipboard and file transfer are Windows-only (only Windows hosts).
            #[cfg(windows)]
            let builder = builder.with_cliprdr_factory(Some(Box::new(host_clip)));
            let mut server = builder.build();
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
        approvals: approve_tx,
        #[cfg(windows)]
        clip,
    })
}

// ---------------------------------------------------------------- display

struct ScreenDisplay<S: FrameSource> {
    source: Arc<Mutex<S>>,
    events: mpsc::Sender<HostEvent>,
    require_approval: bool,
    approvals: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<bool>>>,
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
        // Called once a viewer has signed in and the session starts. In
        // attended mode, ask the person at this computer before showing the
        // screen; a wrong answer or no answer refuses this viewer (the host
        // keeps listening for the next).
        if self.require_approval {
            // Drop any stale answer from an earlier prompt.
            {
                let mut rx = self.approvals.lock().await;
                while rx.try_recv().is_ok() {}
            }
            let _ = self.events.send(HostEvent::ApprovalRequested);
            let allowed = {
                let mut rx = self.approvals.lock().await;
                matches!(
                    tokio::time::timeout(APPROVAL_TIMEOUT, rx.recv()).await,
                    Ok(Some(true))
                )
            };
            if !allowed {
                let _ = self.events.send(HostEvent::ViewerDisconnected);
                anyhow::bail!("viewer was not allowed");
            }
        }
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
        // Fresh pictures only: wait until everything from the last grab has
        // been taken for sending (the encoder takes the next update only after
        // the previous one is written to the network).
        while tx.capacity() < tx.max_capacity() {
            if tx.is_closed() {
                return;
            }
            std::thread::sleep(SEND_WAIT_STEP);
        }
        if tx.is_closed() {
            return;
        }
        let started = std::time::Instant::now();
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
        // Grabbing takes time too; only wait for what is left of the interval.
        if let Some(rest) = CAPTURE_INTERVAL.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
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
