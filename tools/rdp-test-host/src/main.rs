//! Test-only RDP host with NLA and a scriptable clipboard, for the live
//! clipboard tests (Stage 6). Based on the `ironrdp` 0.17 server example
//! (MIT OR Apache-2.0, Devolutions). Not part of the app; never shipped.
//!
//! Configuration comes from environment variables, so no secret ever appears
//! on a command line:
//!   RDPV_HOST_BIND       listen address (default 127.0.0.1:3391)
//!   RDPV_HOST_USER       username
//!   RDPV_HOST_PASSWORD   password
//!   RDPV_HOST_CERT / RDPV_HOST_KEY   PEM certificate and key (TLS)
//!   RDPV_HOST_CLIP_TEXT  text the host offers on its clipboard
//!   RDPV_HOST_CLIP_OUT   file that receives text pasted from the client

use std::io::Write as _;
use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroUsize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use ironrdp::cliprdr::backend::{ClipboardMessage, CliprdrBackend, CliprdrBackendFactory};
use ironrdp::cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId, OwnedFormatDataResponse,
};
use ironrdp::connector::DesktopSize;
use ironrdp::server::tokio::sync::mpsc::UnboundedSender;
use ironrdp::server::tokio::time::{sleep, Duration};
use ironrdp::server::{
    BitmapUpdate, CliprdrServerFactory, Credentials, DisplayUpdate, KeyboardEvent, MouseEvent,
    PixelFormat, RdpServer, RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler,
    ServerEvent, ServerEventSender, TlsIdentityCtx,
};

const WIDTH: u16 = 1024;
const HEIGHT: u16 = 768;

fn env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} is not set"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind: SocketAddr = std::env::var("RDPV_HOST_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3391".into())
        .parse()?;
    let identity = TlsIdentityCtx::init_from_paths(
        &PathBuf::from(env("RDPV_HOST_CERT")?),
        &PathBuf::from(env("RDPV_HOST_KEY")?),
    )
    .context("TLS identity")?;
    let acceptor = identity.make_acceptor().context("TLS acceptor")?;

    let clip = Arc::new(ClipShared {
        host_text: std::env::var("RDPV_HOST_CLIP_TEXT").unwrap_or_default(),
        out: std::env::var("RDPV_HOST_CLIP_OUT").ok().map(PathBuf::from),
        sender: Mutex::new(None),
    });

    let mut server = RdpServer::builder()
        .with_addr(bind)
        .with_hybrid(acceptor, identity.pub_key)
        .with_input_handler(Handler)
        .with_display_handler(Handler)
        .with_cliprdr_factory(Some(Box::new(ClipFactory(Arc::clone(&clip)))))
        .build();
    server.set_credentials(Some(Credentials {
        username: env("RDPV_HOST_USER")?,
        password: env("RDPV_HOST_PASSWORD")?,
        domain: None,
    }));
    server.run().await
}

// ---------------------------------------------------------------- clipboard

struct ClipShared {
    host_text: String,
    out: Option<PathBuf>,
    sender: Mutex<Option<UnboundedSender<ServerEvent>>>,
}

impl ClipShared {
    fn send(&self, m: ClipboardMessage) {
        if let Ok(guard) = self.sender.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(ServerEvent::Clipboard(m));
            }
        }
    }

    fn advertise(&self) {
        let formats = if self.host_text.is_empty() {
            Vec::new()
        } else {
            vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
        };
        self.send(ClipboardMessage::SendInitiateCopy(formats));
    }
}

struct ClipFactory(Arc<ClipShared>);

impl CliprdrBackendFactory for ClipFactory {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        Box::new(ClipBackend(Arc::clone(&self.0)))
    }
}

impl ServerEventSender for ClipFactory {
    fn set_sender(&mut self, sender: UnboundedSender<ServerEvent>) {
        if let Ok(mut g) = self.0.sender.lock() {
            *g = Some(sender);
        }
    }
}

impl CliprdrServerFactory for ClipFactory {}

#[derive(Debug)]
struct ClipBackend(Arc<ClipShared>);

impl std::fmt::Debug for ClipShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipShared")
    }
}

ironrdp_core::impl_as_any!(ClipBackend);

impl CliprdrBackend for ClipBackend {
    fn temporary_directory(&self) -> &str {
        ".cliprdr"
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::empty()
    }

    fn on_ready(&mut self) {
        self.0.advertise();
    }

    fn on_request_format_list(&mut self) {
        self.0.advertise();
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        if formats.iter().any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT) {
            self.0
                .send(ClipboardMessage::SendInitiatePaste(ClipboardFormatId::CF_UNICODETEXT));
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        let response = if request.format == ClipboardFormatId::CF_UNICODETEXT && !self.0.host_text.is_empty() {
            OwnedFormatDataResponse::new_unicode_string(&self.0.host_text)
        } else {
            OwnedFormatDataResponse::new_error()
        };
        self.0.send(ClipboardMessage::SendFormatData(response));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if response.is_error() {
            return;
        }
        let Ok(text) = response.to_unicode_string() else {
            return;
        };
        if let Some(path) = &self.0.out {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{text}");
            }
        }
    }

    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}

// ---------------------------------------------------------------- display / input

#[derive(Clone, Debug)]
struct Handler;

impl RdpServerInputHandler for Handler {
    fn keyboard(&mut self, _: KeyboardEvent) {}
    fn mouse(&mut self, _: MouseEvent) {}
}

struct Updates {
    sent: bool,
}

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        if self.sent {
            sleep(Duration::from_millis(500)).await;
        }
        self.sent = true;
        let (w, h) = (NonZeroU16::new(WIDTH).context("w")?, NonZeroU16::new(HEIGHT).context("h")?);
        let stride = NonZeroUsize::new(usize::from(WIDTH) * 4).context("stride")?;
        let data = vec![0x80u8; usize::from(WIDTH) * usize::from(HEIGHT) * 4];
        Ok(Some(DisplayUpdate::Bitmap(BitmapUpdate {
            x: 0,
            y: 0,
            width: w,
            height: h,
            format: PixelFormat::BgrA32,
            data: data.into(),
            stride,
        })))
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for Handler {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: WIDTH,
            height: HEIGHT,
        }
    }

    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(Updates { sent: false }))
    }
}
