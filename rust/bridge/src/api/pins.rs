//! Certificate pins for the UI.

use rdpv_core::input::parse_endpoint;
use rdpv_core::pins::PinStore;

/// Forgets the trusted certificate for host:port (used when a connection is
/// deleted). Returns false if the pin file could not be read or written.
#[flutter_rust_bridge::frb(sync)]
pub fn forget_certificate(pins_path: String, host: String, port: u16) -> bool {
    let Ok(endpoint) = parse_endpoint(&host, &port.to_string()) else {
        return false;
    };
    PinStore::open(pins_path)
        .and_then(|mut s| s.forget(&endpoint.display()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgets_a_pin() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = dir.path().join("pins.json");
        let fp = rdpv_core::pins::fingerprint(b"cert");
        PinStore::open(&path)
            .and_then(|mut s| s.pin("pc:3389", &fp))
            .map_err(|e| e.to_string())?;
        let p = path.to_string_lossy().into_owned();
        assert!(forget_certificate(p, "PC".into(), 3389));
        let s = PinStore::open(&path).map_err(|e| e.to_string())?;
        assert!(s.is_empty());
        Ok(())
    }
}
