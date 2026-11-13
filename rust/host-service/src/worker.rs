//! The worker: runs the Share screen Host on the active session, following the
//! input desktop so the viewer also sees the sign-in and lock screens.
//!
//! It listens twice, like AnyDesk:
//!
//! - on the sharing port with the sharing password: the viewer gets straight
//!   in (unattended access to your own computers);
//! - on the next port ([`rdpv_host::request_port`]) with the public
//!   "ask to connect" sign-in: a popup asks the person at this computer to
//!   allow or refuse, refused after 30 seconds without an answer.
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

pub fn run() -> anyhow::Result<()> {
    // Real screen pixels at any Windows display scale (picture and clicks).
    rdpv_host::win::make_dpi_aware();
    let data_dir = crate::config::data_dir()?;
    let password =
        crate::config::load_password()?.context("no sharing password is set for the service")?;
    let (username, port) = crate::config::load_settings();

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
        },
        &tx,
    )
    .context("start sharing")?;
    // If the "ask" port can't be used, password access still works.
    let ask_host = match start(
        Door::Ask,
        HostConfig {
            port: rdpv_host::request_port(port),
            username: rdpv_host::REQUEST_USERNAME.to_owned(),
            password: SecretString::new(rdpv_host::REQUEST_PASSWORD.to_owned()),
            data_dir,
            require_approval: true,
            // The person at the computer decides; no code is asked here.
            second_factor: None,
        },
        &tx,
    ) {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "ask-to-connect is off");
            None
        }
    };

    // Stay alive, logging what happens, until the service ends this process.
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((door, HostEvent::Listening { port })) => {
                tracing::info!(?door, port, "service host listening");
            }
            Ok((door, HostEvent::ViewerConnected { .. })) => {
                tracing::info!(?door, "viewer connected");
            }
            Ok((door, HostEvent::ViewerDisconnected)) => {
                tracing::info!(?door, "viewer disconnected");
            }
            Ok((Door::Password, HostEvent::ApprovalRequested)) => {
                // The password door never asks; allow so nobody waits forever.
                password_host.approve(true);
            }
            Ok((Door::Ask, HostEvent::ApprovalRequested)) => {
                if let Some(host) = &ask_host {
                    let approver = host.approver();
                    // The popup blocks until answered or timed out.
                    std::thread::spawn(move || {
                        let allow = ask_person_at_computer();
                        tracing::info!(allow, "ask-to-connect answered");
                        approver.approve(allow);
                    });
                }
            }
            Ok((door, HostEvent::FileReceived { .. })) => {
                tracing::info!(?door, "file received from viewer");
            }
            Ok((door, HostEvent::LockedOut { minutes })) => {
                tracing::warn!(?door, minutes, "locked out");
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
#[allow(unsafe_code)]
fn ask_person_at_computer() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::RemoteDesktop::{WTSSendMessageW, WTS_CURRENT_SERVER_HANDLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
        MESSAGEBOX_RESULT,
    };

    /// "This session" for WTSSendMessage.
    const WTS_CURRENT_SESSION: u32 = u32::MAX;

    let seconds = rdpv_host::APPROVAL_TIMEOUT.as_secs();
    let title: Vec<u16> = "Cipher Desk".encode_utf16().collect();
    let message: Vec<u16> = format!(
        "Someone is asking to see and control this computer.\n\n\
         Allow the connection?\n\n\
         If nobody answers, it is refused in {seconds} seconds."
    )
    .encode_utf16()
    .collect();
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
        tracing::warn!(error = %e, "ask-to-connect popup could not be shown");
        return false;
    }
    response == IDYES
}
