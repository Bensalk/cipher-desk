//! Read-only information for the UI (Stage 2 smoke test: "Hello from Rust").

/// One-line build summary, e.g. "Cipher Desk 0.1.0 (IronRDP 0.17, release)".
#[flutter_rust_bridge::frb(sync)]
pub fn core_summary() -> String {
    rdpv_core::version::summary()
}

/// Greeting shown on the Stage 2 start screen to prove the bridge works.
#[flutter_rust_bridge::frb(sync)]
pub fn hello_from_rust() -> String {
    format!("Hello from Rust — {}", rdpv_core::version::summary())
}

/// Security defaults, as plain text lines for the About screen.
#[flutter_rust_bridge::frb(sync)]
pub fn security_defaults_summary() -> Vec<String> {
    let d = rdpv_core::policy::SecurityDefaults::secure();
    vec![
        format!("NLA required: {}", d.require_nla),
        format!(
            "Minimum TLS: {}.{}",
            d.min_tls_version.0, d.min_tls_version.1
        ),
        format!("Certificate trust: {:?}", d.certificate_trust),
        format!("Clipboard: {:?}", d.clipboard),
        format!("Connect timeout: {} s", d.connect_timeout.as_secs()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_mentions_rust_and_engine() {
        let h = hello_from_rust();
        assert!(h.starts_with("Hello from Rust"));
        assert!(h.contains("IronRDP"));
    }

    #[test]
    fn defaults_summary_reports_secure_values() {
        let lines = security_defaults_summary().join("\n");
        assert!(lines.contains("NLA required: true"));
        assert!(lines.contains("Clipboard: Off"));
        assert!(lines.contains("PinOnFirstUse"));
    }
}
