//! Size and count limits shared by the core service and the phone shell.

use std::time::Duration;

/// Maximum JSON header size in bytes.
pub const MAX_HEADER_BYTES: usize = 1024 * 1024;

/// Maximum size of one pairing message in bytes.
pub const MAX_PAIR_FRAME_BYTES: usize = 16 * 1024;

/// Maximum binary upload body size in bytes.
pub const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

/// Maximum UI package size in bytes.
pub const MAX_UI_PACKAGE_BYTES: usize = 64 * 1024 * 1024;

/// Maximum open streams on one phone connection.
pub const MAX_STREAMS_PER_CONNECTION: usize = 16;

/// Maximum open streams across all phones.
pub const MAX_STREAMS_TOTAL: usize = 64;

/// Maximum paired phones on one computer.
pub const MAX_PAIRED_PHONES: usize = 16;

/// Maximum connected phones on one computer.
pub const MAX_CONNECTED_PHONES: usize = 8;

/// Maximum open demo-server tunnels on one computer.
///
/// There is one tunnel per port, so this also caps the ports that may be
/// forwarded at the same time.
pub const MAX_FORWARD_TUNNELS: usize = 8;

/// Pairing ticket lifetime in seconds.
pub const PAIRING_TICKET_TTL_SECS: u64 = 120;

/// Maximum wrong pairing secrets before the session is cancelled.
pub const PAIRING_MAX_ATTEMPTS: u8 = 5;

/// Seconds the user has to confirm a pairing.
pub const CONFIRMATION_TTL_SECS: u64 = 60;

/// Maximum desktop name length in characters.
pub const DESKTOP_NAME_MAX_CHARS: usize = 64;

/// Minimum phone name length in characters.
pub const PHONE_NAME_MIN_CHARS: usize = 1;

/// Maximum phone name length in characters.
pub const PHONE_NAME_MAX_CHARS: usize = 64;

/// Timeout for the phone's `hello` message.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for a unary request.
pub const UNARY_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// QUIC keep-alive interval.
pub const QUIC_KEEP_ALIVE: Duration = Duration::from_secs(5);

/// QUIC idle timeout.
pub const QUIC_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
