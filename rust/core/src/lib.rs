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
//! Stage 5 (credentials) adds:
//! - [`vault`]: saved passwords in the OS vault (Windows/Linux).
//!
//! Stage 6 (clipboard) adds:
//! - [`clipboard`]: two-way plain-text clipboard under a direction/size policy.
//!
//! Stage 7 (connections) adds:
//! - [`profiles`], [`settings`]: saved connections, history and settings (no secrets),
//! - [`fsutil`]: atomic, private writes with backups,
//! - [`wol`]: Wake-on-LAN and a reachability check.

pub mod clipboard;
pub mod fsutil;
pub mod input;
pub mod logging;
pub mod pins;
pub mod policy;
pub mod profiles;
pub mod secret;
pub mod session;
pub mod settings;
pub mod status;
pub mod tls;
pub mod vault;
pub mod version;
pub mod wol;

/// Internal parsers exposed only to the fuzzing crate (`rust/fuzz`, SA-3).
/// Never enabled in the app.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
