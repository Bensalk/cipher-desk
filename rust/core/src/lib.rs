//! Shared core of Secure RDP Viewer.
//!
//! Stage 2 (foundation) provides:
//! - [`secret`]: a password wrapper that never prints and is wiped from memory on drop,
//! - [`logging`]: the logging policy (capped at `info` in release, `sspi` always off),
//! - [`policy`]: secure defaults the rest of the app must start from,
//! - [`version`]: build and engine information shown in the UI.
//!
//! Stage 4 (connection engine) adds:
//! - [`pins`]: certificate pin store (trust on first use),
//! - [`tls`]: TLS configuration that only accepts the pinned certificate,
//! - [`session`]: connect, display updates, input and disconnect.
//!
//! Later stages add clipboard, profiles and vault access.

pub mod input;
pub mod logging;
pub mod pins;
pub mod policy;
pub mod secret;
pub mod session;
pub mod status;
pub mod tls;
pub mod version;
