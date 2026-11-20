//! The core side of the remote link.
//!
//! The core holds one long-lived iroh endpoint. It uses the shared `link`
//! crate for the device identity, the pairing ticket and the request framing.
//! This module owns the endpoint start and stop, the pairing state machine, the
//! paired device list access and the authenticated app path.
//!
//! The app path runs the `hello` and `welcome` handshake, dispatches every
//! allowlisted unary method through the local router, and forwards the event
//! and session output streams with sequence replay.

mod app;
mod forward;
pub mod hub;
pub mod identity;
mod pair;
pub mod qr;
pub mod wire;

use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use elesdee_link::{
    APP_ALPN, DeviceIdentity, FORWARD_ALPN, MAX_STREAMS_PER_CONNECTION, PAIR_ALPN,
    QUIC_IDLE_TIMEOUT, QUIC_KEEP_ALIVE, SecretKey,
};
use iroh::endpoint::QuicTransportConfig;
use iroh::endpoint::VarInt;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr};

use crate::state::AppState;
use crate::text::clean_label;
pub use app::AppHandler;
pub use forward::ForwardHandler;
pub use hub::{ApproveFailure, DesktopInfo, LinkHub, PairingSnapshot, TicketCreated};
pub use identity::{IDENTITY_FILE, identity_path, load_or_create_identity};
pub use pair::PairHandler;
pub use wire::{
    MAX_PAIR_FRAME_BYTES, PairDecision, PairReject, PairReply, PairRequest, read_pair_frame,
    write_pair_frame,
};

/// A failure to start the link endpoint.
#[derive(Debug)]
pub struct LinkStartError {
    message: String,
}

impl LinkStartError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for LinkStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LinkStartError {}

/// A running link endpoint and its protocol router.
pub struct LinkService {
    endpoint: Endpoint,
    router: Router,
}

impl LinkService {
    /// Loads the desktop identity and starts the production endpoint.
    ///
    /// The endpoint uses the public relays and n0 discovery. A missing relay
    /// does not stop the local API; it only makes pairing a same-network path.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity cannot be saved or the UDP socket
    /// cannot bind. The caller keeps the local API running in that case.
    pub async fn start(state: AppState) -> Result<Self, LinkStartError> {
        let identity = match state.data_dir() {
            Some(directory) => load_or_create_identity(directory).map_err(|error| {
                LinkStartError::new(format!("could not load the device identity: {error}"))
            })?,
            None => {
                tracing::warn!("no data directory is set; the link identity is temporary");
                DeviceIdentity::generate()
            }
        };
        let desktop = DesktopInfo {
            id: identity.endpoint_id(),
            id_string: identity.endpoint_id_string(),
            name: computer_name(),
        };
        let endpoint = bind_endpoint(identity.secret_key().clone())
            .await
            .map_err(|error| {
                LinkStartError::new(format!("could not bind the link endpoint: {error}"))
            })?;
        Ok(Self::start_endpoint(state, endpoint, desktop, true))
    }

    /// Starts the handlers on an endpoint that is already bound.
    ///
    /// Tests use this with loopback endpoints. Production uses [`start`].
    pub fn start_with_endpoint(state: AppState, endpoint: Endpoint) -> Self {
        persist_endpoint_identity(&state, &endpoint);
        let id = endpoint.id();
        let desktop = DesktopInfo {
            id,
            id_string: id.to_z32(),
            name: computer_name(),
        };
        Self::start_endpoint(state, endpoint, desktop, false)
    }

    fn start_endpoint(
        state: AppState,
        endpoint: Endpoint,
        desktop: DesktopInfo,
        wait_relay: bool,
    ) -> Self {
        tracing::info!(
            computer = %short_id(&desktop.id_string),
            name = %desktop.name,
            "link endpoint bound"
        );
        state
            .link()
            .set_ready(endpoint.clone(), desktop, wait_relay);

        let pair = PairHandler::new(state.clone(), state.link().clone());
        let app = AppHandler::new(
            state.clone(),
            state.link().clone(),
            crate::api::router(state.clone()),
        );
        let forward = ForwardHandler::new(state.clone(), state.link().clone());
        let router = Router::builder(endpoint.clone())
            .accept(PAIR_ALPN, pair)
            .accept(APP_ALPN, app)
            .accept(FORWARD_ALPN, forward)
            .spawn();
        Self { endpoint, router }
    }

    /// Returns the bound endpoint address.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Stops the protocol router and closes the endpoint.
    pub async fn shutdown(self) {
        if let Err(error) = self.router.shutdown().await {
            tracing::warn!(%error, "the link router stopped with an error");
        }
        self.endpoint.close().await;
        tracing::info!("link endpoint stopped");
    }
}

/// Saves a bound endpoint's key when no identity file exists yet.
///
/// The production start loads the key before binding. This path keeps a
/// caller-supplied endpoint identity on disk so a restart uses the same one.
fn persist_endpoint_identity(state: &AppState, endpoint: &Endpoint) {
    let Some(directory) = state.data_dir() else {
        return;
    };
    let path = identity_path(directory);
    if path.exists() {
        return;
    }
    let bytes = endpoint.secret_key().to_bytes();
    if let Err(error) = crate::private_fs::write_private_file_atomic(&path, &bytes) {
        tracing::error!(%error, "could not save the link identity");
    }
}

async fn bind_endpoint(secret_key: SecretKey) -> Result<Endpoint, iroh::endpoint::BindError> {
    let transport = QuicTransportConfig::builder()
        .max_concurrent_bidi_streams(VarInt::from_u32(MAX_STREAMS_PER_CONNECTION as u32))
        .keep_alive_interval(QUIC_KEEP_ALIVE)
        .max_idle_timeout(Some(
            VarInt::from_u32(QUIC_IDLE_TIMEOUT.as_millis() as u32).into(),
        ))
        .build();
    Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![
            PAIR_ALPN.to_vec(),
            APP_ALPN.to_vec(),
            FORWARD_ALPN.to_vec(),
        ])
        .transport_config(transport)
        .bind()
        .await
}

/// Returns the current Unix time in seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(crate) fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Returns the computer name shown during pairing.
///
/// The name never names the operating system. An explicit environment value
/// wins, then the host name, then a neutral fallback.
pub fn computer_name() -> String {
    env::var("ELESDEE_COMPUTER_NAME")
        .ok()
        .or_else(|| env::var("HOSTNAME").ok())
        .or_else(|| env::var("COMPUTERNAME").ok())
        .and_then(|value| clean_label(&value, 64))
        .unwrap_or_else(|| "elesdee computer".to_owned())
}
