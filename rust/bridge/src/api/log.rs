//! Viewer-side logging: initialise the tracing subscriber and export the
//! in-memory session log for the "Report Problem" feature.

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Set up the global tracing subscriber for the viewer app.
///
/// Must be called once, right after `RustLib.init()`, before any session
/// starts. Safe to call again — subsequent calls are no-ops because
/// [`tracing_subscriber::Registry`] uses a `Once` guard internally.
///
/// Two layers are registered:
/// 1. A fmt layer that writes to stderr (visible in `adb logcat` on Android,
///    or the console on Windows/Linux).  It is a no-op in release builds at
///    the tracing crate level (`release_max_level_info` feature).
/// 2. [`rdpv_core::session_log::SessionLog`] — the in-memory ring buffer that
///    powers the "Report Problem" export.
#[flutter_rust_bridge::frb(sync)]
pub fn init_logging() {
    // EnvFilter as a global layer + fmt layer + SessionLog ring-buffer.
    let _ = tracing_subscriber::registry()
        .with(rdpv_core::logging::env_filter("info"))
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(rdpv_core::session_log::SessionLog)
        .try_init();
}

/// Export the in-memory session log as plain text.
///
/// Returns the last ≤ 1 000 log lines from the most recent session, with a
/// header that includes `app_version`.  Call this after a session ends (or
/// while it is still running) to get a log file the user can share.
///
/// The result never contains passwords or screen content.
#[flutter_rust_bridge::frb(sync)]
pub fn session_log_export(app_version: String) -> String {
    rdpv_core::session_log::export(&app_version)
}

/// Clear the in-memory log buffer.
///
/// Called by the Flutter session service at the start of each new connection
/// attempt so the exported file covers only the current session.
#[flutter_rust_bridge::frb(sync)]
pub fn session_log_clear() {
    rdpv_core::session_log::clear();
}
