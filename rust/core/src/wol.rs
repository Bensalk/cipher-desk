//! Wake-on-LAN (F11) and a reachability check for the unattended-ready
//! checklist (F12).
//!
//! Wake-on-LAN only works on the same local network (or through a VPN that
//! carries broadcasts); it sends no credentials or other data.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs as _, UdpSocket};
use std::time::Duration;

/// Errors for Wake-on-LAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WolError {
    #[error("enter a MAC address like 00:11:22:33:44:55")]
    InvalidMac,
    #[error("the wake-up packet could not be sent")]
    SendFailed,
}

/// Parses "00:11:22:33:44:55", "00-11-22-33-44-55" or "001122334455".
pub fn parse_mac(text: &str) -> Result<[u8; 6], WolError> {
    let hex: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ':' | '-' | '.'))
        .collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(WolError::InvalidMac);
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| WolError::InvalidMac)?;
    }
    if mac == [0; 6] || mac == [0xFF; 6] {
        return Err(WolError::InvalidMac);
    }
    Ok(mac)
}

/// The standard magic packet: 6 × 0xFF then the MAC 16 times (102 bytes).
pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFF; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Sends the magic packet to `target` (normally 255.255.255.255:9).
pub fn wake(mac_text: &str, target: SocketAddr) -> Result<(), WolError> {
    let mac = parse_mac(mac_text)?;
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_| WolError::SendFailed)?;
    socket
        .set_broadcast(true)
        .map_err(|_| WolError::SendFailed)?;
    let packet = magic_packet(mac);
    // Sent three times, as UDP may drop packets.
    for _ in 0..3 {
        socket
            .send_to(&packet, target)
            .map_err(|_| WolError::SendFailed)?;
    }
    Ok(())
}

/// Default broadcast target for Wake-on-LAN.
pub fn default_target() -> SocketAddr {
    SocketAddr::from(([255, 255, 255, 255], 9))
}

/// True if something accepts TCP connections on host:port within `timeout`.
/// Sends no data.
pub fn reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs
        .take(3)
        .any(|a| TcpStream::connect_timeout(&a, timeout).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mac_formats() {
        let mac = [0x00, 0x11, 0x22, 0xAA, 0xbb, 0xCC];
        assert_eq!(parse_mac("00:11:22:aa:bb:cc"), Ok(mac));
        assert_eq!(parse_mac("00-11-22-AA-BB-CC"), Ok(mac));
        assert_eq!(parse_mac(" 001122aabbcc "), Ok(mac));
        for bad in [
            "",
            "00:11:22:33:44",
            "zz:11:22:33:44:55",
            "00:00:00:00:00:00",
            "ff:ff:ff:ff:ff:ff",
        ] {
            assert_eq!(parse_mac(bad), Err(WolError::InvalidMac), "{bad}");
        }
    }

    #[test]
    fn magic_packet_layout() {
        let mac = [1, 2, 3, 4, 5, 6];
        let p = magic_packet(mac);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert!(p[6..].chunks(6).all(|c| c == mac));
    }

    #[test]
    fn sends_the_packet() -> Result<(), String> {
        let listener = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let target = listener.local_addr().map_err(|e| e.to_string())?;
        wake("00:11:22:33:44:55", target).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 200];
        let (n, _) = listener.recv_from(&mut buf).map_err(|e| e.to_string())?;
        assert_eq!(
            &buf[..n],
            magic_packet([0, 0x11, 0x22, 0x33, 0x44, 0x55]).as_slice()
        );
        Ok(())
    }

    #[test]
    fn reachability() -> Result<(), String> {
        let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = l.local_addr().map_err(|e| e.to_string())?.port();
        assert!(reachable("127.0.0.1", port, Duration::from_secs(1)));
        drop(l);
        assert!(!reachable("127.0.0.1", port, Duration::from_millis(500)));
        Ok(())
    }
}
