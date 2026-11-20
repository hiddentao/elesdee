//! The demo-server forwarding handshake.
//!
//! The phone opens one bidirectional stream for each tunnel and sends the
//! requested loopback port as two big-endian bytes. The computer answers with
//! one status byte before the stream carries the tunnel's bytes. Only a port
//! from the registered demo-server list is accepted.
//!
//! The handshake is deliberately small. The request cannot carry an address,
//! so a phone cannot ask the computer to connect anywhere except the loopback
//! port that the agent registered.

/// Number of bytes in the port request.
pub const FORWARD_REQUEST_BYTES: usize = 2;

/// The computer opened the tunnel.
pub const FORWARD_OK: u8 = 0;

/// The port is not in the registered demo-server list.
pub const FORWARD_REFUSED: u8 = 1;

/// The computer approved the port but nothing listens on it.
pub const FORWARD_UNAVAILABLE: u8 = 2;

/// The lowest port that a demo server may register.
///
/// Privileged ports are refused so a demo server cannot expose system
/// services through the phone link.
pub const MIN_DEMO_PORT: u16 = 1024;

/// Encodes a port for the forward handshake.
pub const fn encode_port(port: u16) -> [u8; FORWARD_REQUEST_BYTES] {
    port.to_be_bytes()
}

/// Decodes a port from the forward handshake.
///
/// Returns `None` for a port outside the demo-server range.
pub fn decode_port(bytes: [u8; FORWARD_REQUEST_BYTES]) -> Option<u16> {
    let port = u16::from_be_bytes(bytes);
    port_allowed(port).then_some(port)
}

/// Returns true when the port may hold a demo server.
pub const fn port_allowed(port: u16) -> bool {
    port >= MIN_DEMO_PORT
}

/// Returns the local address shown for a demo server.
pub fn local_address(port: u16) -> String {
    format!("http://localhost:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_round_trips_through_the_handshake() {
        assert_eq!(decode_port(encode_port(3000)), Some(3000));
        assert_eq!(decode_port(encode_port(u16::MAX)), Some(u16::MAX));
    }

    #[test]
    fn privileged_and_empty_ports_are_refused() {
        assert!(!port_allowed(0));
        assert!(!port_allowed(80));
        assert!(!port_allowed(1023));
        assert!(port_allowed(1024));
        assert!(port_allowed(3000));
        assert_eq!(decode_port(encode_port(22)), None);
        assert_eq!(decode_port(encode_port(0)), None);
    }

    #[test]
    fn the_local_address_uses_the_same_port() {
        assert_eq!(local_address(3000), "http://localhost:3000");
    }

    #[test]
    fn the_forward_handshake_has_distinct_status_bytes() {
        let statuses = [FORWARD_OK, FORWARD_REFUSED, FORWARD_UNAVAILABLE];
        let mut unique = statuses.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), statuses.len());
    }
}
