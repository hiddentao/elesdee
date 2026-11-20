//! The pairing ticket carried in the QR code.

use std::fmt;

use iroh::EndpointAddr;
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::LinkError;
use crate::limits::DESKTOP_NAME_MAX_CHARS;

/// Ticket kind. It is the prefix of the string form.
pub const PAIR_TICKET_KIND: &str = "elesdee-pair";

/// Version of the ticket body.
pub const PAIR_TICKET_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
struct TicketBody {
    v: u8,
    addr: EndpointAddr,
    secret: [u8; 32],
    desktop_name: String,
    expires_at: u64,
}

/// The one-time pairing ticket that the phone reads from the QR code.
///
/// The body is postcard encoded. The string form is the kind plus base32, as
/// [`Ticket`] defines. The secret is cleared from memory when the ticket is
/// dropped, and the [`fmt::Debug`] output never shows it.
pub struct PairTicket {
    body: TicketBody,
}

impl PairTicket {
    /// Creates a ticket for one pairing attempt.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::InvalidDesktopName`] when the desktop name is empty
    /// or over the character limit.
    pub fn new(
        addr: EndpointAddr,
        mut secret: [u8; 32],
        desktop_name: impl Into<String>,
        expires_at: u64,
    ) -> Result<Self, LinkError> {
        let desktop_name = desktop_name.into();
        if !valid_name(&desktop_name) {
            secret.zeroize();
            return Err(LinkError::InvalidDesktopName);
        }
        Ok(Self {
            body: TicketBody {
                v: PAIR_TICKET_VERSION,
                addr,
                secret,
                desktop_name,
                expires_at,
            },
        })
    }

    /// Returns the ticket body version.
    pub fn version(&self) -> u8 {
        self.body.v
    }

    /// Returns the desktop endpoint address.
    pub fn addr(&self) -> &EndpointAddr {
        &self.body.addr
    }

    /// Returns the one-time secret.
    pub fn secret(&self) -> &[u8; 32] {
        &self.body.secret
    }

    /// Returns the desktop name.
    pub fn desktop_name(&self) -> &str {
        &self.body.desktop_name
    }

    /// Returns the expiry hint as Unix seconds.
    pub fn expires_at(&self) -> u64 {
        self.body.expires_at
    }

    /// Returns true when the ticket has expired at the given Unix time.
    ///
    /// The expiry is a hint for the phone. The computer makes the final check.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.body.expires_at
    }
}

impl Ticket for PairTicket {
    const KIND: &'static str = PAIR_TICKET_KIND;

    fn encode_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(&self.body).expect("a pair ticket is always serializable")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut body: TicketBody = postcard::from_bytes(bytes)?;
        if body.v != PAIR_TICKET_VERSION {
            body.secret.zeroize();
            return Err(ParseError::verification_failed(
                "unsupported pair ticket version",
            ));
        }
        if !valid_name(&body.desktop_name) {
            body.secret.zeroize();
            return Err(ParseError::verification_failed("invalid desktop name"));
        }
        Ok(Self { body })
    }
}

impl fmt::Debug for PairTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairTicket")
            .field("v", &self.body.v)
            .field("addr", &self.body.addr)
            .field("secret", &"<redacted>")
            .field("desktop_name", &self.body.desktop_name)
            .field("expires_at", &self.body.expires_at)
            .finish()
    }
}

impl Drop for PairTicket {
    fn drop(&mut self) {
        self.body.secret.zeroize();
    }
}

fn valid_name(name: &str) -> bool {
    let len = name.chars().count();
    (1..=DESKTOP_NAME_MAX_CHARS).contains(&len)
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;
    use iroh_tickets::Ticket;

    use super::*;

    fn address() -> EndpointAddr {
        EndpointAddr::new(SecretKey::from_bytes(&[7; 32]).public())
    }

    fn ticket() -> PairTicket {
        PairTicket::new(address(), [0xAB; 32], "Studio computer", 1_000).expect("valid ticket")
    }

    #[test]
    fn ticket_round_trips_through_the_string_form() {
        let original = ticket();
        let encoded = original.encode_string();
        assert!(encoded.starts_with(PAIR_TICKET_KIND));

        let decoded = PairTicket::decode_string(&encoded).expect("ticket decodes");
        assert_eq!(decoded.version(), PAIR_TICKET_VERSION);
        assert_eq!(decoded.addr(), original.addr());
        assert_eq!(decoded.secret(), original.secret());
        assert_eq!(decoded.desktop_name(), "Studio computer");
        assert_eq!(decoded.expires_at(), 1_000);
    }

    #[test]
    fn ticket_bytes_round_trip() {
        let original = ticket();
        let bytes = original.encode_bytes();
        let decoded = <PairTicket as Ticket>::decode_bytes(&bytes).expect("ticket decodes");
        assert_eq!(decoded.addr(), original.addr());
        assert_eq!(decoded.secret(), original.secret());
    }

    #[test]
    fn tampered_ticket_is_rejected() {
        let encoded = ticket().encode_string();
        let truncated = &encoded[..encoded.len() - 1];
        assert!(PairTicket::decode_string(truncated).is_err());
        assert!(PairTicket::decode_string("not-a-ticket").is_err());
    }

    #[test]
    fn unsupported_ticket_version_is_rejected() {
        let body = TicketBody {
            v: 2,
            addr: address(),
            secret: [1; 32],
            desktop_name: "Studio computer".to_owned(),
            expires_at: 1_000,
        };
        let bytes = postcard::to_allocvec(&body).expect("body encodes");
        assert!(<PairTicket as Ticket>::decode_bytes(&bytes).is_err());
    }

    #[test]
    fn invalid_desktop_name_is_rejected() {
        assert!(matches!(
            PairTicket::new(address(), [1; 32], "", 1_000),
            Err(LinkError::InvalidDesktopName)
        ));
        assert!(matches!(
            PairTicket::new(
                address(),
                [1; 32],
                "x".repeat(DESKTOP_NAME_MAX_CHARS + 1),
                1_000
            ),
            Err(LinkError::InvalidDesktopName)
        ));
    }

    #[test]
    fn expiry_is_inclusive_of_the_deadline() {
        let ticket = ticket();
        assert!(!ticket.is_expired(999));
        assert!(ticket.is_expired(1_000));
        assert!(ticket.is_expired(1_001));
    }

    #[test]
    fn debug_output_redacts_the_secret() {
        let text = format!("{:?}", ticket());
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("abab"));
    }
}
