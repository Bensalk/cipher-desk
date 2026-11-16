//! The worker: runs the Share screen Host on the active session, following the
//! input desktop so the viewer also sees the sign-in and lock screens.
//!
//! It listens twice:
//!
//! - on the sharing port with the sharing password: the viewer gets straight
//!   in (unattended access to your own computers);
//! - only if turned on (off by default): on the next port
//!   ([`rdpv_host::request_port`]) with the public "ask to connect" sign-in:
//!   a popup asks the person at this computer to allow or refuse, refused
//!   after 30 seconds without an answer.
//!
//! Unless turned off, both accept connections only over Tailscale.
//!
//! Started by the service with [`crate::launch::spawn_worker`]; it runs until
//! the service ends it (on stop or when the session changes).

use std::sync::mpsc;
use std::time::Duration;

use anyhow::Context as _;
use rdpv_core::secret::SecretString;
use rdpv_host::win::{ServiceScreenCapture, ServiceSendInputSink};
use rdpv_host::{HostConfig, HostEvent, HostHandle};

/// Which of the two listeners an event came from.
#[derive(Clone, Copy, Debug)]
enum Door {
    Password,
    Ask,
}

impl Door {
    /// For the connection history.
    fn label(self) -> &'static str {
        match self {
            Door::Password => "with the sharing password",
            Door::Ask => "ask to connect",
        }
    }
}

/// "device-name (address)", or "address (no device name)".
fn who_is(address: &str) -> String {
    match crate::history::device_name(address) {
        Some(name) => format!("{name} ({address})"),
        None => format!("{address} (no device name)"),
    }
}

pub fn run() -> anyhow::Result<()> {
    // Real screen pixels at any Windows display scale (picture and clicks).
    rdpv_host::win::make_dpi_aware();
    let data_dir = crate::config::data_dir()?;
    // Never work relative to the system folder the service started in.
    std::env::set_current_dir(&data_dir).context("enter the service folder")?;
    let password =
        crate::config::load_password()?.context("no sharing password is set for the service")?;
    let settings = crate::config::load_settings();
    let (username, port) = (settings.username.clone(), settings.port);
    crate::history::ensure_exists();

    // The tray icon (running as the signed-in person) reads the files copied
    // here for us, and copied files are opened with that person's rights.
    let link = std::sync::Arc::new(crate::link::Link::default());
    crate::link::serve(std::sync::Arc::clone(&link));
    {
        let link = std::sync::Arc::clone(&link);
        rdpv_host::clipboard::set_copied_files_source(Box::new(move || link.copied_files()));
    }
    rdpv_host::clipboard::read_files_as_signed_in_user();

    let (tx, rx) = mpsc::channel::<(Door, HostEvent)>();
    let password_host = start(
        Door::Password,
        HostConfig {
            port,
            username,
            password,
            data_dir: data_dir.clone(),
            require_approval: false,
            // Two-step verification (read at each sign-in, so turning it on
            // or off applies at once).
            second_factor: Some(std::sync::Arc::new(rdpv_host::totp::Gate::with_source(
                Box::new(crate::config::load_twostep),
                crate::config::twostep_devices_file()?,
            ))),
            tailnet_only: settings.tailnet_only,
        },
        &tx,
    )
    .context("start sharing")?;
    // Only when turned on. If the "ask" port can't be used, password access
    // still works.
    let ask_host = if !settings.ask {
        tracing::info!("ask-to-connect is off (turn it on in Cipher Desk's Share screen page)");
        None
    } else {
        match start(
            Door::Ask,
            HostConfig {
                port: rdpv_host::request_port(port),
                username: rdpv_host::REQUEST_USERNAME.to_owned(),
                password: SecretString::new(rdpv_host::REQUEST_PASSWORD.to_owned()),
                data_dir,
                require_approval: true,
                // The person at the computer decides; no code is asked here.
                second_factor: None,
                tailnet_only: settings.tailnet_only,
            },
            &tx,
        ) {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "ask-to-connect is off");
                None
            }
        }
    };
    if !settings.tailnet_only {
        tracing::warn!("accepting connections from any network (Tailscale only is off)");
    }

    // Stay alive, logging what happens, until the service ends this process.
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((door, HostEvent::Listening { port })) => {
                tracing::info!(?door, port, "service host listening");
            }
            Ok((door, HostEvent::ViewerConnected { address })) => {
                tracing::info!(?door, %address, "viewer connected");
                let time = crate::history::local_time();
                link.viewer_connected(door as u32, address.clone());
                let link = std::sync::Arc::clone(&link);
                // The name lookup can take a few seconds; do not hold up events.
                std::thread::spawn(move || {
                    let who = who_is(&address);
                    link.viewer_named(door as u32, &address, who.clone());
                    crate::history::record(&time, "Connected", &who, door.label());
                });
            }
            Ok((door, HostEvent::ViewerDisconnected)) => {
                // Also sent for refused attempts: only real sessions go in
                // the history.
                if let Some(who) = link.viewer_disconnected(door as u32) {
                    tracing::info!(?door, "viewer disconnected");
                    let time = crate::history::local_time();
                    crate::history::record(&time, "Disconnected", &who, door.label());
                }
            }
            Ok((door, HostEvent::SignInFailed { address })) => {
                tracing::warn!(?door, %address, "wrong user name or password");
                let time = crate::history::local_time();
                let link = std::sync::Arc::clone(&link);
                std::thread::spawn(move || {
                    let who = who_is(&address);
                    link.sign_in_failed(who.clone());
                    crate::history::record(&time, "WRONG PASSWORD", &who, door.label());
                });
            }
            Ok((Door::Password, HostEvent::ApprovalRequested { .. })) => {
                // The password door never asks; allow so nobody waits forever.
                password_host.approve(true);
            }
            Ok((Door::Ask, HostEvent::ApprovalRequested { address })) => {
                if let Some(host) = &ask_host {
                    let approver = host.approver();
                    // The popup blocks until answered or timed out.
                    std::thread::spawn(move || {
                        if crate::config::is_allowed_device(&address) {
                            tracing::info!(%address, "ask-to-connect: always-allowed device");
                            approver.approve(true);
                            return;
                        }
                        let who = who_is(&address);
                        let allow = ask_person_at_computer(&who);
                        tracing::info!(allow, "ask-to-connect answered");
                        approver.approve(allow);
                        // Only Tailscale addresses belong to one device for
                        // sure; offer to remember those.
                        let ip = address.parse::<std::net::IpAddr>().ok();
                        let tailnet = ip.is_some_and(|ip| {
                            !ip.is_loopback() && rdpv_host::is_tailnet_or_local(ip)
                        });
                        if allow && tailnet && ask_always_allow(&who) {
                            match crate::config::allow_device(&address) {
                                Ok(()) => tracing::info!(%address, "device will always be allowed"),
                                Err(e) => {
                                    tracing::warn!(error = %format!("{e:#}"), "could not remember the device")
                                }
                            }
                        }
                    });
                }
            }
            Ok((door, HostEvent::FileReceived { .. })) => {
                tracing::info!(?door, "file received from viewer");
            }
            Ok((door, HostEvent::LockedOut { minutes, address })) => {
                tracing::warn!(?door, minutes, %address, "blocked after too many wrong passwords");
                let time = crate::history::local_time();
                let what = format!("BLOCKED {minutes} MIN");
                let who = format!("{address} (too many wrong passwords)");
                crate::history::record(&time, &what, &who, door.label());
            }
            Ok((Door::Password, HostEvent::Failed { message })) => {
                tracing::error!(%message, "service host failed");
                anyhow::bail!("{message}");
            }
            Ok((Door::Ask, HostEvent::Failed { message })) => {
                tracing::warn!(%message, "ask-to-connect stopped");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("the host stopped");
            }
        }
    }
}

/// Starts one listener whose events arrive tagged with `door`.
fn start(
    door: Door,
    config: HostConfig,
    tx: &mpsc::Sender<(Door, HostEvent)>,
) -> anyhow::Result<HostHandle> {
    let (host_tx, host_rx) = mpsc::channel::<HostEvent>();
    let tx = tx.clone();
    std::thread::Builder::new()
        .name(format!("cipher-desk-{door:?}-events").to_lowercase())
        .spawn(move || {
            for event in host_rx {
                if tx.send((door, event)).is_err() {
                    break;
                }
            }
        })
        .context("event thread")?;
    rdpv_host::start(
        config,
        ServiceScreenCapture::default(),
        ServiceSendInputSink::default(),
        host_tx,
    )
}

/// Shows the Windows popup "allow this connection?" on the screen of this
/// session. `true` only for Yes; No, closing it, or no answer within
/// [`rdpv_host::APPROVAL_TIMEOUT`] refuse.
    let seconds = rdpv_host::APPROVAL_TIMEOUT.as_secs();
    show_question(
        &format!(
            "Someone is asking to see and control this computer.\n\n\
             Device: {who}\n\n\
             Allow the connection?\n\n\
             If nobody answers, it is refused in {seconds} seconds."
        ),
        seconds,
    )
}

/// After allowing: offer to skip the question for this device from now on.
fn ask_always_allow(who: &str) -> bool {
    show_question(
        &format!(
            "Always allow this device without asking?\n\n\
             Device: {who}\n\n\
             Choose No to be asked every time. Turning \"Ask to connect\" off in \
             Cipher Desk forgets all always-allowed devices."
        ),
        30,
    )
}

/// A Yes/No question on the signed-in person's screen (No is the default and
/// the answer when nobody responds within `seconds`).
#[allow(unsafe_code)]
fn show_question(text: &str, seconds: u64) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::RemoteDesktop::{WTSSendMessageW, WTS_CURRENT_SERVER_HANDLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
        MESSAGEBOX_RESULT,
    };

    /// "This session" for WTSSendMessage.
    const WTS_CURRENT_SESSION: u32 = u32::MAX;

    let title: Vec<u16> = "Cipher Desk".encode_utf16().collect();
    let message: Vec<u16> = text.encode_utf16().collect();
    let mut response = MESSAGEBOX_RESULT(0);
    let timeout = u32::try_from(seconds).unwrap_or(30);
    // SAFETY: the title and message buffers outlive the call; their lengths
    // are given in bytes as WTSSendMessageW expects; `response` is a valid
    // out-pointer. bWait = true makes the call block until answered.
    let sent = unsafe {
        WTSSendMessageW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            PCWSTR(title.as_ptr()),
            u32::try_from(title.len() * 2).unwrap_or(0),
            PCWSTR(message.as_ptr()),
            u32::try_from(message.len() * 2).unwrap_or(0),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND,
            timeout,
            &mut response,
            true,
        )
    };
    if let Err(e) = sent {
        tracing::warn!(error = %e, "question popup could not be shown");
        return false;
    }
    response == IDYES
}
