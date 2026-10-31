//! The worker: runs the Share screen Host on the active session, following the
//! input desktop so the viewer also sees the sign-in and lock screens.
//!
//! Started by the service with [`crate::launch::spawn_worker`]; it runs until
//! the service ends it (on stop or when the session changes).

use std::sync::mpsc;
use std::time::Duration;

use anyhow::Context as _;
use rdpv_host::win::{ServiceScreenCapture, ServiceSendInputSink};
use rdpv_host::{HostConfig, HostEvent};

pub fn run() -> anyhow::Result<()> {
    let data_dir = crate::config::data_dir()?;
    let password =
        crate::config::load_password()?.context("no sharing password is set for the service")?;
    let (username, port) = crate::config::load_settings();

    let (tx, rx) = mpsc::channel::<HostEvent>();
    let _host = rdpv_host::start(
        HostConfig {
            port,
            username,
            password,
            data_dir,
        },
        ServiceScreenCapture::default(),
        ServiceSendInputSink::default(),
        tx,
    )
    .context("start sharing")?;

    // Stay alive, logging what happens, until the service ends this process.
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(HostEvent::Listening { port }) => tracing::info!(port, "service host listening"),
            Ok(HostEvent::ViewerConnected { .. }) => tracing::info!("viewer connected"),
            Ok(HostEvent::ViewerDisconnected) => tracing::info!("viewer disconnected"),
            Ok(HostEvent::LockedOut { minutes }) => tracing::warn!(minutes, "locked out"),
            Ok(HostEvent::Failed { message }) => {
                tracing::error!(%message, "service host failed");
                anyhow::bail!("{message}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("the host stopped");
            }
        }
    }
}
