//! Pairing handshake messages and framing.
//!
//! A pairing stream carries one request and up to two replies. Each message is
//! a four-byte big-endian JSON length followed by that many bytes of JSON.

use std::fmt;

use data_encoding::BASE32_NOPAD;
use elesdee_link::{EndpointId, PROTOCOL_VERSION};
use iroh::endpoint::{RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::state::PairedDevice;

/// Maximum size of one pairing message.
pub const MAX_PAIR_FRAME_BYTES: usize = 16 * 1024;

/// The smallest bridge version this computer accepts.
pub const MIN_BRIDGE_VERSION: u32 = 1;

/// A phone's one-time pairing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    /// Message version.
    pub v: u8,
    /// The one-time secret from the ticket, as base32 without padding.
    pub secret: String,
    /// The name the phone shows on the computer.
    pub name: String,
    /// The phone's platform, such as `android`.
    pub platform: String,
    /// The phone shell's bridge version.
    pub bridge_version: u32,
    /// The oldest protocol version the phone supports.
    pub protocol_min: u8,
    /// The newest protocol version the phone supports.
    pub protocol_max: u8,
}

impl PairRequest {
    /// Builds a request from a ticket secret.
    pub fn new(secret: &[u8; 32], name: impl Into<String>, platform: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            secret: BASE32_NOPAD.encode(secret),
            name: name.into(),
            platform: platform.into(),
            bridge_version: MIN_BRIDGE_VERSION,
            protocol_min: PROTOCOL_VERSION,
            protocol_max: PROTOCOL_VERSION,
        }
    }

    /// Decodes the secret, checking its length.
    ///
    /// # Errors
    ///
    /// Returns [`PairReject::InvalidRequest`] when the text is not base32 for
    /// exactly 32 bytes.
    pub fn secret_bytes(&self) -> Result<[u8; 32], PairReject> {
        let bytes = BASE32_NOPAD
            .decode(self.secret.to_ascii_uppercase().as_bytes())
            .map_err(|_| PairReject::InvalidRequest("The pairing secret is not valid."))?;
        bytes
            .try_into()
            .map_err(|_| PairReject::InvalidRequest("The pairing secret has the wrong length."))
    }

    /// Returns true when the computer's protocol version is in the phone range.
    pub fn protocol_overlaps(&self) -> bool {
        self.protocol_min <= self.protocol_max
            && self.protocol_min <= PROTOCOL_VERSION
            && PROTOCOL_VERSION <= self.protocol_max
    }
}

/// A reply in the pairing handshake.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairReply {
    /// Message version.
    pub v: u8,
    /// True when the step succeeded.
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The step name, such as `confirm` or `paired`.
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The six-digit code that both devices show.
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The computer identity as z-base-32.
    pub computer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The computer name to show on the phone.
    pub computer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The protocol version the computer uses.
    pub protocol_version: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The smallest phone bridge version the computer supports.
    pub min_bridge_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// A short reason for a rejection.
    pub message: Option<String>,
}

impl PairReply {
    /// Builds the confirmation step that carries the code.
    pub fn confirm(code: String, computer_id: String, computer_name: String) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ok: true,
            status: Some("confirm".to_owned()),
            code: Some(code),
            computer_id: Some(computer_id),
            computer_name: Some(computer_name),
            protocol_version: Some(PROTOCOL_VERSION),
            min_bridge_version: Some(MIN_BRIDGE_VERSION),
            message: None,
        }
    }

    /// Builds the final success after the user approves.
    pub fn paired() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ok: true,
            status: Some("paired".to_owned()),
            ..Self::default()
        }
    }

    /// Builds a rejection with a status and a safe message.
    pub fn rejected(status: &str, message: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ok: false,
            status: Some(status.to_owned()),
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

/// The user's answer to a confirmation request.
#[derive(Debug, Clone)]
pub enum PairDecision {
    /// The user approved the phone.
    Approved(PairedDevice),
    /// The user denied the phone, or the request expired.
    Denied,
}

/// A rejected pairing request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairReject {
    /// The endpoint is not ready.
    NotReady,
    /// The ticket has expired.
    Expired,
    /// The ticket was already used.
    Used,
    /// The pairing session was cancelled or replaced.
    Cancelled,
    /// Too many wrong secrets were presented.
    TooManyAttempts,
    /// The secret does not match.
    SecretMismatch,
    /// The request is malformed or unsupported.
    InvalidRequest(&'static str),
    /// The phone and computer have no protocol version in common.
    ProtocolMismatch,
    /// The phone name is empty, too long or only invisible characters.
    InvalidName,
    /// The paired device list is full.
    DeviceLimit,
}

impl PairReject {
    /// Returns the status name used on the wire.
    pub fn status(&self) -> &'static str {
        match self {
            Self::NotReady => "not_ready",
            Self::Expired => "expired",
            Self::Used => "used",
            Self::Cancelled => "cancelled",
            Self::TooManyAttempts => "too_many_attempts",
            Self::SecretMismatch => "rejected",
            Self::InvalidRequest(_) => "invalid",
            Self::ProtocolMismatch => "protocol",
            Self::InvalidName => "invalid_name",
            Self::DeviceLimit => "device_limit",
        }
    }

    /// Returns a message that does not expose the secret.
    pub fn message(&self) -> String {
        match self {
            Self::NotReady => "The computer is not ready for pairing.".to_owned(),
            Self::Expired => "The pairing code expired. Show a new code.".to_owned(),
            Self::Used => "This pairing code was already used.".to_owned(),
            Self::Cancelled => "Pairing was cancelled.".to_owned(),
            Self::TooManyAttempts => "Too many wrong codes. Show a new code.".to_owned(),
            Self::SecretMismatch => "The pairing secret does not match.".to_owned(),
            Self::InvalidRequest(reason) => (*reason).to_owned(),
            Self::ProtocolMismatch => {
                "Update elesdee on this computer or the phone app.".to_owned()
            }
            Self::InvalidName => "The phone name is not valid.".to_owned(),
            Self::DeviceLimit => "This computer reached its paired phone limit.".to_owned(),
        }
    }

    /// Builds the reply for this rejection.
    pub fn reply(&self) -> PairReply {
        PairReply::rejected(self.status(), self.message())
    }
}

impl fmt::Display for PairReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// A failure to read or write a pairing frame.
#[derive(Debug)]
pub enum WireError {
    /// The message is larger than the limit.
    TooLarge,
    /// The message is empty or not valid JSON.
    Malformed,
    /// The stream failed.
    Stream,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("pairing message is too large"),
            Self::Malformed => f.write_str("pairing message is malformed"),
            Self::Stream => f.write_str("pairing stream failed"),
        }
    }
}

impl std::error::Error for WireError {}

/// Reads one length-prefixed JSON message.
///
/// # Errors
///
/// Rejects an empty or oversized message, a stream failure and invalid JSON.
pub async fn read_pair_frame<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T, WireError> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| WireError::Stream)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_PAIR_FRAME_BYTES {
        return Err(if length == 0 {
            WireError::Malformed
        } else {
            WireError::TooLarge
        });
    }
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| WireError::Stream)?;
    serde_json::from_slice(&bytes).map_err(|_| WireError::Malformed)
}

/// Writes one length-prefixed JSON message.
///
/// # Errors
///
/// Rejects an oversized message and a stream failure.
pub async fn write_pair_frame<T: Serialize>(
    send: &mut SendStream,
    value: &T,
) -> Result<(), WireError> {
    let bytes = serde_json::to_vec(value).map_err(|_| WireError::Malformed)?;
    if bytes.is_empty() || bytes.len() > MAX_PAIR_FRAME_BYTES {
        return Err(WireError::TooLarge);
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|_| WireError::Stream)?;
    send.write_all(&bytes).await.map_err(|_| WireError::Stream)
}

/// The identity encoded in a pairing request.
pub type PhoneId = EndpointId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_round_trips_as_base32() {
        let secret = [7u8; 32];
        let request = PairRequest::new(&secret, "Phone", "android");
        assert_eq!(request.secret_bytes().expect("secret"), secret);
        assert_eq!(request.secret.len(), 52);
        assert!(request.protocol_overlaps());
    }

    #[test]
    fn invalid_secret_is_rejected() {
        let mut request = PairRequest::new(&[1u8; 32], "Phone", "android");
        request.secret = "not-base32".to_owned();
        assert!(matches!(
            request.secret_bytes(),
            Err(PairReject::InvalidRequest(_))
        ));
    }

    #[test]
    fn protocol_range_must_include_the_computer_version() {
        let mut request = PairRequest::new(&[1u8; 32], "Phone", "android");
        request.protocol_min = PROTOCOL_VERSION + 1;
        request.protocol_max = PROTOCOL_VERSION + 1;
        assert!(!request.protocol_overlaps());
    }

    #[test]
    fn rejection_replies_hide_the_secret() {
        let reply = PairReject::SecretMismatch.reply();
        assert!(!reply.ok);
        assert_eq!(reply.status.as_deref(), Some("rejected"));
        assert!(!reply.message.unwrap_or_default().contains("base32"));
    }
}
