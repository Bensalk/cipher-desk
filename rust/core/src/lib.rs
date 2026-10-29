//! Shared core of Secure RDP Viewer.
//!
//! Stage 2 (foundation) provides:
//! - [`secret`]: a password wrapper that never prints and is wiped from memory on drop,
//! - [`logging`]: the logging policy (capped at `info` in release, `sspi` always off),
//! - [`policy`]: secure defaults the rest of the app must start from,
//! - [`version`]: build and engine information shown in the UI.
//!
//! Later stages add the connection engine, certificate pinning, clipboard,
//! profiles and vault access.

pub mod logging;
pub mod policy;
pub mod secret;
pub mod version;
