//! Build and engine information shown in the app's About screen.

/// Application display name.
pub const APP_NAME: &str = "Secure RDP Viewer";

/// Version of this core library (from Cargo).
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The RDP engine this build uses.
pub const ENGINE: &str = "IronRDP (connector 0.12, session 0.13)";

/// Short one-line description, e.g. "Secure RDP Viewer 0.1.0 (IronRDP 0.17, release)".
pub fn summary() -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!("{APP_NAME} {CORE_VERSION} ({ENGINE}, {profile})")
}

/// Confirms the IronRDP engine is linked by touching one of its public types.
/// Returns the protocol flag value IronRDP uses for NLA (CredSSP, "HYBRID").
pub fn engine_nla_protocol_bits() -> u32 {
    ironrdp_pdu::nego::SecurityProtocol::HYBRID.bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_has_name_version_and_engine() {
        let s = summary();
        assert!(s.starts_with(APP_NAME));
        assert!(s.contains(CORE_VERSION));
        assert!(s.contains("IronRDP"));
    }

    #[test]
    fn engine_is_linked() {
        // MS-RDPBCGR: PROTOCOL_HYBRID = 0x00000002
        assert_eq!(engine_nla_protocol_bits(), 0x0000_0002);
    }
}
