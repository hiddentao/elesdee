//! Demo servers that a session registers for the phone to open.
//!
//! A demo server is a process that the agent starts in the session. The app
//! records only its loopback port, a name and its state. The Mac opens the
//! local address directly. A paired phone may forward a registered port over
//! the link; it cannot forward any other port.
//!
//! Registrations belong to one session. A port is registered to at most one
//! active session, and closing or pausing a session stops its servers. They
//! are kept in memory with the session and are not shared between sessions.

use std::fmt;

use elesdee_link::{MIN_DEMO_PORT, local_address};
use serde::{Deserialize, Serialize};

use crate::text::clean_label;

/// Maximum demo servers that one session may register.
pub const MAX_DEMO_SERVERS: usize = 8;

/// Maximum characters in a demo server name.
pub const MAX_DEMO_NAME_CHARS: usize = 64;

/// The state of one registered demo server.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DemoServerState {
    /// The port is registered and may be opened or forwarded.
    Active,
    /// The session stopped the server. It must be registered again to open it.
    Stopped,
}

/// A demo server registered by a session.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoServer {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub port: u16,
    /// The local address, such as `http://localhost:3000`.
    pub address: String,
    /// The tailnet address, when Tailscale was running at registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_address: Option<String>,
    /// The public HTTPS address while Tailscale Funnel exposes the port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funnel_address: Option<String>,
    pub state: DemoServerState,
    pub created_at: String,
}

impl DemoServer {
    /// Returns true when the port may be opened or forwarded.
    pub fn is_active(&self) -> bool {
        self.state == DemoServerState::Active
    }

    /// Returns true when Tailscale Funnel exposes the port to the internet.
    pub fn is_public(&self) -> bool {
        self.funnel_address.is_some()
    }
}

/// Why a demo server registration or stop failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoFailure {
    /// The session does not exist.
    UnknownSession,
    /// The session is closed.
    Closed,
    /// The name is empty after cleaning.
    InvalidName,
    /// The port is outside the allowed range.
    InvalidPort,
    /// The port is the core service's own port.
    ReservedPort,
    /// The port is already registered by another active session.
    PortInUse,
    /// The session reached its demo-server limit.
    TooMany,
    /// The demo server is stopped.
    Stopped,
    /// No demo server has that id in this session.
    NotFound,
}

impl fmt::Display for DemoFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownSession => "Unknown session.",
            Self::Closed => "The session is closed.",
            Self::InvalidName => "The demo server name is required.",
            Self::InvalidPort => {
                "The port must be between 1024 and 65535. Privileged ports are refused."
            }
            Self::ReservedPort => "That port belongs to the elesdee service.",
            Self::PortInUse => "Another session registered that port.",
            Self::TooMany => "A session may register at most 8 demo servers.",
            Self::Stopped => "The demo server is stopped. Register it again first.",
            Self::NotFound => "Unknown demo server.",
        })
    }
}

/// Why a public address change failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FunnelFailure {
    /// The demo-server rule refused the change.
    Demo(DemoFailure),
    /// The request did not carry the user's confirmation.
    NotConfirmed,
    /// Tailscale could not make the server public.
    Unavailable(String),
}

impl fmt::Display for FunnelFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Demo(failure) => failure.fmt(formatter),
            Self::NotConfirmed => formatter
                .write_str("The public address needs your confirmation. The server stays local."),
            Self::Unavailable(reason) => formatter.write_str(reason),
        }
    }
}

impl From<DemoFailure> for FunnelFailure {
    fn from(failure: DemoFailure) -> Self {
        Self::Demo(failure)
    }
}

/// Cleans and validates a demo server name.
pub fn clean_name(name: &str) -> Option<String> {
    clean_label(name, MAX_DEMO_NAME_CHARS)
}

/// Returns true when a port may hold a demo server.
pub fn port_is_allowed(port: u16) -> bool {
    port >= MIN_DEMO_PORT
}

/// Builds a new active demo server record.
pub fn new_server(
    session_id: &str,
    name: String,
    port: u16,
    tailscale_address: Option<String>,
    now: &str,
) -> DemoServer {
    DemoServer {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        name,
        port,
        address: local_address(port),
        tailscale_address,
        funnel_address: None,
        state: DemoServerState::Active,
        created_at: now.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_limited() {
        assert_eq!(clean_name("  Preview  ").as_deref(), Some("Preview"));
        assert_eq!(clean_name("\u{202e}  "), None);
        assert_eq!(
            clean_name(&"a".repeat(100)).map(|name| name.chars().count()),
            Some(MAX_DEMO_NAME_CHARS)
        );
    }

    #[test]
    fn only_loopback_ports_above_the_privileged_range_are_allowed() {
        assert!(!port_is_allowed(0));
        assert!(!port_is_allowed(80));
        assert!(!port_is_allowed(1023));
        assert!(port_is_allowed(1024));
        assert!(port_is_allowed(65535));
    }

    #[test]
    fn a_server_carries_its_local_address() {
        let server = new_server(
            "s1",
            "Preview".to_owned(),
            3000,
            None,
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(server.address, "http://localhost:3000");
        assert_eq!(server.tailscale_address, None);
        assert_eq!(server.funnel_address, None);
        assert!(server.is_active());
        assert!(!server.is_public());
    }
}
