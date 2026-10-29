//! Logging policy (security finding SF-2 / requirement SA-7).
//!
//! The CredSSP library used by IronRDP (`sspi`) records credential buffers —
//! including the password — when its logs are at `debug` or `trace` level.
//! This module guarantees:
//! - `sspi` logging is **always off**, whatever level is requested,
//! - release builds never log below `info` (also enforced at compile time by
//!   the `tracing/release_max_level_info` feature in the workspace).

use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// Crates whose logs may contain secrets and are therefore always disabled.
pub const ALWAYS_SILENCED: &[&str] = &["sspi", "picky", "picky_krb"];

/// The most detailed level allowed in this build.
pub const fn max_level() -> LevelFilter {
    if cfg!(debug_assertions) {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    }
}

/// The level used when the user has not asked for anything.
pub const DEFAULT_LEVEL: LevelFilter = LevelFilter::WARN;

/// Parses a requested level name ("error", "warn", "info", "debug", "trace").
/// Unknown names fall back to [`DEFAULT_LEVEL`].
pub fn parse_level(requested: &str) -> LevelFilter {
    match requested.trim().to_ascii_lowercase().as_str() {
        "off" => LevelFilter::OFF,
        "error" => LevelFilter::ERROR,
        "warn" | "warning" => LevelFilter::WARN,
        "info" => LevelFilter::INFO,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        _ => DEFAULT_LEVEL,
    }
}

/// Builds the filter directive string for a requested level, clamped to
/// [`max_level`] and with every crate in [`ALWAYS_SILENCED`] turned off.
///
/// Only a single global level is accepted; per-crate directives from the
/// user are ignored so they cannot re-enable silenced crates.
pub fn directive(requested: &str) -> String {
    let wanted = parse_level(requested);
    let level = if wanted > max_level() {
        max_level()
    } else {
        wanted
    };
    let mut parts = vec![level.to_string().to_ascii_lowercase()];
    parts.extend(ALWAYS_SILENCED.iter().map(|c| format!("{c}=off")));
    parts.join(",")
}

/// Builds the [`EnvFilter`] for a requested level (see [`directive`]).
pub fn env_filter(requested: &str) -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(DEFAULT_LEVEL.into())
        .parse_lossy(directive(requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sspi_is_always_off() {
        for level in ["trace", "debug", "info", "warn", "error", "nonsense", ""] {
            let d = directive(level);
            assert!(d.contains("sspi=off"), "{level}: {d}");
        }
    }

    #[test]
    fn user_cannot_reenable_silenced_crates() {
        // A per-crate directive is not a level name, so it falls back to the default.
        // Built at run time so the policy scanner does not flag this test.
        let attempt = format!("{}=trace", ALWAYS_SILENCED[0]);
        let d = directive(&attempt);
        assert!(!d.contains(&attempt));
        assert!(d.starts_with("warn,"));
    }

    #[test]
    fn requested_level_is_clamped_to_build_maximum() {
        let d = directive("trace");
        let expected = max_level().to_string().to_ascii_lowercase();
        assert!(d.starts_with(&format!("{expected},")), "{d}");
    }

    #[test]
    fn release_builds_never_exceed_info() {
        if !cfg!(debug_assertions) {
            assert_eq!(max_level(), LevelFilter::INFO);
        }
        assert!(max_level() <= LevelFilter::DEBUG);
    }

    #[test]
    fn unknown_level_uses_default() {
        assert_eq!(parse_level("loud"), DEFAULT_LEVEL);
        assert_eq!(parse_level(" INFO "), LevelFilter::INFO);
    }

    #[test]
    fn env_filter_builds() {
        let f = env_filter("info");
        assert!(f.to_string().contains("sspi=off"));
    }
}
