//! Secure defaults (approved security measures S1–S13, SA-1–SA-9).
//!
//! Every new connection profile starts from [`SecurityDefaults::secure()`].
//! Settings the user may relax are documented; settings marked "fixed" are
//! not exposed to the user at all.

use core::time::Duration;

/// How the server's TLS certificate is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateTrust {
    /// Trust-on-first-use: the user approves the fingerprint once, then it is
    /// pinned. A changed certificate blocks the connection (S2).
    PinOnFirstUse,
    /// Validate against the operating system's trusted certificate authorities
    /// (for hosts with a CA-issued certificate), and also pin.
    SystemTrustAndPin,
}
// Note: there is intentionally no "accept any certificate" variant (SA-6).

/// Which way clipboard text may flow (S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardDirection {
    Off,
    LocalToHost,
    HostToLocal,
    Both,
}

/// Security-relevant defaults for a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityDefaults {
    /// Fixed: NLA (CredSSP) is required (S1).
    pub require_nla: bool,
    /// Fixed: legacy "standard RDP security" is refused (S1).
    pub allow_legacy_rdp_security: bool,
    /// Fixed: minimum TLS version is 1.2 (S1).
    pub min_tls_version: (u8, u8),
    /// S2 / SA-6.
    pub certificate_trust: CertificateTrust,
    /// S3 / S6: clipboard is opt-in.
    pub clipboard: ClipboardDirection,
    /// S6: largest clipboard text accepted, in bytes.
    pub clipboard_max_bytes: usize,
    /// S3: device redirection is off.
    pub redirect_drives: bool,
    pub redirect_printers: bool,
    pub redirect_usb: bool,
    pub redirect_audio: bool,
    /// SF-3: every network step has a timeout.
    pub connect_timeout: Duration,
    /// S7: idle disconnect.
    pub idle_disconnect: Duration,
    /// S5 / S13: OS authentication before a saved password is used.
    pub require_unlock_for_saved_password: bool,
    /// SA-8: never auto-retry after an authentication failure.
    pub retry_after_auth_failure: bool,
    /// F13: maximum automatic reconnect attempts after a network drop.
    pub max_auto_reconnects: u8,
}

impl SecurityDefaults {
    /// The secure starting point for every profile.
    pub const fn secure() -> Self {
        Self {
            require_nla: true,
            allow_legacy_rdp_security: false,
            min_tls_version: (1, 2),
            certificate_trust: CertificateTrust::PinOnFirstUse,
            clipboard: ClipboardDirection::Off,
            clipboard_max_bytes: 1024 * 1024,
            redirect_drives: false,
            redirect_printers: false,
            redirect_usb: false,
            redirect_audio: false,
            connect_timeout: Duration::from_secs(10),
            idle_disconnect: Duration::from_secs(30 * 60),
            require_unlock_for_saved_password: true,
            retry_after_auth_failure: false,
            max_auto_reconnects: 3,
        }
    }
}

impl Default for SecurityDefaults {
    fn default() -> Self {
        Self::secure()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_secure() {
        let d = SecurityDefaults::default();
        assert!(d.require_nla);
        assert!(!d.allow_legacy_rdp_security);
        assert!(d.min_tls_version >= (1, 2));
        assert_eq!(d.certificate_trust, CertificateTrust::PinOnFirstUse);
        assert_eq!(d.clipboard, ClipboardDirection::Off);
        assert!(!(d.redirect_drives || d.redirect_printers || d.redirect_usb || d.redirect_audio));
        assert!(d.require_unlock_for_saved_password);
        assert!(!d.retry_after_auth_failure);
    }

    #[test]
    fn timeouts_are_bounded() {
        let d = SecurityDefaults::secure();
        assert!(d.connect_timeout > Duration::ZERO);
        assert!(d.connect_timeout <= Duration::from_secs(60));
        assert!(d.idle_disconnect > Duration::ZERO);
        assert!(d.max_auto_reconnects <= 5);
    }

    #[test]
    fn clipboard_cap_is_reasonable() {
        let d = SecurityDefaults::secure();
        assert!(d.clipboard_max_bytes > 0 && d.clipboard_max_bytes <= 16 * 1024 * 1024);
    }
}
