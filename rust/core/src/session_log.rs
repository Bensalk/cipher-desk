//! In-memory session log for the "Report Problem" export (viewer side).
//!
//! [`SessionLog`] is a [`tracing_subscriber::Layer`] that records the last
//! [`MAX_ENTRIES`] log lines produced during a session into a fixed ring
//! buffer. The buffer is global and lives for the app's lifetime; it is
//! cleared at the start of each new connection attempt so the exported file
//! covers only the most recent session.
//!
//! Security: passwords never reach the log because
//! - `sspi`/`picky` are silenced by [`crate::logging::ALWAYS_SILENCED`],
//! - tracing events never carry raw credentials (the core uses
//!   [`crate::secret::SecretString`] which redacts on `Display`/`Debug`),
//! - pixel / RGBA data is never logged (it is sent as bytes, not events).
//!
//! The exported text is plain UTF-8, safe to share.

use std::fmt::Write as FmtWrite;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum number of log lines kept in memory.
pub const MAX_ENTRIES: usize = 1_000;

/// One recorded log line.
#[derive(Clone)]
struct Entry {
    /// Milliseconds since UNIX epoch (UTC).
    ms: u64,
    level: Level,
    target: String,
    message: String,
}

/// A visitor that collects the `message` field (and any extra fields) from a
/// tracing event into a single `String`.
struct MessageVisitor {
    buf: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            if !self.buf.is_empty() {
                self.buf.push(' ');
            }
            let _ = write!(self.buf, "{value:?}");
        } else {
            if !self.buf.is_empty() {
                self.buf.push(' ');
            }
            let _ = write!(self.buf, "{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            if !self.buf.is_empty() {
                self.buf.push(' ');
            }
            self.buf.push_str(value);
        } else {
            if !self.buf.is_empty() {
                self.buf.push(' ');
            }
            let _ = write!(self.buf, "{}={value}", field.name());
        }
    }
}

static BUFFER: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// Clear the in-memory buffer. Call this at the start of each connection
/// attempt so the exported file covers only the current session.
pub fn clear() {
    if let Ok(mut buf) = BUFFER.lock() {
        buf.clear();
    }
}

/// Export the buffer as a plain-text log string ready to save to a file.
/// `app_version` is included in the header (e.g. `"0.3.5"`).
pub fn export(app_version: &str) -> String {
    let entries = match BUFFER.lock() {
        Ok(buf) => buf.clone(),
        Err(_) => return String::from("(log unavailable)"),
    };

    let mut out = String::with_capacity(entries.len() * 80 + 256);
    let _ = writeln!(out, "Cipher Desk v{app_version} — session log");
    let _ = writeln!(out, "Lines: {}", entries.len());
    let _ = writeln!(out, "{}", "-".repeat(60));

    for e in &entries {
        // Format as HH:MM:SS.mmm UTC
        let total_s = e.ms / 1_000;
        let ms = e.ms % 1_000;
        let s = total_s % 60;
        let m = (total_s / 60) % 60;
        let h = (total_s / 3600) % 24;
        let level = match e.level {
            Level::ERROR => "ERROR",
            Level::WARN  => "WARN ",
            Level::INFO  => "INFO ",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };
        // Truncate the target to keep lines readable.
        let target = if e.target.len() > 30 {
            &e.target[e.target.len() - 30..]
        } else {
            &e.target
        };
        let _ = writeln!(out, "{h:02}:{m:02}:{s:02}.{ms:03} {level} {target:<30}  {}", e.message);
    }

    if entries.is_empty() {
        let _ = writeln!(out, "(no events recorded — connect and reproduce the problem first)");
    }

    out
}

/// The tracing [`Layer`] that feeds [`BUFFER`].
pub struct SessionLog;

impl<S: Subscriber> Layer<S> for SessionLog {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();

        // Skip silenced crates (passwords live there).
        let target = meta.target();
        for silenced in crate::logging::ALWAYS_SILENCED {
            if target.starts_with(silenced) {
                return;
            }
        }

        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut visitor = MessageVisitor { buf: String::new() };
        event.record(&mut visitor);

        let entry = Entry {
            ms,
            level: *meta.level(),
            target: target.to_owned(),
            message: visitor.buf,
        };

        if let Ok(mut buf) = BUFFER.lock() {
            if buf.len() >= MAX_ENTRIES {
                buf.remove(0);
            }
            buf.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_empty() {
        clear();
        let out = export("0.3.5");
        assert!(out.contains("Cipher Desk v0.3.5"));
        assert!(out.contains("no events recorded"));
    }
}
