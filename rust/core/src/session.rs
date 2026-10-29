//! RDP session engine (Stage 4).
//!
//! Blocking design: [`connect`] performs TCP → TLS (pinned certificate) →
//! NLA/CredSSP → capability exchange; [`Session`] is then driven by one
//! worker thread that calls [`Session::poll`] in a loop and forwards input.
//!
//! Security properties:
//! - NLA (CredSSP) is mandatory and legacy RDP security is never offered (S1).
//! - The server certificate must match the pin; an unknown or changed
//!   certificate stops the connection before any credentials are sent (S2).
//! - Every network step has a timeout (SF-3).
//! - Errors shown to users are mapped to fixed messages
//!   ([`ConnectionError`]); raw library text is only used for classification.

use core::time::Duration;
use std::io::{self, Write as _};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs as _};

use ironrdp_blocking::Framed;
use ironrdp_cliprdr::pdu::{ClipboardFormat, ClipboardFormatId, OwnedFormatDataResponse};
use ironrdp_cliprdr::CliprdrClient;
use ironrdp_connector::connection_activation::{
    ConnectionActivationFactory, ConnectionActivationState,
};
use ironrdp_connector::{
    ClientConnector, ConnectionResult, Credentials, DesktopSize, Sequence as _,
};
use ironrdp_core::WriteBuf;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use ironrdp_pdu::gcc::{ConnectionType, KeyboardType};
use ironrdp_pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{
    ActiveStage, ActiveStageBuilder, ActiveStageOutput, GracefulDisconnectReason,
};

use crate::clipboard::{decode_unicode_text, ClipAction, ClipQueue, ClipboardPolicy, QueueBackend};
use crate::input::{Endpoint, Username};
use crate::secret::SecretString;
use crate::status::ConnectionError;
use crate::tls;

type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;
type RdpFramed = Framed<TlsStream>;

/// How long one [`Session::poll`] call waits for data before returning.
pub const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// Longest time one [`Session::poll`] call keeps processing when data arrives
/// continuously (a busy screen), so the worker can still send input and see
/// stop requests.
pub const POLL_BUDGET: Duration = Duration::from_millis(50);

/// Everything needed to open a connection.
#[derive(Debug)]
pub struct ConnectParams {
    pub endpoint: Endpoint,
    pub username: Username,
    pub password: SecretString,
    /// Requested desktop size; the server may choose another.
    pub desktop_width: u16,
    pub desktop_height: u16,
    /// Pinned certificate fingerprint for this endpoint, if the user approved one.
    pub pinned_fingerprint: Option<String>,
    /// Timeout for connecting and for each read/write during setup.
    pub connect_timeout: Duration,
    /// Text clipboard sharing (off by default). When off, the clipboard
    /// channel is not even offered to the host.
    pub clipboard: ClipboardPolicy,
}

/// Why [`connect`] did not produce a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectFailure {
    /// First connection: show this fingerprint and ask the user to approve it.
    /// Nothing was sent to the host.
    CertificateNeedsApproval { fingerprint: String },
    /// The certificate differs from the pinned one. Nothing was sent to the host.
    CertificateChanged { pinned: String, presented: String },
    /// Any other failure, as a user-facing error.
    Failed(ConnectionError),
}

impl ConnectFailure {
    /// The user-facing error for this failure.
    pub fn error(&self) -> ConnectionError {
        match self {
            Self::CertificateNeedsApproval { .. } => ConnectionError::CertificateUnknown,
            Self::CertificateChanged { .. } => ConnectionError::CertificateChanged,
            Self::Failed(e) => *e,
        }
    }
}

/// A rectangle of the remote screen that changed (inclusive coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

/// Something that happened during a [`Session::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Part of the screen changed; read it from [`Session::frame`].
    Updated(Region),
    /// The server changed the desktop size (after a reactivation).
    Resized { width: u16, height: u16 },
    /// The session ended. `None` means the user asked to disconnect.
    Ended(Option<ConnectionError>),
    /// Text copied on the host, allowed by the policy, for the local clipboard.
    RemoteClipboardText(String),
    /// Text copied on the host was larger than the policy allows and was dropped.
    RemoteClipboardTooLarge,
}

/// Result of offering this device's clipboard text to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOffer {
    /// The host was told new text is available; it is sent when the host pastes.
    Offered,
    /// Sharing from this device to the host is off for this session.
    Disabled,
    /// Larger than the policy allows; not offered.
    TooLarge,
    /// Same text as last time (or text that just came from the host); nothing to do.
    Unchanged,
}

/// Clipboard state for one session.
struct ClipState {
    policy: ClipboardPolicy,
    queue: ClipQueue,
    /// Text this device currently offers to the host.
    local_text: Option<String>,
    /// Last text received from the host, to avoid echoing it straight back.
    last_remote: Option<String>,
    ready: bool,
}

/// An open RDP session.
pub struct Session {
    framed: RdpFramed,
    stage: ActiveStage,
    image: DecodedImage,
    activation_factory: ConnectionActivationFactory,
    input: Database,
    ended: bool,
    clip: Option<ClipState>,
}

impl core::fmt::Debug for Session {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Session")
            .field("width", &self.image.width())
            .field("height", &self.image.height())
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

/// Opens a session: TCP, pinned TLS, NLA and capability exchange.
pub fn connect(params: ConnectParams) -> Result<Session, ConnectFailure> {
    let ConnectParams {
        endpoint,
        username,
        password,
        desktop_width,
        desktop_height,
        pinned_fingerprint,
        connect_timeout,
        clipboard,
    } = params;

    let addr = resolve(&endpoint).map_err(ConnectFailure::Failed)?;
    let tcp = TcpStream::connect_timeout(&addr, connect_timeout)
        .map_err(|e| ConnectFailure::Failed(classify_io(&e)))?;
    set_timeouts(&tcp, Some(connect_timeout))
        .map_err(|e| ConnectFailure::Failed(classify_io(&e)))?;
    let client_addr = tcp
        .local_addr()
        .map_err(|e| ConnectFailure::Failed(classify_io(&e)))?;

    let config = connector_config(&username, &password, desktop_width, desktop_height);
    // The connector holds its own copy of the password until CredSSP is done.
    drop(password);
    let mut connector = ClientConnector::new(config, client_addr);
    let clip = if clipboard.is_enabled() {
        let queue = ClipQueue::default();
        connector = connector.with_static_channel(CliprdrClient::new(Box::new(QueueBackend::new(
            std::sync::Arc::clone(&queue),
        ))));
        Some(ClipState {
            policy: clipboard,
            queue,
            local_text: None,
            last_remote: None,
            ready: false,
        })
    } else {
        None
    };

    let mut framed = Framed::new(tcp);
    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector)
        .map_err(|e| ConnectFailure::Failed(classify_connector(&e)))?;

    let tcp = framed.into_inner_no_leftover();
    let (tls_stream, server_public_key) = tls_upgrade(tcp, &endpoint.host, pinned_fingerprint)?;
    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = Framed::new(tls_stream);

    let mut network_client = sspi::network_client::reqwest_network_client::ReqwestNetworkClient;
    let result: ConnectionResult = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut network_client,
        endpoint.host.clone().into(),
        server_public_key,
        None,
    )
    .and_then(ironrdp_connector::ConnectionOutcome::into_connected)
    .map_err(|e| ConnectFailure::Failed(classify_connector(&e)))?;

    // From now on reads return quickly so the worker can also handle input and stop requests.
    set_timeouts(&framed.get_inner().0.sock, Some(POLL_INTERVAL))
        .map_err(|e| ConnectFailure::Failed(classify_io(&e)))?;

    let image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let activation_factory = result.activation_factory.clone();
    let stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    Ok(Session {
        framed,
        stage,
        image,
        activation_factory,
        input: Database::new(),
        ended: false,
        clip,
    })
}

impl Session {
    /// Current desktop size.
    pub fn size(&self) -> (u16, u16) {
        (self.image.width(), self.image.height())
    }

    /// The whole remote screen as RGBA pixels, row by row.
    pub fn frame(&self) -> &[u8] {
        self.image.data()
    }

    /// Copies one region out of the frame as tightly packed RGBA rows.
    pub fn region_pixels(&self, r: Region) -> Vec<u8> {
        let (w, h) = self.size();
        let (w, h) = (usize::from(w), usize::from(h));
        let left = usize::from(r.left).min(w.saturating_sub(1));
        let right = usize::from(r.right).min(w.saturating_sub(1));
        let top = usize::from(r.top).min(h.saturating_sub(1));
        let bottom = usize::from(r.bottom).min(h.saturating_sub(1));
        if w == 0 || h == 0 || right < left || bottom < top {
            return Vec::new();
        }
        let data = self.image.data();
        let stride = w * 4;
        let mut out = Vec::with_capacity((right - left + 1) * (bottom - top + 1) * 4);
        for y in top..=bottom {
            let start = y * stride + left * 4;
            let end = y * stride + (right + 1) * 4;
            if let Some(row) = data.get(start..end) {
                out.extend_from_slice(row);
            }
        }
        out
    }

    /// True once the session has ended.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Reads and processes what the server sent. Returns after about
    /// [`POLL_INTERVAL`] when nothing arrives, or after [`POLL_BUDGET`] when
    /// data keeps arriving.
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        if self.ended {
            return events;
        }
        let started = std::time::Instant::now();
        while started.elapsed() < POLL_BUDGET {
            let (action, payload) = match self.framed.read_pdu() {
                Ok(pdu) => pdu,
                Err(e) if is_timeout(&e) => break,
                Err(e) => {
                    self.end(&mut events, Some(classify_io(&e)));
                    break;
                }
            };
            let outputs = match self.stage.process(
                &mut self.image,
                action,
                &payload,
                self.framed.last_read_at(),
            ) {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!(error = %e.report(), "session processing failed");
                    self.end(&mut events, Some(ConnectionError::Unexpected));
                    break;
                }
            };
            if let Err(err) = self.handle_outputs(outputs, &mut events) {
                self.end(&mut events, Some(err));
                break;
            }
            if self.ended {
                break;
            }
        }
        if !self.ended {
            if let Err(err) = self.service_clipboard(&mut events) {
                self.end(&mut events, Some(err));
            }
        }
        events
    }

    /// Offers this device's clipboard text to the host (if the policy allows).
    /// The text itself is only sent when the host pastes.
    pub fn offer_local_clipboard(
        &mut self,
        text: String,
    ) -> Result<ClipboardOffer, ConnectionError> {
        let Some(clip) = self.clip.as_mut() else {
            return Ok(ClipboardOffer::Disabled);
        };
        if !clip.policy.local_to_host {
            return Ok(ClipboardOffer::Disabled);
        }
        if text.is_empty() || !clip.policy.fits(&text) {
            return Ok(ClipboardOffer::TooLarge);
        }
        if clip.local_text.as_deref() == Some(text.as_str())
            || clip.last_remote.as_deref() == Some(text.as_str())
        {
            return Ok(ClipboardOffer::Unchanged);
        }
        clip.local_text = Some(text);
        if clip.ready {
            self.advertise_local()?;
        }
        Ok(ClipboardOffer::Offered)
    }

    /// Tells the host which formats this device offers (plain text or nothing).
    fn advertise_local(&mut self) -> Result<(), ConnectionError> {
        let offer_text = self
            .clip
            .as_ref()
            .is_some_and(|c| c.policy.local_to_host && c.local_text.is_some());
        let formats = if offer_text {
            vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
        } else {
            Vec::new()
        };
        let msgs = self
            .stage
            .get_svc_processor_mut::<CliprdrClient>()
            .ok_or(ConnectionError::Unexpected)?
            .initiate_copy(&formats)
            .map_err(|_| ConnectionError::Unexpected)?;
        self.send_svc(msgs)
    }

    fn send_svc(
        &mut self,
        msgs: ironrdp_cliprdr::CliprdrSvcMessages<ironrdp_cliprdr::Client>,
    ) -> Result<(), ConnectionError> {
        let frame = self
            .stage
            .process_svc_processor_messages(msgs)
            .map_err(|_| ConnectionError::Unexpected)?;
        if !frame.is_empty() {
            self.write(&frame)?;
        }
        Ok(())
    }

    /// Acts on what the clipboard channel reported, applying the policy.
    fn service_clipboard(&mut self, events: &mut Vec<SessionEvent>) -> Result<(), ConnectionError> {
        let actions: Vec<ClipAction> = match self.clip.as_ref() {
            Some(c) => c
                .queue
                .lock()
                .map(|mut q| std::mem::take(&mut *q))
                .unwrap_or_default(),
            None => return Ok(()),
        };
        for action in actions {
            let Some(clip) = self.clip.as_mut() else {
                return Ok(());
            };
            let policy = clip.policy;
            match action {
                ClipAction::AdvertiseLocal => {
                    clip.ready = true;
                    self.advertise_local()?;
                }
                ClipAction::RemoteChanged { has_text } => {
                    if has_text && policy.host_to_local {
                        let msgs = self
                            .stage
                            .get_svc_processor_mut::<CliprdrClient>()
                            .ok_or(ConnectionError::Unexpected)?
                            .initiate_paste(ClipboardFormatId::CF_UNICODETEXT)
                            .map_err(|_| ConnectionError::Unexpected)?;
                        self.send_svc(msgs)?;
                    }
                }
                ClipAction::DataRequested(format) => {
                    let text = clip.local_text.clone().filter(|_| policy.local_to_host);
                    let response = match text {
                        Some(t) if format == ClipboardFormatId::CF_UNICODETEXT => {
                            OwnedFormatDataResponse::new_unicode_string(&t)
                        }
                        _ => OwnedFormatDataResponse::new_error(),
                    };
                    let msgs = self
                        .stage
                        .get_svc_processor_mut::<CliprdrClient>()
                        .ok_or(ConnectionError::Unexpected)?
                        .submit_format_data(response)
                        .map_err(|_| ConnectionError::Unexpected)?;
                    self.send_svc(msgs)?;
                }
                ClipAction::DataReceived(Some(bytes)) if policy.host_to_local => {
                    if bytes.len() > policy.max_bytes + 2 {
                        events.push(SessionEvent::RemoteClipboardTooLarge);
                        continue;
                    }
                    if let Some(text) = decode_unicode_text(&bytes) {
                        if !text.is_empty() {
                            clip.last_remote = Some(text.clone());
                            events.push(SessionEvent::RemoteClipboardText(text));
                        }
                    }
                }
                ClipAction::DataReceived(_) => {}
            }
        }
        Ok(())
    }

    fn handle_outputs(
        &mut self,
        outputs: Vec<ActiveStageOutput>,
        events: &mut Vec<SessionEvent>,
    ) -> Result<(), ConnectionError> {
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => self.write(&frame)?,
                ActiveStageOutput::GraphicsUpdate(r) => {
                    events.push(SessionEvent::Updated(Region {
                        left: r.left,
                        top: r.top,
                        right: r.right,
                        bottom: r.bottom,
                    }))
                }
                ActiveStageOutput::Terminate(reason) => {
                    tracing::info!(reason = %reason, "server ended the session");
                    let err = match reason {
                        GracefulDisconnectReason::UserInitiated => None,
                        _ => Some(classify_disconnect_reason(&reason.description())),
                    };
                    self.end(events, err);
                    return Ok(());
                }
                ActiveStageOutput::DeactivateAll => self.reactivate(events)?,
                ActiveStageOutput::ServerRedirection(_) => {
                    // Redirection (RD Connection Broker) is not supported yet.
                    return Err(ConnectionError::Unexpected);
                }
                // Pointer shapes, monitor layout, auto-reconnect cookies and other
                // notifications are not used by this client yet.
                _ => {}
            }
        }
        Ok(())
    }

    /// Runs the Deactivation-Reactivation Sequence (MS-RDPBCGR 1.3.1.3), which
    /// servers use for example when the desktop size changes.
    fn reactivate(&mut self, events: &mut Vec<SessionEvent>) -> Result<(), ConnectionError> {
        tracing::debug!("server requested reactivation");
        let mut seq = self.activation_factory.create();
        let mut buf = WriteBuf::new();
        loop {
            buf.clear();
            let written = if let Some(hint) = seq.next_pdu_hint() {
                let pdu = read_by_hint_blocking(&mut self.framed, hint)?;
                seq.step(&pdu, self.framed.last_read_at(), &mut buf)
            } else {
                seq.step_no_input(&mut buf)
            }
            .map_err(|e| classify_connector(&e))?;
            if written.size().is_some() {
                self.write(buf.filled())?;
            }
            match seq.connection_activation_state() {
                ConnectionActivationState::Finalized {
                    desktop_size,
                    share_id,
                    static_channel_chunk_size,
                    enable_server_pointer,
                    pointer_software_rendering,
                    ..
                } => {
                    self.image = DecodedImage::new(
                        PixelFormat::RgbA32,
                        desktop_size.width,
                        desktop_size.height,
                    );
                    if !self.stage.reactivate(
                        seq.io_channel_id(),
                        seq.user_channel_id(),
                        share_id,
                        enable_server_pointer,
                        pointer_software_rendering,
                        static_channel_chunk_size,
                    ) {
                        return Err(ConnectionError::Unexpected);
                    }
                    events.push(SessionEvent::Resized {
                        width: desktop_size.width,
                        height: desktop_size.height,
                    });
                    return Ok(());
                }
                ConnectionActivationState::Redirected { .. } => {
                    return Err(ConnectionError::Unexpected)
                }
                _ => {}
            }
        }
    }

    /// Sends keyboard/mouse input to the server.
    pub fn send_input(&mut self, ops: Vec<InputEvent>) -> Result<(), ConnectionError> {
        if self.ended {
            return Err(ConnectionError::ConnectionLost);
        }
        let (w, h) = self.size();
        let ops = ops.into_iter().filter_map(|op| op.into_operation(w, h));
        let events = self.input.apply(ops);
        if events.is_empty() {
            return Ok(());
        }
        let outputs = self
            .stage
            .process_fastpath_input(&mut self.image, &events)
            .map_err(|_| ConnectionError::Unexpected)?;
        let mut ignored = Vec::new();
        self.handle_outputs(outputs, &mut ignored)
    }

    /// Ends the session politely: releases held keys/buttons, asks the server to
    /// shut down, then closes the connection.
    pub fn disconnect(mut self) {
        if self.ended {
            return;
        }
        let release = self.input.release_all();
        if !release.is_empty() {
            if let Ok(outputs) = self.stage.process_fastpath_input(&mut self.image, &release) {
                let mut ignored = Vec::new();
                let _ = self.handle_outputs(outputs, &mut ignored);
            }
        }
        if let Ok(outputs) = self.stage.graceful_shutdown() {
            for out in outputs {
                if let ActiveStageOutput::ResponseFrame(frame) = out {
                    let _ = self.write(&frame);
                }
            }
        }
        let (stream, _) = self.framed.get_inner_mut();
        stream.conn.send_close_notify();
        let _ = stream.flush();
        let _ = stream.sock.shutdown(std::net::Shutdown::Both);
        self.ended = true;
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), ConnectionError> {
        self.framed.write_all(frame).map_err(|e| classify_io(&e))
    }

    fn end(&mut self, events: &mut Vec<SessionEvent>, err: Option<ConnectionError>) {
        if !self.ended {
            self.ended = true;
            events.push(SessionEvent::Ended(err));
        }
    }
}

/// Input from the UI, in remote-desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    MouseMove {
        x: u16,
        y: u16,
    },
    MouseDown(PointerButton),
    MouseUp(PointerButton),
    /// Positive = away from the user (scroll up). One notch is 120.
    Wheel {
        vertical: bool,
        delta: i16,
    },
    /// PC/AT set-1 scancode; `extended` for keys such as arrows and right Ctrl.
    KeyDown {
        scancode: u8,
        extended: bool,
    },
    KeyUp {
        scancode: u8,
        extended: bool,
    },
    /// Text typed with an on-screen keyboard or IME.
    Unicode {
        ch: char,
        down: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

impl InputEvent {
    fn into_operation(self, width: u16, height: u16) -> Option<Operation> {
        let button = |b: PointerButton| match b {
            PointerButton::Left => MouseButton::Left,
            PointerButton::Right => MouseButton::Right,
            PointerButton::Middle => MouseButton::Middle,
        };
        Some(match self {
            Self::MouseMove { x, y } => Operation::MouseMove(MousePosition {
                x: x.min(width.saturating_sub(1)),
                y: y.min(height.saturating_sub(1)),
            }),
            Self::MouseDown(b) => Operation::MouseButtonPressed(button(b)),
            Self::MouseUp(b) => Operation::MouseButtonReleased(button(b)),
            Self::Wheel { vertical, delta } => {
                if delta == 0 {
                    return None;
                }
                Operation::WheelRotations(WheelRotations {
                    is_vertical: vertical,
                    rotation_units: delta,
                })
            }
            Self::KeyDown { scancode, extended } => {
                Operation::KeyPressed(Scancode::from_u8(extended, scancode))
            }
            Self::KeyUp { scancode, extended } => {
                Operation::KeyReleased(Scancode::from_u8(extended, scancode))
            }
            Self::Unicode { ch, down: true } => Operation::UnicodeKeyPressed(ch),
            Self::Unicode { ch, down: false } => Operation::UnicodeKeyReleased(ch),
        })
    }
}

// ---------------------------------------------------------------------------
// Connection helpers

fn connector_config(
    username: &Username,
    password: &SecretString,
    width: u16,
    height: u16,
) -> ironrdp_connector::Config {
    // DOMAIN\user -> user + domain; user@domain (UPN) and plain names are passed as typed.
    let (user, domain) = match (&username.domain, username.upn) {
        (Some(d), false) => (username.user.clone(), Some(d.clone())),
        (Some(d), true) => (format!("{}@{}", username.user, d), None),
        (None, _) => (username.user.clone(), None),
    };
    ironrdp_connector::Config {
        credentials: Credentials::UsernamePassword {
            username: user,
            password: password.expose().to_owned(),
        },
        domain,
        // S1: TLS + NLA only; legacy RDP security is never offered.
        enable_tls: false,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        connection_type: ConnectionType::Lan,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: DesktopSize {
            width: width.max(200),
            height: height.max(200),
        },
        monitor_layout: None,
        desktop_scale_factor: 0,
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: None,
        monitor_extended_layout: None,
        relative_mouse_input: false,
        bitmap: None,
        client_build: 0,
        // Privacy: a fixed name, so the device's own name is not sent to the host.
        client_name: "rdpv".to_owned(),
        client_dir: String::new(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
        platform: platform(),
        hardware_id: None,
        request_data: None,
        autologon: false,
        // S3: audio redirection off.
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: None,
        enable_server_pointer: false,
        pointer_software_rendering: true,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
    }
}

const fn platform() -> MajorPlatformType {
    if cfg!(windows) {
        MajorPlatformType::WINDOWS
    } else if cfg!(target_os = "android") {
        MajorPlatformType::ANDROID
    } else {
        MajorPlatformType::UNIX
    }
}

fn resolve(endpoint: &Endpoint) -> Result<SocketAddr, ConnectionError> {
    (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| ConnectionError::HostNotFound)?
        .next()
        .ok_or(ConnectionError::HostNotFound)
}

fn set_timeouts(tcp: &TcpStream, t: Option<Duration>) -> io::Result<()> {
    tcp.set_read_timeout(t)?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;
    tcp.set_nodelay(true)
}

fn tls_upgrade(
    tcp: TcpStream,
    host: &str,
    pinned: Option<String>,
) -> Result<(TlsStream, Vec<u8>), ConnectFailure> {
    let (config, observed) = tls::client_config(pinned.clone())
        .map_err(|_| ConnectFailure::Failed(ConnectionError::Unexpected))?;
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| ConnectFailure::Failed(ConnectionError::HostNotFound))?;
    let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), name)
        .map_err(|_| ConnectFailure::Failed(ConnectionError::SecurityNotSupported))?;
    let mut stream = rustls::StreamOwned::new(conn, tcp);

    // Drive the handshake to completion before anything else is sent.
    while stream.conn.is_handshaking() {
        if let Err(e) = stream.conn.complete_io(&mut stream.sock) {
            let presented = observed.lock().ok().and_then(|o| o.fingerprint.clone());
            return Err(match (pinned, presented) {
                (None, Some(fp)) => ConnectFailure::CertificateNeedsApproval { fingerprint: fp },
                (Some(p), Some(fp)) if p != fp => ConnectFailure::CertificateChanged {
                    pinned: p,
                    presented: fp,
                },
                _ if is_timeout(&e) => ConnectFailure::Failed(ConnectionError::Timeout),
                _ => ConnectFailure::Failed(ConnectionError::SecurityNotSupported),
            });
        }
    }
    let cert = stream
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .ok_or(ConnectFailure::Failed(
            ConnectionError::SecurityNotSupported,
        ))?;
    let key = server_public_key(cert.as_ref()).ok_or(ConnectFailure::Failed(
        ConnectionError::SecurityNotSupported,
    ))?;
    Ok((stream, key))
}

/// The certificate's subject public key, which CredSSP binds to (MS-CSSP).
fn server_public_key(cert_der: &[u8]) -> Option<Vec<u8>> {
    use x509_cert::der::Decode as _;
    let cert = x509_cert::Certificate::from_der(cert_der).ok()?;
    cert.tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .map(<[u8]>::to_vec)
}

fn read_by_hint_blocking(
    framed: &mut RdpFramed,
    hint: &dyn ironrdp_pdu::PduHint,
) -> Result<bytes::Bytes, ConnectionError> {
    // Reads during reactivation may span several poll intervals.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match framed.read_by_hint(hint) {
            Ok(pdu) => return Ok(pdu),
            Err(e) if is_timeout(&e) && std::time::Instant::now() < deadline => continue,
            Err(e) => return Err(classify_io(&e)),
        }
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

// ---------------------------------------------------------------------------
// Error classification (internal text -> fixed user-facing error)

fn classify_io(e: &io::Error) -> ConnectionError {
    use io::ErrorKind as K;
    match e.kind() {
        K::ConnectionRefused => ConnectionError::HostRefused,
        K::TimedOut | K::WouldBlock => ConnectionError::Timeout,
        K::HostUnreachable | K::NetworkUnreachable => ConnectionError::Timeout,
        K::ConnectionReset
        | K::ConnectionAborted
        | K::BrokenPipe
        | K::UnexpectedEof
        | K::NotConnected
        | K::NetworkDown => ConnectionError::ConnectionLost,
        _ => ConnectionError::Unexpected,
    }
}

fn classify_connector(e: &ironrdp_connector::ConnectorError) -> ConnectionError {
    use ironrdp_connector::ConnectorErrorKind as K;
    match e.kind() {
        K::Negotiation(_) => ConnectionError::SecurityNotSupported,
        K::AccessDenied => ConnectionError::AccountProblem,
        _ => {
            // Find an underlying I/O error first (timeouts, resets).
            let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
            while let Some(s) = src {
                if let Some(io) = s.downcast_ref::<io::Error>() {
                    return classify_io(io);
                }
                src = s.source();
            }
            let text = format!("{}", e.report());
            classify_text(&text)
        }
    }
}

/// Classifies authentication/negotiation failures from library error text,
/// using Windows NTSTATUS codes. The text itself is never shown to users.
fn classify_text(text: &str) -> ConnectionError {
    let t = text.to_ascii_lowercase();
    const ACCOUNT: &[&str] = &[
        "c0000234", // STATUS_ACCOUNT_LOCKED_OUT
        "c0000072", // STATUS_ACCOUNT_DISABLED
        "c0000193", // STATUS_ACCOUNT_EXPIRED
        "c0000071", // STATUS_PASSWORD_EXPIRED
        "c0000224", // STATUS_PASSWORD_MUST_CHANGE
        "c000006e", // STATUS_ACCOUNT_RESTRICTION
        "c000006f", // STATUS_INVALID_LOGON_HOURS
        "c0000070", // STATUS_INVALID_WORKSTATION
        "c000015b", // STATUS_LOGON_TYPE_NOT_GRANTED
    ];
    // Wrong username/password: STATUS_LOGON_FAILURE (Windows), SEC_E_LOGON_DENIED and the
    // variant some servers send with a Win32 facility (e.g. IronRDP-based hosts).
    if t.contains("c000006d")
        || t.contains("8009030c")
        || t.contains("c007030c")
        || t.contains("logon_failure")
        || t.contains("logon failure")
        || t.contains("logon_denied")
    {
        ConnectionError::WrongCredentials
    } else if ACCOUNT.iter().any(|c| t.contains(c)) {
        ConnectionError::AccountProblem
    } else if t.contains("negotiation")
        || t.contains("hybrid_required")
        || t.contains("ssl_not_allowed")
        // The host offered only legacy security, which this client never accepts (S1).
        || t.contains("but server selected")
        || t.contains("standard_rdp_security")
    {
        ConnectionError::SecurityNotSupported
    } else if t.contains("timed out") || t.contains("timeout") {
        ConnectionError::Timeout
    } else {
        ConnectionError::Unexpected
    }
}

/// Maps a server disconnect reason to a user-facing error.
fn classify_disconnect_reason(text: &str) -> ConnectionError {
    let t = text.to_ascii_lowercase();
    if t.contains("idle") {
        ConnectionError::IdleTimeout
    } else if t.contains("another") || t.contains("replaced") || t.contains("other connection") {
        ConnectionError::SessionTakenOver
    } else {
        ConnectionError::DisconnectedByHost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_logon_errors() {
        // Text seen in the Stage 1 spike for a wrong password.
        let wrong = "[CredSSP] CredSSP: InvalidToken: CredSSP server returned an error status; status is STATUS_LOGON_FAILURE [0xc000006d]";
        assert_eq!(classify_text(wrong), ConnectionError::WrongCredentials);
        // Text seen from an IronRDP-based host for a wrong password.
        assert_eq!(
            classify_text("[CredSSP] CredSSP, caused by: InvalidToken: CredSSP server returned an error status; status is NSTATUS code 0xc007030c"),
            ConnectionError::WrongCredentials
        );
        assert_eq!(
            classify_text("SEC_E_LOGON_DENIED (0x8009030C)"),
            ConnectionError::WrongCredentials
        );
        assert_eq!(
            classify_text("status is STATUS_ACCOUNT_LOCKED_OUT [0xC0000234]"),
            ConnectionError::AccountProblem
        );
        assert_eq!(
            classify_text("negotiation failure: SSL_NOT_ALLOWED_BY_SERVER"),
            ConnectionError::SecurityNotSupported
        );
        // Text seen from xrdp, which does not support NLA.
        assert_eq!(
            classify_text("[Initiation] reason: client advertised HYBRID | HYBRID_EX, but server selected STANDARD_RDP_SECURITY"),
            ConnectionError::SecurityNotSupported
        );
        assert_eq!(classify_text("something else"), ConnectionError::Unexpected);
    }

    #[test]
    fn classifies_io_errors() {
        let e = |k| io::Error::new(k, "x");
        assert_eq!(
            classify_io(&e(io::ErrorKind::ConnectionRefused)),
            ConnectionError::HostRefused
        );
        assert_eq!(
            classify_io(&e(io::ErrorKind::TimedOut)),
            ConnectionError::Timeout
        );
        assert_eq!(
            classify_io(&e(io::ErrorKind::ConnectionReset)),
            ConnectionError::ConnectionLost
        );
        assert_eq!(
            classify_io(&e(io::ErrorKind::UnexpectedEof)),
            ConnectionError::ConnectionLost
        );
    }

    #[test]
    fn classifies_disconnect_reasons() {
        assert_eq!(
            classify_disconnect_reason("idle timeout"),
            ConnectionError::IdleTimeout
        );
        assert_eq!(
            classify_disconnect_reason("another user connected to the session"),
            ConnectionError::SessionTakenOver
        );
        assert_eq!(
            classify_disconnect_reason("server initiated disconnect"),
            ConnectionError::DisconnectedByHost
        );
    }

    #[test]
    fn input_is_clamped_to_the_desktop() {
        let op = InputEvent::MouseMove { x: 5000, y: 5000 }.into_operation(800, 600);
        assert!(matches!(
            op,
            Some(Operation::MouseMove(MousePosition { x: 799, y: 599 }))
        ));
        let wheel = InputEvent::Wheel {
            vertical: true,
            delta: 0,
        };
        assert!(wheel.into_operation(800, 600).is_none());
    }

    #[test]
    fn config_is_nla_only_and_sends_no_device_name() {
        let user = Username {
            user: "alice".into(),
            domain: Some("CORP".into()),
            upn: false,
        };
        let pw = SecretString::new(["a", "b", "c", "1"].concat());
        let c = connector_config(&user, &pw, 1024, 768);
        assert!(c.enable_credssp);
        assert!(!c.enable_standard_rdp_security);
        assert!(!c.enable_audio_playback && !c.enable_audio_capture);
        assert_eq!(c.client_name, "rdpv");
        assert_eq!(c.domain.as_deref(), Some("CORP"));
        let upn = Username {
            user: "alice".into(),
            domain: Some("example.com".into()),
            upn: true,
        };
        let c = connector_config(&upn, &pw, 1024, 768);
        assert!(c.domain.is_none());
        assert!(
            matches!(c.credentials, Credentials::UsernamePassword { ref username, .. } if username == "alice@example.com")
        );
    }
}
