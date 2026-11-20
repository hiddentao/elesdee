//! Shared protocol crate for the elesdee phone link.
//!
//! The core service and the phone shell must agree on the device identity, the
//! pairing ticket and session, the framed request and response envelopes, the
//! method allowlist, the limits and the error codes. This crate owns those
//! pieces. Transport setup, request dispatch and UI delivery live in the core
//! service and the phone shell.
//!
//! The protocol version is part of the app ALPN. An incompatible change gets a
//! new ALPN, so an old phone shows an update message instead of a parse error.
#![deny(missing_docs)]

mod app;
mod client;
mod error;
mod forward;
mod framing;
mod identity;
mod limits;
mod method;
mod pairing;
mod reconnect;
mod ticket;

pub use app::{
    AppHello, FrameError, MAX_APP_FRAME_BYTES, Welcome, WelcomeError, read_app_frame,
    write_app_frame,
};
pub use client::{
    AppConnection, BRIDGE_VERSION, ClientError, ComputerRecord, EventStream, HistoryStream,
    LinkClient, OutputStream, PairReply, PairingCancel, PairingConfirm, PairingMachine,
    PairingRejection, PairingStart, PairingState, PhoneStore, StoreError, load_or_create_identity,
};
pub use error::{ErrorBody, ErrorCode, LinkError, PairingError};
pub use forward::{
    FORWARD_OK, FORWARD_REFUSED, FORWARD_REQUEST_BYTES, FORWARD_UNAVAILABLE, ForwardConnection,
    ForwardError, ForwardTunnel, MIN_DEMO_PORT, decode_port, encode_port, local_address,
    phone_address, port_allowed,
};
pub use framing::{Request, Response, StreamEvent};
pub use identity::DeviceIdentity;
pub use limits::{
    CONFIRMATION_TTL_SECS, DESKTOP_NAME_MAX_CHARS, HELLO_TIMEOUT, MAX_CONNECTED_PHONES,
    MAX_FORWARD_TUNNELS, MAX_HEADER_BYTES, MAX_IMAGE_BYTES, MAX_PAIR_FRAME_BYTES,
    MAX_PAIRED_PHONES, MAX_STREAMS_PER_CONNECTION, MAX_STREAMS_TOTAL, MAX_UI_PACKAGE_BYTES,
    MAX_UPLOAD_BYTES, PAIRING_MAX_ATTEMPTS, PAIRING_TICKET_TTL_SECS, PHONE_NAME_MAX_CHARS,
    PHONE_NAME_MIN_CHARS, QUIC_IDLE_TIMEOUT, QUIC_KEEP_ALIVE, UNARY_REQUEST_TIMEOUT,
};
pub use method::Method;
pub use pairing::{PairingSession, pairing_code};
pub use reconnect::{RECONNECT_DELAYS, RECONNECT_RESET, ReconnectPolicy, is_terminal, next_retry};
pub use ticket::{PAIR_TICKET_KIND, PAIR_TICKET_VERSION, PairTicket};

pub use iroh::{EndpointAddr, EndpointId, PublicKey, SecretKey};

/// Message format version. It is owned by this crate.
pub const PROTOCOL_VERSION: u8 = 1;

/// ALPN for the one-time pairing transport.
pub const PAIR_ALPN: &[u8] = b"elesdee/pair/1";

/// ALPN for requests, events and session output.
pub const APP_ALPN: &[u8] = b"elesdee/app/1";

/// ALPN for demo-server TCP forwarding.
pub const FORWARD_ALPN: &[u8] = b"elesdee/forward/1";

/// Connection close code: the phone key is not in the paired device list.
pub const CLOSE_NOT_PAIRED: u32 = 2;

/// Connection close code: the paired device was revoked.
pub const CLOSE_REVOKED: u32 = 3;
