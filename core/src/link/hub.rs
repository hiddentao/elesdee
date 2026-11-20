//! Pairing state, paired device access and live link tracking.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use elesdee_link::{
    CONFIRMATION_TTL_SECS, MAX_CONNECTED_PHONES, MAX_FORWARD_TUNNELS, MAX_PAIRED_PHONES,
    MAX_STREAMS_TOTAL, PAIRING_TICKET_TTL_SECS, PHONE_NAME_MAX_CHARS, PHONE_NAME_MIN_CHARS,
    PairTicket, PairingSession, pairing_code,
};
use iroh::endpoint::Connection;
use iroh::endpoint::VarInt;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_tickets::Ticket;
use serde::Serialize;
use tokio::sync::{oneshot, watch};
use zeroize::{Zeroize, Zeroizing};

use super::wire::{PairDecision, PairReject, PairRequest};
use super::{now_unix, short_id, timestamp};
use crate::state::{AppState, PairedDevice};
use crate::text::clean_label;

const RELAY_WAIT: Duration = Duration::from_secs(5);
const MAX_PLATFORM_CHARS: usize = 32;

/// The desktop identity as the phone sees it.
#[derive(Debug, Clone)]
pub struct DesktopInfo {
    /// The endpoint id.
    pub id: EndpointId,
    /// The endpoint id as z-base-32.
    pub id_string: String,
    /// The name shown during pairing.
    pub name: String,
}

/// A freshly created pairing ticket.
pub struct TicketCreated {
    /// Unix expiry time.
    pub expires_at: u64,
    /// True when the endpoint reached a relay.
    pub relay_ready: bool,
    /// The ticket string for the QR code. It carries the one-time secret.
    pub ticket: String,
}

impl fmt::Debug for TicketCreated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TicketCreated")
            .field("expires_at", &self.expires_at)
            .field("relay_ready", &self.relay_ready)
            .field("ticket", &"<redacted>")
            .finish()
    }
}

/// The pairing state that the desktop UI polls.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSnapshot {
    /// One of `idle`, `waiting`, `confirm`, `paired`, `denied`, `expired` or
    /// `cancelled`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Unix expiry time for the active ticket or confirmation.
    pub expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The phone name that is waiting for confirmation.
    pub phone_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The six-digit code that both devices show.
    pub code: Option<String>,
}

/// A confirmation that the pair handler waits on.
pub struct PairConfirm {
    /// The six-digit code to send to the phone.
    pub code: String,
    /// The user's answer, or an error when the confirmation expires.
    pub decision: oneshot::Receiver<PairDecision>,
}

/// A failure to approve a phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApproveFailure {
    /// No phone is waiting for confirmation.
    NoPending,
    /// The paired device list is full.
    DeviceLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting,
    Confirm,
    Paired,
    Denied,
    Expired,
    Cancelled,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Waiting => "waiting",
            Self::Confirm => "confirm",
            Self::Paired => "paired",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }
}

struct Pairing {
    phase: Phase,
    ticket: Option<PairTicket>,
    session: Option<PairingSession>,
    pending: Option<PairedDevice>,
    code: Option<String>,
    confirm_expires_at: Option<u64>,
    decision: Option<oneshot::Sender<PairDecision>>,
}

impl Pairing {
    fn new() -> Self {
        Self {
            phase: Phase::Idle,
            ticket: None,
            session: None,
            pending: None,
            code: None,
            confirm_expires_at: None,
            decision: None,
        }
    }

    fn reset_terminal(&mut self, phase: Phase) {
        self.phase = phase;
        self.ticket = None;
        self.pending = None;
        self.code = None;
        self.confirm_expires_at = None;
        if let Some(mut session) = self.session.take() {
            session.cancel();
            self.session = Some(session);
        }
    }
}

struct HubState {
    endpoint: Option<Endpoint>,
    addr: Option<EndpointAddr>,
    desktop: Option<DesktopInfo>,
    wait_relay: bool,
    pairing: Pairing,
}

struct HubInner {
    state: Mutex<HubState>,
    streams_total: AtomicUsize,
    connected: AtomicUsize,
    next_connection: AtomicU64,
    live: Mutex<HashMap<String, Vec<(u64, Connection)>>>,
    revoked: Mutex<HashSet<String>>,
    forward_tunnels: Mutex<HashMap<u16, watch::Sender<bool>>>,
}

/// Shared remote-link state.
///
/// The core starts one endpoint. The local API uses this hub to create pairing
/// tickets and manage devices. The endpoint handlers use it to check the paired
/// device list and the connection and stream limits.
#[derive(Clone)]
pub struct LinkHub {
    inner: Arc<HubInner>,
}

impl LinkHub {
    /// Creates a hub with no endpoint.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(HubInner {
                state: Mutex::new(HubState {
                    endpoint: None,
                    addr: None,
                    desktop: None,
                    wait_relay: false,
                    pairing: Pairing::new(),
                }),
                streams_total: AtomicUsize::new(0),
                connected: AtomicUsize::new(0),
                next_connection: AtomicU64::new(0),
                live: Mutex::new(HashMap::new()),
                revoked: Mutex::new(HashSet::new()),
                forward_tunnels: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn set_ready(&self, endpoint: Endpoint, desktop: DesktopInfo, wait_relay: bool) {
        let mut state = self.lock_state();
        state.addr = Some(endpoint.addr());
        state.endpoint = Some(endpoint);
        state.desktop = Some(desktop);
        state.wait_relay = wait_relay;
    }

    /// Marks the hub ready with a stored address, without an endpoint. Tests use
    /// this to exercise the pairing state machine.
    #[doc(hidden)]
    pub fn set_test_ready(&self, desktop: DesktopInfo, addr: EndpointAddr) {
        let mut state = self.lock_state();
        state.addr = Some(addr);
        state.desktop = Some(desktop);
        state.wait_relay = false;
    }

    /// Returns true when an endpoint is bound and a desktop identity is known.
    pub fn is_ready(&self) -> bool {
        let state = self.lock_state();
        state.endpoint.is_some() && state.desktop.is_some()
    }

    /// Returns the desktop identity, when the endpoint is ready.
    pub fn desktop(&self) -> Option<DesktopInfo> {
        self.lock_state().desktop.clone()
    }

    /// Creates a ticket for 120 seconds.
    ///
    /// When `wait_relay` is true and the endpoint is configured for relays, the
    /// call waits at most five seconds for a relay. A missing relay does not
    /// stop pairing; the ticket then works on the same network only.
    ///
    /// # Errors
    ///
    /// Returns [`PairReject::NotReady`] when no endpoint is bound.
    pub async fn create_ticket(
        &self,
        now: u64,
        wait_relay: bool,
    ) -> Result<TicketCreated, PairReject> {
        let (endpoint, waits_for_relay) = {
            let state = self.lock_state();
            (state.endpoint.clone(), state.wait_relay)
        };
        let Some(endpoint) = endpoint else {
            return Err(PairReject::NotReady);
        };

        let relay_ready = if wait_relay && waits_for_relay {
            tokio::time::timeout(RELAY_WAIT, endpoint.online())
                .await
                .is_ok()
        } else {
            false
        };
        {
            let mut state = self.lock_state();
            state.addr = Some(endpoint.addr());
        }
        self.install_ticket(now.saturating_add(PAIRING_TICKET_TTL_SECS), relay_ready)
    }

    /// Creates a ticket with an explicit expiry. Tests use this to create an
    /// expired ticket without waiting.
    ///
    /// # Errors
    ///
    /// Returns [`PairReject::NotReady`] when no desktop identity is set.
    pub fn create_ticket_at(
        &self,
        expires_at: u64,
        relay_ready: bool,
    ) -> Result<TicketCreated, PairReject> {
        self.install_ticket(expires_at, relay_ready)
    }

    fn install_ticket(
        &self,
        expires_at: u64,
        relay_ready: bool,
    ) -> Result<TicketCreated, PairReject> {
        let mut hub = self.lock_state();
        let Some(addr) = hub.addr.clone() else {
            return Err(PairReject::NotReady);
        };
        let Some(desktop) = hub.desktop.clone() else {
            return Err(PairReject::NotReady);
        };

        let previous = hub.pairing.decision.take();
        if let Some(mut session) = hub.pairing.session.take() {
            session.cancel();
        }
        drop(previous);

        let mut secret = Zeroizing::new(rand::random::<[u8; 32]>());
        let ticket = PairTicket::new(addr, *secret, desktop.name, expires_at)
            .map_err(|_| PairReject::InvalidRequest("The computer name is not valid."))?;
        secret.zeroize();
        let ticket_string = ticket.encode_string();
        let session = PairingSession::from_ticket(&ticket);

        hub.pairing = Pairing {
            phase: Phase::Waiting,
            ticket: Some(ticket),
            session: Some(session),
            pending: None,
            code: None,
            confirm_expires_at: None,
            decision: None,
        };
        drop(hub);

        tracing::info!(expires_at, relay_ready, "pairing ticket created");
        Ok(TicketCreated {
            expires_at,
            relay_ready,
            ticket: ticket_string,
        })
    }

    /// Returns the ticket string for the QR code while the ticket waits.
    pub fn active_ticket(&self, now: u64) -> Option<String> {
        let mut hub = self.lock_state();
        refresh_expiry(&mut hub.pairing, now);
        if hub.pairing.phase != Phase::Waiting {
            return None;
        }
        hub.pairing
            .ticket
            .as_ref()
            .map(|ticket| ticket.encode_string())
    }

    /// Returns the pairing state, expiring an old ticket or confirmation first.
    pub fn snapshot(&self, now: u64) -> PairingSnapshot {
        let mut hub = self.lock_state();
        refresh_expiry(&mut hub.pairing, now);
        let pairing = &hub.pairing;
        PairingSnapshot {
            state: pairing.phase.name(),
            expires_at: match pairing.phase {
                Phase::Waiting => pairing.session.as_ref().map(|session| session.expires_at()),
                Phase::Confirm => pairing.confirm_expires_at,
                _ => None,
            },
            phone_name: if pairing.phase == Phase::Confirm {
                pairing.pending.as_ref().map(|device| device.name.clone())
            } else {
                None
            },
            code: if pairing.phase == Phase::Confirm {
                pairing.code.clone()
            } else {
                None
            },
        }
    }

    /// Checks one pairing request and starts the confirmation step.
    ///
    /// # Errors
    ///
    /// Returns a [`PairReject`] for an expired, used or cancelled ticket, a
    /// wrong secret, too many attempts, an invalid request, no shared protocol
    /// version, an invalid name or a full device list.
    pub fn begin_pairing(
        &self,
        request: &PairRequest,
        phone_id: EndpointId,
        already_paired: bool,
        paired_count: usize,
        now: u64,
    ) -> Result<PairConfirm, PairReject> {
        let mut hub = self.lock_state();
        let Some(desktop) = hub.desktop.clone() else {
            return Err(PairReject::NotReady);
        };
        let pairing = &mut hub.pairing;
        let Some(session) = pairing.session.as_ref() else {
            return Err(PairReject::Cancelled);
        };

        let already_used = session.is_used();
        let already_cancelled = session.is_cancelled();
        let already_expired = session.is_expired(now);
        if already_used {
            return Err(PairReject::Used);
        }
        if already_cancelled {
            return Err(PairReject::Cancelled);
        }
        if already_expired {
            pairing.reset_terminal(Phase::Expired);
            return Err(PairReject::Expired);
        }
        if request.v != elesdee_link::PROTOCOL_VERSION {
            return Err(PairReject::InvalidRequest(
                "The pairing protocol is not supported.",
            ));
        }

        let secret = request.secret_bytes()?;
        let verified = pairing
            .session
            .as_mut()
            .expect("the session was checked above")
            .verify(&secret, now);
        match verified {
            Ok(()) => {}
            Err(elesdee_link::PairingError::SecretMismatch) => {
                return Err(PairReject::SecretMismatch);
            }
            Err(elesdee_link::PairingError::TooManyAttempts) => {
                pairing.reset_terminal(Phase::Cancelled);
                return Err(PairReject::TooManyAttempts);
            }
            Err(elesdee_link::PairingError::Expired) => {
                pairing.reset_terminal(Phase::Expired);
                return Err(PairReject::Expired);
            }
            Err(elesdee_link::PairingError::Used) => return Err(PairReject::Used),
            Err(elesdee_link::PairingError::Cancelled) => {
                return Err(PairReject::Cancelled);
            }
            Err(_) => return Err(PairReject::Cancelled),
        }

        if !request.protocol_overlaps() {
            pairing.reset_terminal(Phase::Denied);
            return Err(PairReject::ProtocolMismatch);
        }
        let Some(name) = clean_phone_name(&request.name) else {
            pairing.reset_terminal(Phase::Denied);
            return Err(PairReject::InvalidName);
        };
        if !already_paired && paired_count >= MAX_PAIRED_PHONES {
            pairing.reset_terminal(Phase::Denied);
            return Err(PairReject::DeviceLimit);
        }

        let Some(ticket) = pairing.ticket.as_ref() else {
            return Err(PairReject::Cancelled);
        };
        let code = pairing_code(ticket.secret(), &phone_id, &desktop.id);
        let platform = clean_label(&request.platform, MAX_PLATFORM_CHARS)
            .unwrap_or_else(|| "unknown".to_owned());
        let paired_at = timestamp();
        let device = PairedDevice {
            endpoint_id: phone_id.to_z32(),
            name,
            platform,
            paired_at: paired_at.clone(),
            last_seen: Some(paired_at),
        };

        let (sender, receiver) = oneshot::channel();
        pairing.phase = Phase::Confirm;
        pairing.pending = Some(device);
        pairing.code = Some(code.clone());
        pairing.confirm_expires_at = Some(now.saturating_add(CONFIRMATION_TTL_SECS));
        pairing.decision = Some(sender);

        tracing::info!(
            phone = %short_id(&phone_id.to_z32()),
            "a phone is waiting for confirmation"
        );
        Ok(PairConfirm {
            code,
            decision: receiver,
        })
    }

    /// Stores the waiting phone and tells the pair handler the user approved.
    ///
    /// # Errors
    ///
    /// Returns [`ApproveFailure::NoPending`] when no phone waits, and
    /// [`ApproveFailure::DeviceLimit`] when the device list is full.
    pub fn approve(&self, state: &AppState) -> Result<PairedDevice, ApproveFailure> {
        let mut hub = self.lock_state();
        refresh_expiry(&mut hub.pairing, now_unix());
        if hub.pairing.phase != Phase::Confirm {
            return Err(ApproveFailure::NoPending);
        }
        let Some(device) = hub.pairing.pending.clone() else {
            return Err(ApproveFailure::NoPending);
        };

        match state.add_paired_device(device.clone()) {
            Ok(device) => {
                hub.pairing.phase = Phase::Paired;
                hub.pairing.pending = None;
                hub.pairing.ticket = None;
                hub.pairing.confirm_expires_at = None;
                let sender = hub.pairing.decision.take();
                drop(hub);
                state.save();
                if let Some(sender) = sender {
                    let _ = sender.send(PairDecision::Approved(device.clone()));
                }
                tracing::info!(
                    phone = %short_id(&device.endpoint_id),
                    "paired a phone"
                );
                Ok(device)
            }
            Err(_) => {
                hub.pairing.reset_terminal(Phase::Denied);
                let sender = hub.pairing.decision.take();
                drop(hub);
                if let Some(sender) = sender {
                    let _ = sender.send(PairDecision::Denied);
                }
                Err(ApproveFailure::DeviceLimit)
            }
        }
    }

    /// Denies the waiting phone.
    pub fn deny(&self) {
        let mut hub = self.lock_state();
        if !matches!(hub.pairing.phase, Phase::Waiting | Phase::Confirm) {
            return;
        }
        hub.pairing.reset_terminal(Phase::Denied);
        let sender = hub.pairing.decision.take();
        drop(hub);
        if let Some(sender) = sender {
            let _ = sender.send(PairDecision::Denied);
        }
        tracing::info!("pairing denied");
    }

    /// Cancels pairing and any waiting confirmation.
    pub fn cancel(&self) {
        let mut hub = self.lock_state();
        hub.pairing.reset_terminal(Phase::Cancelled);
        let sender = hub.pairing.decision.take();
        drop(hub);
        if let Some(sender) = sender {
            let _ = sender.send(PairDecision::Denied);
        }
        tracing::info!("pairing cancelled");
    }

    /// Returns true when a pairing session exists and a connection may read a
    /// request. Expired, used and cancelled sessions still answer, so the phone
    /// gets a clear error.
    pub fn pairing_open(&self) -> bool {
        self.lock_state().pairing.session.is_some()
    }

    /// Marks an endpoint id as removed so the accept path closes it with the
    /// revocation code.
    pub fn mark_revoked(&self, endpoint_id: &str) {
        self.lock_revoked().insert(endpoint_id.to_owned());
    }

    /// Returns true when a phone was removed during this run.
    pub fn is_revoked(&self, endpoint_id: &str) -> bool {
        self.lock_revoked().contains(endpoint_id)
    }

    /// Returns true while a phone holds at least one live connection.
    pub fn is_connected(&self, endpoint_id: &str) -> bool {
        self.lock_live()
            .get(endpoint_id)
            .is_some_and(|connections| !connections.is_empty())
    }

    /// Closes every live connection for a removed phone.
    pub fn revoke_connections(&self, endpoint_id: &str) {
        let connections = self.lock_live().remove(endpoint_id).unwrap_or_default();
        for (_, connection) in connections {
            connection.close(VarInt::from_u32(elesdee_link::CLOSE_REVOKED), b"revoked");
        }
        tracing::info!(phone = %short_id(endpoint_id), "closed revoked phone connections");
    }

    /// Registers a live app connection.
    ///
    /// Returns `None` when the connected phone limit is reached.
    pub fn register_connection(
        &self,
        endpoint_id: &str,
        connection: Connection,
    ) -> Option<ConnectionPermit> {
        if !try_increment(&self.inner.connected, MAX_CONNECTED_PHONES) {
            return None;
        }
        let connection_id = self.inner.next_connection.fetch_add(1, Ordering::Relaxed);
        self.lock_live()
            .entry(endpoint_id.to_owned())
            .or_default()
            .push((connection_id, connection));
        Some(ConnectionPermit {
            hub: self.clone(),
            endpoint_id: endpoint_id.to_owned(),
            connection_id,
        })
    }

    /// Reserves one stream across all phones.
    pub fn acquire_stream(&self) -> Option<StreamPermit> {
        if try_increment(&self.inner.streams_total, MAX_STREAMS_TOTAL) {
            Some(StreamPermit { hub: self.clone() })
        } else {
            None
        }
    }

    /// Opens one demo-server tunnel.
    ///
    /// A port may have one tunnel, and the computer may hold at most
    /// [`MAX_FORWARD_TUNNELS`] tunnels. The permit closes the tunnel's
    /// cancellation channel when it drops.
    pub fn open_forward(&self, port: u16) -> ForwardOpen {
        let mut tunnels = lock(&self.inner.forward_tunnels);
        if tunnels.contains_key(&port) {
            return ForwardOpen::PortInUse;
        }
        if tunnels.len() >= MAX_FORWARD_TUNNELS {
            return ForwardOpen::Limit;
        }
        let (cancel, receiver) = watch::channel(false);
        tunnels.insert(port, cancel);
        ForwardOpen::Opened(ForwardPermit {
            hub: self.clone(),
            port,
            cancel: receiver,
        })
    }

    /// Closes the tunnel for a port, if one is open.
    pub fn close_forward(&self, port: u16) {
        if let Some(cancel) = lock(&self.inner.forward_tunnels).get(&port) {
            let _ = cancel.send(true);
        }
    }

    /// Closes every demo-server tunnel.
    pub fn close_all_forwards(&self) {
        for cancel in lock(&self.inner.forward_tunnels).values() {
            let _ = cancel.send(true);
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, HubState> {
        lock(&self.inner.state)
    }

    fn lock_live(&self) -> MutexGuard<'_, HashMap<String, Vec<(u64, Connection)>>> {
        lock(&self.inner.live)
    }

    fn lock_revoked(&self) -> MutexGuard<'_, HashSet<String>> {
        lock(&self.inner.revoked)
    }
}

impl Default for LinkHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps a live connection registered until its handler exits.
pub struct ConnectionPermit {
    hub: LinkHub,
    endpoint_id: String,
    connection_id: u64,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        if let Some(connections) = self.hub.lock_live().get_mut(&self.endpoint_id) {
            connections.retain(|(id, _)| *id != self.connection_id);
        }
        self.hub.inner.connected.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Keeps one stream counted until it closes.
pub struct StreamPermit {
    hub: LinkHub,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.hub.inner.streams_total.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The result of opening a demo-server tunnel.
pub enum ForwardOpen {
    /// The tunnel was reserved.
    Opened(ForwardPermit),
    /// Another tunnel already uses the port.
    PortInUse,
    /// The computer holds the maximum number of tunnels.
    Limit,
}

/// Keeps one demo-server tunnel reserved until the handler ends.
pub struct ForwardPermit {
    hub: LinkHub,
    port: u16,
    cancel: watch::Receiver<bool>,
}

impl ForwardPermit {
    /// Returns the loopback port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns a receiver that reports when the tunnel is stopped.
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel.clone()
    }
}

impl Drop for ForwardPermit {
    fn drop(&mut self) {
        lock(&self.hub.inner.forward_tunnels).remove(&self.port);
    }
}

fn refresh_expiry(pairing: &mut Pairing, now: u64) {
    match pairing.phase {
        Phase::Waiting => {
            let expired = pairing
                .session
                .as_ref()
                .is_some_and(|session| session.is_expired(now));
            if expired {
                pairing.reset_terminal(Phase::Expired);
            }
        }
        Phase::Confirm => {
            let expired = pairing
                .confirm_expires_at
                .is_some_and(|expires_at| now >= expires_at);
            if expired {
                pairing.reset_terminal(Phase::Expired);
                let sender = pairing.decision.take();
                drop(sender);
            }
        }
        Phase::Idle | Phase::Paired | Phase::Denied | Phase::Expired | Phase::Cancelled => {}
    }
}

fn clean_phone_name(value: &str) -> Option<String> {
    let cleaned = clean_label(value, usize::MAX)?;
    let length = cleaned.chars().count();
    (PHONE_NAME_MIN_CHARS..=PHONE_NAME_MAX_CHARS)
        .contains(&length)
        .then_some(cleaned)
}

fn try_increment(counter: &AtomicUsize, max: usize) -> bool {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        if current >= max {
            return false;
        }
        match counter.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use elesdee_link::{MAX_PAIRED_PHONES, SecretKey};

    use super::*;
    use crate::state::{AppState, PairedDevice};

    fn hub() -> (LinkHub, EndpointId) {
        let desktop = SecretKey::from_bytes(&[1; 32]).public();
        let info = DesktopInfo {
            id: desktop,
            id_string: desktop.to_z32(),
            name: "Studio computer".to_owned(),
        };
        let hub = LinkHub::new();
        hub.set_test_ready(info, EndpointAddr::new(desktop));
        (hub, desktop)
    }

    fn phone() -> EndpointId {
        SecretKey::from_bytes(&[2; 32]).public()
    }

    fn request(secret: &[u8; 32]) -> PairRequest {
        PairRequest::new(secret, "Phone 7F3A", "android")
    }

    fn reject(result: Result<PairConfirm, PairReject>) -> PairReject {
        match result {
            Ok(_) => panic!("expected the pairing request to be rejected"),
            Err(error) => error,
        }
    }

    #[test]
    fn a_ticket_starts_in_the_waiting_state() {
        let (hub, _) = hub();
        let created = hub.create_ticket_at(1_000, false).expect("ticket");
        assert_eq!(created.expires_at, 1_000);
        assert!(!created.ticket.is_empty());
        assert_eq!(hub.snapshot(999).state, "waiting");
        assert!(hub.active_ticket(999).is_some());
        assert_eq!(hub.snapshot(1_000).state, "expired");
        assert!(hub.active_ticket(1_000).is_none());
    }

    #[test]
    fn wrong_secrets_cancel_the_session_at_the_limit() {
        let (hub, _) = hub();
        hub.create_ticket_at(10_000, false).expect("ticket");
        let wrong = [9u8; 32];

        for _ in 1..elesdee_link::PAIRING_MAX_ATTEMPTS {
            assert_eq!(
                reject(hub.begin_pairing(&request(&wrong), phone(), false, 0, 1)),
                PairReject::SecretMismatch
            );
        }
        assert_eq!(
            reject(hub.begin_pairing(&request(&wrong), phone(), false, 0, 1)),
            PairReject::TooManyAttempts
        );
        assert_eq!(hub.snapshot(1).state, "cancelled");
    }

    #[test]
    fn an_expired_ticket_is_rejected() {
        let (hub, _) = hub();
        let created = hub.create_ticket_at(500, false).expect("ticket");
        let secret = *PairTicket::decode_string(&created.ticket)
            .expect("ticket")
            .secret();
        assert_eq!(
            reject(hub.begin_pairing(&request(&secret), phone(), false, 0, 500)),
            PairReject::Expired
        );
        assert_eq!(hub.snapshot(500).state, "expired");
    }

    #[test]
    fn a_correct_secret_opens_confirmation_once() {
        let (hub, desktop) = hub();
        let created = hub.create_ticket_at(10_000, false).expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        let phone = phone();

        let confirm = hub
            .begin_pairing(&request(ticket.secret()), phone, false, 0, 1)
            .expect("confirmation");
        assert_eq!(confirm.code.len(), 6);
        assert!(
            confirm
                .code
                .chars()
                .all(|character| character.is_ascii_digit())
        );
        assert_eq!(
            confirm.code,
            pairing_code(ticket.secret(), &phone, &desktop)
        );

        let snapshot = hub.snapshot(1);
        assert_eq!(snapshot.state, "confirm");
        assert_eq!(snapshot.phone_name.as_deref(), Some("Phone 7F3A"));
        assert_eq!(snapshot.code.as_deref(), Some(confirm.code.as_str()));

        assert_eq!(
            reject(hub.begin_pairing(&request(ticket.secret()), phone, false, 0, 1)),
            PairReject::Used
        );
    }

    #[test]
    fn approval_stores_one_device_and_replay_is_rejected() {
        let state = AppState::ephemeral();
        let hub = state.link().clone();
        let desktop = SecretKey::from_bytes(&[3; 32]).public();
        hub.set_test_ready(
            DesktopInfo {
                id: desktop,
                id_string: desktop.to_z32(),
                name: "Studio computer".to_owned(),
            },
            EndpointAddr::new(desktop),
        );
        let now = now_unix();
        let created = hub
            .create_ticket_at(now.saturating_add(120), false)
            .expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        let phone = phone();
        hub.begin_pairing(&request(ticket.secret()), phone, false, 0, now)
            .expect("confirmation");

        let device = hub.approve(&state).expect("approve");
        assert_eq!(device.endpoint_id, phone.to_z32());
        assert_eq!(state.paired_device_count(), 1);
        assert_eq!(hub.snapshot(now).state, "paired");
        assert_eq!(
            reject(hub.begin_pairing(&request(ticket.secret()), phone, true, 1, now)),
            PairReject::Used
        );
        assert_eq!(hub.approve(&state), Err(ApproveFailure::NoPending));
    }

    #[test]
    fn deny_and_cancel_release_the_waiting_phone() {
        let state = AppState::ephemeral();
        let hub = state.link().clone();
        let desktop = SecretKey::from_bytes(&[4; 32]).public();
        hub.set_test_ready(
            DesktopInfo {
                id: desktop,
                id_string: desktop.to_z32(),
                name: "Studio computer".to_owned(),
            },
            EndpointAddr::new(desktop),
        );
        let created = hub.create_ticket_at(10_000, false).expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        hub.begin_pairing(&request(ticket.secret()), phone(), false, 0, 1)
            .expect("confirmation");
        hub.deny();
        assert_eq!(hub.snapshot(1).state, "denied");
        assert_eq!(state.paired_device_count(), 0);

        let created = hub.create_ticket_at(10_000, false).expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        hub.begin_pairing(&request(ticket.secret()), phone(), false, 0, 1)
            .expect("confirmation");
        hub.cancel();
        assert_eq!(hub.snapshot(1).state, "cancelled");
        assert_eq!(state.paired_device_count(), 0);
    }

    #[test]
    fn the_device_limit_blocks_a_new_phone() {
        let state = AppState::ephemeral();
        let hub = state.link().clone();
        let desktop = SecretKey::from_bytes(&[5; 32]).public();
        hub.set_test_ready(
            DesktopInfo {
                id: desktop,
                id_string: desktop.to_z32(),
                name: "Studio computer".to_owned(),
            },
            EndpointAddr::new(desktop),
        );
        for index in 0..MAX_PAIRED_PHONES {
            let id = SecretKey::from_bytes(&[index as u8; 32]).public().to_z32();
            state
                .add_paired_device(PairedDevice {
                    endpoint_id: id,
                    name: "Phone".to_owned(),
                    platform: "android".to_owned(),
                    paired_at: timestamp(),
                    last_seen: None,
                })
                .expect("add device");
        }
        let created = hub.create_ticket_at(10_000, false).expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        assert_eq!(
            reject(hub.begin_pairing(
                &request(ticket.secret()),
                phone(),
                false,
                MAX_PAIRED_PHONES,
                1
            )),
            PairReject::DeviceLimit
        );
    }

    #[test]
    fn an_overlong_name_is_rejected_after_the_secret_is_used() {
        let (hub, _) = hub();
        let created = hub.create_ticket_at(10_000, false).expect("ticket");
        let ticket = PairTicket::decode_string(&created.ticket).expect("ticket");
        let mut overlong = request(ticket.secret());
        overlong.name = "x".repeat(PHONE_NAME_MAX_CHARS + 1);
        assert_eq!(
            reject(hub.begin_pairing(&overlong, phone(), false, 0, 1)),
            PairReject::InvalidName
        );
        assert_eq!(
            reject(hub.begin_pairing(&request(ticket.secret()), phone(), false, 0, 1)),
            PairReject::Used
        );
    }
}
