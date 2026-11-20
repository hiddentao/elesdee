//! App connection handshake and stream messages.
//!
//! The phone opens one control stream after it dials the app transport. It
//! sends [`AppHello`] and reads [`Welcome`]. Both messages use a four-byte
//! big-endian JSON length followed by that many bytes of JSON. The reader
//! checks the declared length against a cap before it allocates a buffer.

use std::fmt;

use iroh::endpoint::{RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PROTOCOL_VERSION;

/// Maximum size of one app handshake message in bytes.
pub const MAX_APP_FRAME_BYTES: usize = 16 * 1024;

/// The phone's opening message on a fresh app connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHello {
    /// Message version.
    pub v: u8,
    /// The phone shell's bridge version.
    pub bridge_version: u32,
    /// The protocol version that the phone speaks.
    pub protocol_version: u8,
    /// The phone platform, such as `android`.
    pub platform: String,
    /// The phone app version.
    pub app_version: String,
    /// The UI package build that the phone holds for this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ui_build_id: Option<String>,
}

impl AppHello {
    /// Builds a hello for this protocol version.
    pub fn new(
        bridge_version: u32,
        platform: impl Into<String>,
        app_version: impl Into<String>,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            bridge_version,
            protocol_version: PROTOCOL_VERSION,
            platform: platform.into(),
            app_version: app_version.into(),
            last_ui_build_id: None,
        }
    }

    /// Returns true when the phone and the computer speak the same protocol.
    pub fn is_compatible(&self) -> bool {
        self.v == PROTOCOL_VERSION && self.protocol_version == PROTOCOL_VERSION
    }
}

/// The computer's answer to [`AppHello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    /// Message version.
    pub v: u8,
    /// The computer endpoint id as z-base-32.
    pub computer_id: String,
    /// The computer name shown on the phone.
    pub computer_name: String,
    /// The core service version.
    pub app_version: String,
    /// The protocol version that the computer uses.
    pub protocol_version: u8,
    /// The oldest phone bridge that the computer accepts.
    pub min_bridge_version: u32,
    /// The UI package manifest, when the computer has one.
    #[serde(default)]
    pub ui: Option<Value>,
}

impl Welcome {
    /// Checks the handshake against the phone's expectations.
    ///
    /// # Errors
    ///
    /// Rejects another message version, another protocol version, a computer
    /// id that differs from the stored address and a computer that needs a
    /// newer phone app.
    pub fn validate(&self, expected_id: &str, bridge_version: u32) -> Result<(), WelcomeError> {
        if self.v != PROTOCOL_VERSION || self.protocol_version != PROTOCOL_VERSION {
            return Err(WelcomeError::Protocol);
        }
        if self.computer_id != expected_id {
            return Err(WelcomeError::ComputerMismatch);
        }
        if self.min_bridge_version > bridge_version {
            return Err(WelcomeError::PhoneTooOld);
        }
        Ok(())
    }
}

/// A welcome that the phone must not accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeError {
    /// The protocol version is not supported.
    Protocol,
    /// The computer id differs from the stored endpoint address.
    ComputerMismatch,
    /// The computer needs a newer phone app.
    PhoneTooOld,
}

impl fmt::Display for WelcomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol => f.write_str("the computer protocol is not supported"),
            Self::ComputerMismatch => f.write_str("the computer id does not match the address"),
            Self::PhoneTooOld => f.write_str("the phone app is too old"),
        }
    }
}

impl std::error::Error for WelcomeError {}

/// A failure to read or write an app handshake message.
#[derive(Debug)]
pub enum FrameError {
    /// The message is larger than the limit.
    TooLarge,
    /// The message is empty or not valid JSON.
    Malformed,
    /// The stream failed.
    Stream,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the handshake message is too large"),
            Self::Malformed => f.write_str("the handshake message is malformed"),
            Self::Stream => f.write_str("the handshake stream failed"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Reads one length-prefixed JSON handshake message.
///
/// # Errors
///
/// Rejects an empty or oversized message, a stream failure and invalid JSON.
pub async fn read_app_frame<T: DeserializeOwned>(
    recv: &mut RecvStream,
    max: usize,
) -> Result<T, FrameError> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| FrameError::Stream)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > max {
        return Err(if length == 0 {
            FrameError::Malformed
        } else {
            FrameError::TooLarge
        });
    }
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| FrameError::Stream)?;
    serde_json::from_slice(&bytes).map_err(|_| FrameError::Malformed)
}

/// Writes one length-prefixed JSON handshake message.
///
/// # Errors
///
/// Rejects an oversized message and a stream failure.
pub async fn write_app_frame<T: Serialize>(
    send: &mut SendStream,
    value: &T,
    max: usize,
) -> Result<(), FrameError> {
    let bytes = serde_json::to_vec(value).map_err(|_| FrameError::Malformed)?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(FrameError::TooLarge);
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|_| FrameError::Stream)?;
    send.write_all(&bytes).await.map_err(|_| FrameError::Stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn welcome() -> Welcome {
        Welcome {
            v: PROTOCOL_VERSION,
            computer_id: "computer".to_owned(),
            computer_name: "Studio computer".to_owned(),
            app_version: "0.1.0".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            min_bridge_version: 1,
            ui: None,
        }
    }

    #[test]
    fn a_matching_welcome_passes() {
        assert_eq!(welcome().validate("computer", 1), Ok(()));
    }

    #[test]
    fn another_computer_is_rejected() {
        assert_eq!(
            welcome().validate("other", 1),
            Err(WelcomeError::ComputerMismatch)
        );
    }

    #[test]
    fn an_old_or_new_protocol_is_rejected() {
        let mut value = welcome();
        value.protocol_version = PROTOCOL_VERSION + 1;
        assert_eq!(value.validate("computer", 1), Err(WelcomeError::Protocol));
        let mut value = welcome();
        value.v = PROTOCOL_VERSION + 1;
        assert_eq!(value.validate("computer", 1), Err(WelcomeError::Protocol));
    }

    #[test]
    fn a_newer_bridge_requirement_is_rejected() {
        let mut value = welcome();
        value.min_bridge_version = 9;
        assert_eq!(
            value.validate("computer", 1),
            Err(WelcomeError::PhoneTooOld)
        );
    }

    #[test]
    fn a_hello_carries_the_protocol_version() {
        let hello = AppHello::new(1, "android", "0.1.0");
        assert!(hello.is_compatible());
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
    }
}
