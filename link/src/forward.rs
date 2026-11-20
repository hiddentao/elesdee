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

use std::fmt;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};

use crate::client::{ClientError, classify_closed};
use crate::limits::UNARY_REQUEST_TIMEOUT;

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

/// Returns the local address shown for a demo server on the computer.
pub fn local_address(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// Returns the loopback address that the phone shows for its listener.
///
/// The phone binds IPv4 loopback only, so the address names that address
/// directly. A name may resolve to IPv6 on some phones and miss the listener.
pub fn phone_address(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// A phone-side connection to one paired computer for demo-server tunnels.
///
/// The shell gets one from [`crate::LinkClient::forward`]. Each call to
/// [`Self::open`] adds one tunnel stream, so the connection may carry several
/// demo servers at once.
#[derive(Clone)]
pub struct ForwardConnection {
    connection: Connection,
}

impl fmt::Debug for ForwardConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardConnection")
            .field("computer", &self.connection.remote_id().to_z32())
            .finish_non_exhaustive()
    }
}

impl ForwardConnection {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Opens one tunnel for a registered port.
    ///
    /// Sends the port and waits for the computer's status byte. A refused or
    /// unavailable port returns an error before any bytes are forwarded.
    ///
    /// # Errors
    ///
    /// Returns a refusal, an unavailable port, a timeout or a transport
    /// failure.
    pub async fn open(&self, port: u16) -> Result<ForwardTunnel, ForwardError> {
        let result = tokio::time::timeout(UNARY_REQUEST_TIMEOUT, async {
            let (mut send, mut recv) = self
                .connection
                .open_bi()
                .await
                .map_err(|error| ForwardError::Client(classify_closed(&error)))?;
            send.write_all(&encode_port(port))
                .await
                .map_err(|_| ForwardError::Client(transport()))?;
            let mut status = [0u8; 1];
            recv.read_exact(&mut status)
                .await
                .map_err(|_| ForwardError::Client(transport()))?;
            match status[0] {
                FORWARD_OK => Ok(ForwardTunnel { send, recv }),
                FORWARD_REFUSED => Err(ForwardError::Refused),
                FORWARD_UNAVAILABLE => Err(ForwardError::Unavailable),
                _ => Err(ForwardError::Client(ClientError::Protocol(
                    "the forward status is not valid",
                ))),
            }
        })
        .await;
        result.unwrap_or(Err(ForwardError::Timeout))
    }

    /// Returns true when the computer closed the connection.
    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    /// Waits for the computer to close the connection.
    ///
    /// The result classifies a revocation, an unknown device and an ordinary
    /// drop.
    pub async fn wait_closed(&self) -> ClientError {
        classify_closed(&self.connection.closed().await)
    }

    /// Closes the connection and every tunnel on it.
    pub fn close(&self) {
        self.connection.close(VarInt::from_u32(0), b"closed");
    }
}

/// One open tunnel stream.
///
/// The stream carries raw bytes in both directions until either side closes
/// it. [`Self::split`] returns the two halves for a bidirectional copy.
pub struct ForwardTunnel {
    send: SendStream,
    recv: RecvStream,
}

impl fmt::Debug for ForwardTunnel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardTunnel")
            .finish_non_exhaustive()
    }
}

impl ForwardTunnel {
    /// Splits the tunnel into its send and receive halves.
    pub fn split(self) -> (SendStream, RecvStream) {
        (self.send, self.recv)
    }
}

fn transport() -> ClientError {
    ClientError::Transport("the forward stream failed".to_owned())
}

/// A failure to open a demo-server tunnel from the phone.
#[derive(Debug)]
#[non_exhaustive]
pub enum ForwardError {
    /// The computer does not have that port in its active list.
    Refused,
    /// The computer approved the port but no server listens on it.
    Unavailable,
    /// The computer did not answer in time.
    Timeout,
    /// The link connection failed.
    Client(ClientError),
}

impl fmt::Display for ForwardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => formatter.write_str("the computer refused the demo-server port"),
            Self::Unavailable => formatter.write_str("no demo server listens on the port"),
            Self::Timeout => formatter.write_str("the forward request timed out"),
            Self::Client(error) => write!(formatter, "the forward link failed: {error}"),
        }
    }
}

impl std::error::Error for ForwardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            _ => None,
        }
    }
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
    fn the_phone_address_names_ipv4_loopback() {
        assert_eq!(phone_address(3000), "http://127.0.0.1:3000");
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
