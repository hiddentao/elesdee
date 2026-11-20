//! Typed protocol errors and the wire error codes.

use std::error::Error;
use std::fmt;

use iroh_tickets::ParseError;
use serde::{Deserialize, Serialize};

/// An error code carried in a failure response.
///
/// The first nine codes map to the local API status codes. [`Self::NotPaired`]
/// and [`Self::Revoked`] are specific to the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request is malformed or invalid.
    BadRequest,
    /// The caller may not use this method.
    Forbidden,
    /// The requested resource does not exist.
    NotFound,
    /// The request conflicts with the current state.
    Conflict,
    /// The request or payload is too large.
    TooLarge,
    /// The protocol version or media type is not supported.
    Unsupported,
    /// The request is well formed but cannot be processed.
    Unprocessable,
    /// The caller sent too many requests.
    RateLimited,
    /// The computer failed internally.
    Internal,
    /// The phone key is not in the paired device list.
    NotPaired,
    /// The paired device was revoked.
    Revoked,
}

impl ErrorCode {
    /// Returns the string used on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::TooLarge => "too_large",
            Self::Unsupported => "unsupported",
            Self::Unprocessable => "unprocessable",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal",
            Self::NotPaired => "not_paired",
            Self::Revoked => "revoked",
        }
    }

    /// Returns the local API HTTP status that this code maps to.
    ///
    /// The two link-only codes have no HTTP status; they use 403 because the
    /// connection is not authorized.
    pub const fn status(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Forbidden | Self::NotPaired | Self::Revoked => 403,
            Self::NotFound => 404,
            Self::Conflict => 409,
            Self::TooLarge => 413,
            Self::Unsupported => 415,
            Self::Unprocessable => 422,
            Self::RateLimited => 429,
            Self::Internal => 500,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error object inside a failure response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    code: ErrorCode,
    message: String,
}

impl ErrorBody {
    /// Creates an error object.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the machine-readable code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A failure to build, encode or decode a link message or ticket.
#[derive(Debug)]
#[non_exhaustive]
pub enum LinkError {
    /// The method is not on the phone allowlist.
    UnknownMethod(String),
    /// The envelope version is not supported.
    UnsupportedVersion(u8),
    /// The JSON header is longer than the limit.
    HeaderTooLarge {
        /// The observed or declared header length.
        len: usize,
        /// The maximum allowed length.
        max: usize,
    },
    /// The binary body is longer than the limit.
    BodyTooLarge {
        /// The observed or declared body length.
        len: usize,
        /// The maximum allowed length.
        max: usize,
    },
    /// The frame ended before its declared length.
    Truncated,
    /// Bytes follow the declared body.
    TrailingBytes(usize),
    /// The header is not a valid request or response.
    MalformedHeader(&'static str),
    /// The header is not valid JSON.
    Json(serde_json::Error),
    /// The pairing ticket could not be decoded.
    Ticket(ParseError),
    /// The desktop name is empty or too long.
    InvalidDesktopName,
    /// The phone name is empty or too long.
    InvalidPhoneName,
    /// An identity key must be exactly 32 bytes.
    InvalidIdentityKey,
}

impl LinkError {
    /// Returns the wire error code that this failure maps to.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::UnknownMethod(_) => ErrorCode::Forbidden,
            Self::UnsupportedVersion(_) => ErrorCode::Unsupported,
            Self::HeaderTooLarge { .. } | Self::BodyTooLarge { .. } => ErrorCode::TooLarge,
            _ => ErrorCode::BadRequest,
        }
    }
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMethod(method) => write!(f, "unknown method: {method}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported protocol version: {version}")
            }
            Self::HeaderTooLarge { len, max } => {
                write!(f, "header is {len} bytes, limit is {max}")
            }
            Self::BodyTooLarge { len, max } => {
                write!(f, "body is {len} bytes, limit is {max}")
            }
            Self::Truncated => f.write_str("frame ended early"),
            Self::TrailingBytes(count) => write!(f, "{count} bytes follow the declared body"),
            Self::MalformedHeader(reason) => write!(f, "malformed header: {reason}"),
            Self::Json(error) => write!(f, "invalid JSON header: {error}"),
            Self::Ticket(_) => f.write_str("invalid pairing ticket"),
            Self::InvalidDesktopName => f.write_str("invalid desktop name"),
            Self::InvalidPhoneName => f.write_str("invalid phone name"),
            Self::InvalidIdentityKey => f.write_str("identity key must be 32 bytes"),
        }
    }
}

impl Error for LinkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Ticket(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for LinkError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ParseError> for LinkError {
    fn from(error: ParseError) -> Self {
        Self::Ticket(error)
    }
}

/// A rejected pairing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PairingError {
    /// The pairing ticket has expired.
    Expired,
    /// The pairing ticket was already used successfully.
    Used,
    /// The pairing session was cancelled.
    Cancelled,
    /// Too many wrong secrets were presented.
    TooManyAttempts,
    /// The presented secret does not match.
    SecretMismatch,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Expired => "pairing ticket expired",
            Self::Used => "pairing ticket already used",
            Self::Cancelled => "pairing session cancelled",
            Self::TooManyAttempts => "too many wrong pairing secrets",
            Self::SecretMismatch => "pairing secret does not match",
        };
        f.write_str(message)
    }
}

impl Error for PairingError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_use_the_wire_names() {
        let pairs = [
            (ErrorCode::BadRequest, "bad_request", 400),
            (ErrorCode::Forbidden, "forbidden", 403),
            (ErrorCode::NotFound, "not_found", 404),
            (ErrorCode::Conflict, "conflict", 409),
            (ErrorCode::TooLarge, "too_large", 413),
            (ErrorCode::Unsupported, "unsupported", 415),
            (ErrorCode::Unprocessable, "unprocessable", 422),
            (ErrorCode::RateLimited, "rate_limited", 429),
            (ErrorCode::Internal, "internal", 500),
            (ErrorCode::NotPaired, "not_paired", 403),
            (ErrorCode::Revoked, "revoked", 403),
        ];

        for (code, name, status) in pairs {
            assert_eq!(code.as_str(), name);
            assert_eq!(serde_json::to_value(code).expect("code encodes"), name);
            assert_eq!(code.status(), status);
        }
    }

    #[test]
    fn error_body_round_trips() {
        let body = ErrorBody::new(ErrorCode::Conflict, "session already closed");
        let json = serde_json::to_string(&body).expect("body encodes");
        assert_eq!(
            json,
            r#"{"code":"conflict","message":"session already closed"}"#
        );
        let decoded: ErrorBody = serde_json::from_str(&json).expect("body decodes");
        assert_eq!(decoded, body);
    }

    #[test]
    fn link_error_maps_to_a_wire_code() {
        assert_eq!(
            LinkError::UnknownMethod("projects.create".to_owned()).code(),
            ErrorCode::Forbidden
        );
        assert_eq!(
            LinkError::UnsupportedVersion(2).code(),
            ErrorCode::Unsupported
        );
        assert_eq!(
            LinkError::HeaderTooLarge { len: 2, max: 1 }.code(),
            ErrorCode::TooLarge
        );
        assert_eq!(
            LinkError::BodyTooLarge { len: 2, max: 1 }.code(),
            ErrorCode::TooLarge
        );
    }
}
