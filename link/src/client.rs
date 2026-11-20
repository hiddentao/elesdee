//! Phone-side link client.
//!
//! The client owns the phone endpoint, the pairing handshake, the app
//! connection and the unary request path. The shell supplies a [`PhoneStore`]
//! for the identity and the computer list. The identity secret goes to the
//! platform keychain, so this crate never writes it to a plain file.
//!
//! The pairing handshake uses a small length-prefixed JSON frame. The app
//! path uses the shared request and response framing. Both readers check the
//! declared frame length before they allocate a buffer.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use data_encoding::BASE32_NOPAD;
use iroh::endpoint::presets;
use iroh::endpoint::{Connection, ConnectionError, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointAddr};
use iroh_tickets::Ticket;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::timeout;
use zeroize::Zeroize;

use crate::limits::{
    CONFIRMATION_TTL_SECS, MAX_HEADER_BYTES, MAX_PAIR_FRAME_BYTES, MAX_UI_PACKAGE_BYTES,
    UNARY_REQUEST_TIMEOUT,
};
use crate::{
    APP_ALPN, CLOSE_NOT_PAIRED, CLOSE_REVOKED, DeviceIdentity, LinkError, Method, PAIR_ALPN,
    PROTOCOL_VERSION, PairTicket, Request, Response,
};

/// The bridge version that this client implements.
pub const BRIDGE_VERSION: u32 = 1;

/// How long the phone waits for the user to approve a pairing.
const PAIRING_WAIT: Duration = Duration::from_secs(CONFIRMATION_TTL_SECS + 5);

/// A platform store for the phone identity and the paired computers.
///
/// The shell implements this trait. The identity must go to the platform
/// keychain, never to a plain file. The computer list holds no secrets, so the
/// shell may keep it as JSON in app-private storage.
pub trait PhoneStore: Send + Sync {
    /// Returns the stored identity secret, when one exists.
    ///
    /// # Errors
    ///
    /// Returns a [`StoreError`] when the keychain cannot be read.
    fn load_identity(&self) -> Result<Option<[u8; 32]>, StoreError>;

    /// Saves the identity secret to the platform keychain.
    ///
    /// # Errors
    ///
    /// Returns a [`StoreError`] when the keychain cannot be written.
    fn save_identity(&self, secret: &[u8; 32]) -> Result<(), StoreError>;

    /// Returns the stored computers.
    ///
    /// # Errors
    ///
    /// Returns a [`StoreError`] when the list cannot be read or parsed.
    fn load_computers(&self) -> Result<Vec<ComputerRecord>, StoreError>;

    /// Saves the computer list. It holds no secrets.
    ///
    /// # Errors
    ///
    /// Returns a [`StoreError`] when the list cannot be written.
    fn save_computers(&self, computers: &[ComputerRecord]) -> Result<(), StoreError>;
}

/// A failure in the platform store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    /// Creates a store failure from a platform error message.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// Returns the message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// A computer that this phone paired with.
///
/// The record holds the endpoint address from the ticket and the names that
/// both sides showed during pairing. It never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerRecord {
    /// The computer endpoint id as z-base-32.
    pub id: String,
    /// The computer name shown on the phone.
    pub name: String,
    /// The endpoint address from the pairing ticket.
    pub addr: EndpointAddr,
    /// The protocol version that the computer announced.
    pub protocol_version: u8,
    /// The oldest phone bridge that the computer accepts.
    pub min_bridge_version: u32,
    /// The pairing time as Unix seconds.
    pub paired_at: u64,
}

/// The confirmation step of the pairing handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingConfirm {
    /// The six-digit code that both devices show.
    pub code: String,
    /// The computer record, ready to save after approval.
    pub computer: ComputerRecord,
}

/// A refusal from the computer during pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRejection {
    /// The wire status, such as `denied`, `expired` or `rejected`.
    pub status: String,
    /// A short message from the computer.
    pub message: Option<String>,
}

/// The computer's reply in the pairing handshake.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairReply {
    /// Message version.
    pub v: u8,
    /// True when this step succeeded.
    pub ok: bool,
    /// The step name, such as `confirm` or `paired`.
    #[serde(default)]
    pub status: Option<String>,
    /// The six-digit code that both devices show.
    #[serde(default)]
    pub code: Option<String>,
    /// The computer identity as z-base-32.
    #[serde(default)]
    pub computer_id: Option<String>,
    /// The computer name to show on the phone.
    #[serde(default)]
    pub computer_name: Option<String>,
    /// The protocol version that the computer uses.
    #[serde(default)]
    pub protocol_version: Option<u8>,
    /// The oldest phone bridge that the computer accepts.
    #[serde(default)]
    pub min_bridge_version: Option<u32>,
    /// A short reason for a rejection.
    #[serde(default)]
    pub message: Option<String>,
}

/// The phone's request in the pairing handshake.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairHello<'a> {
    v: u8,
    secret: String,
    name: &'a str,
    platform: &'a str,
    bridge_version: u32,
    protocol_min: u8,
    protocol_max: u8,
}

/// The state of the pairing handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingState {
    /// The phone waits for the computer's confirmation reply.
    Opening,
    /// The computer showed a code and waits for the user to approve.
    AwaitingApproval(PairingConfirm),
    /// The computer approved the pairing.
    Paired(ComputerRecord),
    /// The computer or the user refused the pairing.
    Rejected(PairingRejection),
}

/// The pure pairing state machine.
///
/// It consumes the computer's replies and checks the order. Unit tests drive
/// it without a network. [`PairingStart`] uses it for the live handshake.
#[derive(Debug, Clone)]
pub struct PairingMachine {
    state: PairingState,
    addr: EndpointAddr,
    paired_at: u64,
}

impl PairingMachine {
    /// Starts the handshake for one ticket address and pairing time.
    pub fn new(addr: EndpointAddr, paired_at: u64) -> Self {
        Self {
            state: PairingState::Opening,
            addr,
            paired_at,
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &PairingState {
        &self.state
    }

    /// Consumes the computer's confirmation reply.
    ///
    /// # Errors
    ///
    /// Rejects a reply for another protocol version, a missing required
    /// field, and a reply that arrives out of order.
    pub fn accept_confirm(&mut self, reply: &PairReply) -> Result<(), ClientError> {
        if !matches!(self.state, PairingState::Opening) {
            return Err(ClientError::Protocol("the pairing reply is out of order"));
        }
        if reply.v != PROTOCOL_VERSION {
            return Err(ClientError::Protocol(
                "the pairing protocol is not supported",
            ));
        }
        if !reply.ok {
            self.state = PairingState::Rejected(PairingRejection {
                status: reply
                    .status
                    .clone()
                    .unwrap_or_else(|| "rejected".to_owned()),
                message: reply.message.clone(),
            });
            return Ok(());
        }
        let status = reply.status.as_deref().unwrap_or_default();
        if status != "confirm" {
            return Err(ClientError::Protocol(
                "the computer did not confirm pairing",
            ));
        }
        let code = reply
            .code
            .clone()
            .filter(|code| valid_code(code))
            .ok_or(ClientError::Protocol("the pairing code is not valid"))?;
        let id = reply
            .computer_id
            .clone()
            .filter(|id| !id.is_empty())
            .ok_or(ClientError::Protocol("the computer id is missing"))?;
        let name = reply
            .computer_name
            .clone()
            .filter(|name| !name.is_empty())
            .ok_or(ClientError::Protocol("the computer name is missing"))?;
        let protocol_version = reply
            .protocol_version
            .ok_or(ClientError::Protocol("the protocol version is missing"))?;
        let min_bridge_version = reply
            .min_bridge_version
            .ok_or(ClientError::Protocol("the bridge version is missing"))?;
        self.state = PairingState::AwaitingApproval(PairingConfirm {
            code,
            computer: ComputerRecord {
                id,
                name,
                addr: self.addr.clone(),
                protocol_version,
                min_bridge_version,
                paired_at: self.paired_at,
            },
        });
        Ok(())
    }

    /// Consumes the computer's final reply.
    ///
    /// # Errors
    ///
    /// Rejects a reply that arrives before the confirmation and a reply for
    /// another protocol version.
    pub fn accept_final(&mut self, reply: &PairReply) -> Result<(), ClientError> {
        if !matches!(self.state, PairingState::AwaitingApproval(_)) {
            return Err(ClientError::Protocol("the pairing reply is out of order"));
        }
        if reply.v != PROTOCOL_VERSION {
            return Err(ClientError::Protocol(
                "the pairing protocol is not supported",
            ));
        }
        if reply.ok && reply.status.as_deref() == Some("paired") {
            let PairingState::AwaitingApproval(confirm) = &self.state else {
                return Err(ClientError::Protocol("the pairing reply is out of order"));
            };
            let computer = confirm.computer.clone();
            self.state = PairingState::Paired(computer);
            return Ok(());
        }
        self.state = PairingState::Rejected(PairingRejection {
            status: reply
                .status
                .clone()
                .unwrap_or_else(|| "rejected".to_owned()),
            message: reply.message.clone(),
        });
        Ok(())
    }
}

/// A live pairing that waits for the user to approve it on the computer.
pub struct PairingStart {
    connection: Connection,
    recv: RecvStream,
    machine: PairingMachine,
    store: Arc<dyn PhoneStore>,
}

impl fmt::Debug for PairingStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingStart")
            .field("state", self.machine.state())
            .finish_non_exhaustive()
    }
}

impl PairingStart {
    /// Returns the confirmation details to show on the phone.
    ///
    /// # Errors
    ///
    /// Returns a protocol error when the handshake is not at the confirmation
    /// step.
    pub fn confirm(&self) -> Result<&PairingConfirm, ClientError> {
        match self.machine.state() {
            PairingState::AwaitingApproval(confirm) => Ok(confirm),
            _ => Err(ClientError::Protocol("pairing is not waiting for approval")),
        }
    }

    /// Waits for the computer's final answer.
    ///
    /// On approval, the computer record is saved to the store.
    ///
    /// # Errors
    ///
    /// Returns the rejection, a timeout or a transport failure.
    pub async fn finish(mut self) -> Result<ComputerRecord, ClientError> {
        let reply = timeout(
            PAIRING_WAIT,
            read_json::<PairReply>(&mut self.recv, MAX_PAIR_FRAME_BYTES),
        )
        .await
        .map_err(|_| ClientError::Timeout)??;
        self.machine.accept_final(&reply)?;
        match self.machine.state() {
            PairingState::Paired(record) => {
                save_computer(self.store.as_ref(), record.clone())?;
                self.connection.close(VarInt::from_u32(0), b"paired");
                Ok(record.clone())
            }
            PairingState::Rejected(rejection) => {
                self.connection.close(VarInt::from_u32(0), b"rejected");
                Err(ClientError::Rejected(rejection.clone()))
            }
            _ => Err(ClientError::Protocol("pairing ended in an unknown state")),
        }
    }

    /// Abandons the pairing and closes the connection.
    pub fn cancel(self) {
        self.connection.close(VarInt::from_u32(0), b"cancelled");
    }

    /// Returns a handle that can close the pairing from another task.
    ///
    /// The shell uses it so the Cancel control works while a pairing is
    /// waiting for approval.
    pub fn cancel_handle(&self) -> PairingCancel {
        PairingCancel(self.connection.clone())
    }
}

/// A handle that cancels a live pairing from another task.
#[derive(Debug, Clone)]
pub struct PairingCancel(Connection);

impl PairingCancel {
    /// Closes the pairing connection.
    pub fn cancel(self) {
        self.0.close(VarInt::from_u32(0), b"cancelled");
    }
}

/// A connection to one paired computer.
pub struct AppConnection {
    connection: Connection,
}

impl fmt::Debug for AppConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConnection")
            .field("computer", &self.connection.remote_id().to_z32())
            .finish()
    }
}

impl AppConnection {
    /// Sends one allowlisted request and returns its response.
    ///
    /// # Errors
    ///
    /// Rejects a body or header over the limits, a transport failure, a
    /// timeout, a revoked device and a malformed response.
    pub async fn request(
        &self,
        method: Method,
        params: Option<Value>,
        body: Vec<u8>,
    ) -> Result<Response, ClientError> {
        let (mut send, mut recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|error| classify_closed(&error))?;
        let request = Request::new(method, params);
        let request = if body.is_empty() {
            request
        } else {
            request.with_body(body)
        };
        let frame = request.encode()?;
        send.write_all(&frame)
            .await
            .map_err(|_| ClientError::Transport("the request could not be sent".to_owned()))?;
        let _ = send.finish();
        timeout(UNARY_REQUEST_TIMEOUT, read_response(&mut recv))
            .await
            .map_err(|_| ClientError::Timeout)?
    }

    /// Checks a method name against the shared allowlist, then sends it.
    ///
    /// Callers that take a method name from the interface use this path, so
    /// an unknown name never reaches the computer.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Forbidden`] for a name outside the allowlist
    /// and the same failures as [`Self::request`].
    pub async fn request_named(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Response, ClientError> {
        let method = allowed_method(method)?;
        self.request(method, params, Vec::new()).await
    }

    /// Returns the computer endpoint id as z-base-32.
    pub fn computer_id(&self) -> String {
        self.connection.remote_id().to_z32()
    }

    /// Closes the connection.
    pub fn close(self) {
        self.connection.close(VarInt::from_u32(0), b"closed");
    }
}

/// The phone link client: identity, endpoint, pairing and app connections.
pub struct LinkClient {
    identity: DeviceIdentity,
    endpoint: Endpoint,
    store: Arc<dyn PhoneStore>,
}

impl fmt::Debug for LinkClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkClient")
            .field("identity", &self.identity)
            .field("endpoint", &self.endpoint.id().to_z32())
            .finish_non_exhaustive()
    }
}

impl LinkClient {
    /// Loads or creates the identity and binds the production endpoint.
    ///
    /// The endpoint uses the public relays and n0 discovery, as the design
    /// says. A missing relay does not stop the bind.
    ///
    /// # Errors
    ///
    /// Returns a store or transport failure.
    pub async fn start(store: Arc<dyn PhoneStore>) -> Result<Self, ClientError> {
        let identity = load_or_create_identity(store.as_ref())?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity.secret_key().clone())
            .alpns(vec![PAIR_ALPN.to_vec(), APP_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Ok(Self::from_endpoint(store, identity, endpoint))
    }

    /// Builds a client on an endpoint that is already bound.
    ///
    /// Tests use loopback endpoints. The endpoint must already carry the
    /// identity key and the pair and app ALPNs.
    pub fn from_endpoint(
        store: Arc<dyn PhoneStore>,
        identity: DeviceIdentity,
        endpoint: Endpoint,
    ) -> Self {
        Self {
            identity,
            endpoint,
            store,
        }
    }

    /// Returns the phone endpoint id as z-base-32.
    pub fn endpoint_id(&self) -> String {
        self.identity.endpoint_id_string()
    }

    /// Returns the stored computers.
    ///
    /// # Errors
    ///
    /// Returns a store failure.
    pub fn computers(&self) -> Result<Vec<ComputerRecord>, ClientError> {
        self.store
            .load_computers()
            .map_err(|error| ClientError::Store(error.to_string()))
    }

    /// Removes a computer from the local list.
    ///
    /// The computer keeps its own record until the user removes the phone
    /// there.
    ///
    /// # Errors
    ///
    /// Returns a store failure.
    pub fn forget(&self, computer_id: &str) -> Result<(), ClientError> {
        let mut computers = self.computers()?;
        computers.retain(|computer| computer.id != computer_id);
        self.store
            .save_computers(&computers)
            .map_err(|error| ClientError::Store(error.to_string()))
    }

    /// Dials the ticket endpoint and starts the pairing handshake.
    ///
    /// Returns once the computer shows its code. The caller shows the code
    /// and then calls [`PairingStart::finish`].
    ///
    /// # Errors
    ///
    /// Rejects an invalid or expired ticket, a connect failure, a timeout and
    /// a refusal.
    pub async fn pair(
        &self,
        ticket_text: &str,
        phone_name: &str,
        platform: &str,
    ) -> Result<PairingStart, ClientError> {
        self.pair_at(ticket_text, phone_name, platform, now_unix())
            .await
    }

    /// Starts pairing with an explicit clock, for tests.
    ///
    /// # Errors
    ///
    /// The same failures as [`Self::pair`]. An expired ticket is rejected
    /// before the phone dials.
    pub async fn pair_at(
        &self,
        ticket_text: &str,
        phone_name: &str,
        platform: &str,
        now: u64,
    ) -> Result<PairingStart, ClientError> {
        let ticket =
            PairTicket::decode_string(ticket_text).map_err(|_| ClientError::InvalidTicket)?;
        if ticket.is_expired(now) {
            return Err(ClientError::Expired);
        }

        let connection = timeout(
            UNARY_REQUEST_TIMEOUT,
            self.endpoint.connect(ticket.addr().clone(), PAIR_ALPN),
        )
        .await
        .map_err(|_| ClientError::Timeout)?
        .map_err(|error| ClientError::Transport(error.to_string()))?;
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| classify_closed(&error))?;

        let mut hello = PairHello {
            v: PROTOCOL_VERSION,
            secret: BASE32_NOPAD.encode(ticket.secret()),
            name: phone_name,
            platform,
            bridge_version: BRIDGE_VERSION,
            protocol_min: PROTOCOL_VERSION,
            protocol_max: PROTOCOL_VERSION,
        };
        write_json(&mut send, &hello).await?;
        hello.secret.zeroize();
        let _ = send.finish();

        let reply = timeout(
            UNARY_REQUEST_TIMEOUT,
            read_json::<PairReply>(&mut recv, MAX_PAIR_FRAME_BYTES),
        )
        .await
        .map_err(|_| ClientError::Timeout)??;

        let mut machine = PairingMachine::new(ticket.addr().clone(), now);
        machine.accept_confirm(&reply)?;
        match machine.state() {
            PairingState::AwaitingApproval(_) => Ok(PairingStart {
                connection,
                recv,
                machine,
                store: Arc::clone(&self.store),
            }),
            PairingState::Rejected(rejection) => {
                connection.close(VarInt::from_u32(0), b"rejected");
                Err(ClientError::Rejected(rejection.clone()))
            }
            _ => Err(ClientError::Protocol("pairing ended in an unknown state")),
        }
    }

    /// Opens the app connection to a paired computer.
    ///
    /// # Errors
    ///
    /// Returns a connect failure, a protocol mismatch or a revoked device.
    pub async fn connect(&self, computer: &ComputerRecord) -> Result<AppConnection, ClientError> {
        if computer.protocol_version != PROTOCOL_VERSION {
            return Err(ClientError::Protocol(
                "the computer protocol is not supported",
            ));
        }
        if computer.min_bridge_version > BRIDGE_VERSION {
            return Err(ClientError::Protocol("the phone app is too old"));
        }
        let connection = timeout(
            UNARY_REQUEST_TIMEOUT,
            self.endpoint.connect(computer.addr.clone(), APP_ALPN),
        )
        .await
        .map_err(|_| ClientError::Timeout)?
        .map_err(|error| ClientError::Transport(error.to_string()))?;
        Ok(AppConnection { connection })
    }

    /// Closes the endpoint.
    pub async fn shutdown(self) {
        self.endpoint.close().await;
    }
}

/// Loads the identity from the store, or creates and saves one.
///
/// # Errors
///
/// Returns a store failure or an invalid stored key.
pub fn load_or_create_identity(store: &dyn PhoneStore) -> Result<DeviceIdentity, ClientError> {
    match store
        .load_identity()
        .map_err(|error| ClientError::Store(error.to_string()))?
    {
        Some(bytes) => DeviceIdentity::from_slice(&bytes).map_err(ClientError::Link),
        None => {
            let identity = DeviceIdentity::generate();
            let bytes = identity.to_bytes();
            store
                .save_identity(&bytes)
                .map_err(|error| ClientError::Store(error.to_string()))?;
            Ok(identity)
        }
    }
}

fn save_computer(store: &dyn PhoneStore, record: ComputerRecord) -> Result<(), ClientError> {
    let mut computers = store
        .load_computers()
        .map_err(|error| ClientError::Store(error.to_string()))?;
    computers.retain(|computer| computer.id != record.id);
    computers.push(record);
    store
        .save_computers(&computers)
        .map_err(|error| ClientError::Store(error.to_string()))
}

fn allowed_method(name: &str) -> Result<Method, ClientError> {
    Method::parse(name).ok_or_else(|| ClientError::Forbidden(name.to_owned()))
}

fn valid_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|character| character.is_ascii_digit())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

async fn write_json<T: Serialize>(send: &mut SendStream, value: &T) -> Result<(), ClientError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| ClientError::Protocol("the pairing message is not valid"))?;
    if bytes.is_empty() || bytes.len() > MAX_PAIR_FRAME_BYTES {
        return Err(ClientError::TooLarge);
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|_| ClientError::Transport("the pairing message could not be sent".to_owned()))?;
    send.write_all(&bytes)
        .await
        .map_err(|_| ClientError::Transport("the pairing message could not be sent".to_owned()))
}

async fn read_json<T: for<'de> Deserialize<'de>>(
    recv: &mut RecvStream,
    max: usize,
) -> Result<T, ClientError> {
    let length = read_length(recv).await?;
    if length == 0 || length > max {
        return Err(ClientError::TooLarge);
    }
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| ClientError::ConnectionClosed)?;
    serde_json::from_slice(&bytes).map_err(|_| ClientError::Protocol("the message is not valid"))
}

async fn read_length(recv: &mut RecvStream) -> Result<usize, ClientError> {
    let mut bytes = [0u8; 4];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| ClientError::ConnectionClosed)?;
    Ok(u32::from_be_bytes(bytes) as usize)
}

fn declared_body_length(value: &Value) -> Result<usize, ClientError> {
    match value.get("bytes") {
        None | Some(Value::Null) => Ok(0),
        Some(value) => {
            let length = value.as_u64().ok_or(ClientError::Protocol(
                "the response body length is not valid",
            ))?;
            if length > MAX_UI_PACKAGE_BYTES as u64 {
                return Err(ClientError::TooLarge);
            }
            Ok(length as usize)
        }
    }
}

async fn read_response(recv: &mut RecvStream) -> Result<Response, ClientError> {
    let header_length = read_length(recv).await?;
    if header_length == 0 {
        return Err(ClientError::Protocol("the response header is empty"));
    }
    if header_length > MAX_HEADER_BYTES {
        return Err(ClientError::TooLarge);
    }
    let mut header = vec![0u8; header_length];
    recv.read_exact(&mut header)
        .await
        .map_err(|_| ClientError::ConnectionClosed)?;
    let value: Value = serde_json::from_slice(&header)
        .map_err(|_| ClientError::Protocol("the response header is not valid"))?;
    let body_length = declared_body_length(&value)?;
    let mut body = vec![0u8; body_length];
    recv.read_exact(&mut body)
        .await
        .map_err(|_| ClientError::ConnectionClosed)?;

    let mut frame = Vec::with_capacity(4 + header_length + body_length);
    frame.extend_from_slice(&(header_length as u32).to_be_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    Response::decode(&frame).map_err(ClientError::Link)
}

fn classify_closed(error: &ConnectionError) -> ClientError {
    match error {
        ConnectionError::ApplicationClosed(close) => {
            let code = close.error_code.into_inner();
            if code == u64::from(CLOSE_REVOKED) {
                return ClientError::Revoked;
            }
            if code == u64::from(CLOSE_NOT_PAIRED) {
                return ClientError::NotPaired;
            }
            ClientError::ConnectionClosed
        }
        ConnectionError::LocallyClosed | ConnectionError::TimedOut | ConnectionError::Reset => {
            ClientError::ConnectionClosed
        }
        _ => ClientError::Transport("the connection failed".to_owned()),
    }
}

/// A failure in the phone link client.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// The ticket text is not a valid pairing ticket.
    InvalidTicket,
    /// The ticket expired before the phone dialed.
    Expired,
    /// The computer refused the pairing.
    Rejected(PairingRejection),
    /// The computer removed this phone.
    Revoked,
    /// The computer does not know this phone key.
    NotPaired,
    /// The method is not on the phone allowlist.
    Forbidden(String),
    /// The frame or message is over its limit.
    TooLarge,
    /// The request or reply timed out.
    Timeout,
    /// The connection closed before the reply arrived.
    ConnectionClosed,
    /// The peer sent a message that does not fit the protocol.
    Protocol(&'static str),
    /// The platform store failed.
    Store(String),
    /// The transport failed.
    Transport(String),
    /// A shared link error.
    Link(LinkError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTicket => f.write_str("the pairing ticket is not valid"),
            Self::Expired => f.write_str("the pairing ticket expired"),
            Self::Rejected(rejection) => {
                write!(f, "the computer refused pairing: {}", rejection.status)
            }
            Self::Revoked => f.write_str("this computer removed this phone"),
            Self::NotPaired => f.write_str("this phone is not paired with the computer"),
            Self::Forbidden(method) => write!(f, "the method is not allowed: {method}"),
            Self::TooLarge => f.write_str("the link message is too large"),
            Self::Timeout => f.write_str("the link request timed out"),
            Self::ConnectionClosed => f.write_str("the connection closed"),
            Self::Protocol(reason) => write!(f, "the link message is not valid: {reason}"),
            Self::Store(message) => write!(f, "the phone store failed: {message}"),
            Self::Transport(message) => write!(f, "the link transport failed: {message}"),
            Self::Link(error) => write!(f, "the link message is not valid: {error}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Link(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LinkError> for ClientError {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    fn address() -> EndpointAddr {
        EndpointAddr::new(SecretKey::from_bytes(&[7; 32]).public())
    }

    fn confirm() -> PairReply {
        PairReply {
            v: PROTOCOL_VERSION,
            ok: true,
            status: Some("confirm".to_owned()),
            code: Some("428913".to_owned()),
            computer_id: Some("computer".to_owned()),
            computer_name: Some("Studio computer".to_owned()),
            protocol_version: Some(PROTOCOL_VERSION),
            min_bridge_version: Some(1),
            message: None,
        }
    }

    fn paired() -> PairReply {
        PairReply {
            v: PROTOCOL_VERSION,
            ok: true,
            status: Some("paired".to_owned()),
            ..PairReply::default()
        }
    }

    fn rejected(status: &str) -> PairReply {
        PairReply {
            v: PROTOCOL_VERSION,
            ok: false,
            status: Some(status.to_owned()),
            message: Some("refused".to_owned()),
            ..PairReply::default()
        }
    }

    #[test]
    fn confirm_moves_to_awaiting_approval() {
        let mut machine = PairingMachine::new(address(), 1_000);
        machine.accept_confirm(&confirm()).expect("confirm");
        match machine.state() {
            PairingState::AwaitingApproval(confirm) => {
                assert_eq!(confirm.code, "428913");
                assert_eq!(confirm.computer.id, "computer");
                assert_eq!(confirm.computer.name, "Studio computer");
                assert_eq!(confirm.computer.addr, address());
                assert_eq!(confirm.computer.paired_at, 1_000);
                assert_eq!(confirm.computer.protocol_version, PROTOCOL_VERSION);
                assert_eq!(confirm.computer.min_bridge_version, 1);
            }
            other => panic!("unexpected state: {other:?}"),
        }
    }

    #[test]
    fn paired_moves_to_paired_with_the_record() {
        let mut machine = PairingMachine::new(address(), 1_000);
        machine.accept_confirm(&confirm()).expect("confirm");
        machine.accept_final(&paired()).expect("paired");
        assert!(matches!(machine.state(), PairingState::Paired(record) if record.id == "computer"));
    }

    #[test]
    fn a_refused_confirmation_is_terminal() {
        let mut machine = PairingMachine::new(address(), 1_000);
        machine.accept_confirm(&rejected("expired")).expect("reply");
        assert_eq!(
            machine.state(),
            &PairingState::Rejected(PairingRejection {
                status: "expired".to_owned(),
                message: Some("refused".to_owned()),
            })
        );
    }

    #[test]
    fn a_refused_final_step_is_terminal() {
        let mut machine = PairingMachine::new(address(), 1_000);
        machine.accept_confirm(&confirm()).expect("confirm");
        machine.accept_final(&rejected("denied")).expect("reply");
        assert!(matches!(
            machine.state(),
            PairingState::Rejected(PairingRejection { status, .. }) if status == "denied"
        ));
    }

    #[test]
    fn replies_must_arrive_in_order() {
        let mut machine = PairingMachine::new(address(), 1_000);
        assert!(matches!(
            machine.accept_final(&paired()),
            Err(ClientError::Protocol(_))
        ));
        machine.accept_confirm(&confirm()).expect("confirm");
        assert!(matches!(
            machine.accept_confirm(&confirm()),
            Err(ClientError::Protocol(_))
        ));
        machine.accept_final(&paired()).expect("paired");
        assert!(matches!(
            machine.accept_final(&paired()),
            Err(ClientError::Protocol(_))
        ));
    }

    #[test]
    fn an_incomplete_confirmation_is_rejected() {
        let mut machine = PairingMachine::new(address(), 1_000);
        let mut reply = confirm();
        reply.code = Some("12345".to_owned());
        assert!(matches!(
            machine.accept_confirm(&reply),
            Err(ClientError::Protocol(_))
        ));

        let mut reply = confirm();
        reply.computer_id = None;
        assert!(matches!(
            machine.accept_confirm(&reply),
            Err(ClientError::Protocol(_))
        ));

        let mut reply = confirm();
        reply.protocol_version = None;
        assert!(matches!(
            machine.accept_confirm(&reply),
            Err(ClientError::Protocol(_))
        ));
    }

    #[test]
    fn another_protocol_version_is_rejected() {
        let mut machine = PairingMachine::new(address(), 1_000);
        let mut reply = confirm();
        reply.v = PROTOCOL_VERSION + 1;
        assert!(matches!(
            machine.accept_confirm(&reply),
            Err(ClientError::Protocol(_))
        ));
    }

    #[test]
    fn only_allowed_method_names_pass() {
        assert_eq!(
            allowed_method("bootstrap").expect("allowed"),
            Method::Bootstrap
        );
        assert_eq!(
            allowed_method("sessions.input").expect("allowed"),
            Method::SessionsInput
        );
        assert!(matches!(
            allowed_method("sessions.delete"),
            Err(ClientError::Forbidden(name)) if name == "sessions.delete"
        ));
        assert!(matches!(
            allowed_method("projects.create"),
            Err(ClientError::Forbidden(_))
        ));
    }

    #[test]
    fn every_allowlisted_method_is_accepted() {
        for method in Method::ALL {
            assert_eq!(allowed_method(method.as_str()).expect("allowed"), method);
        }
    }

    #[test]
    fn the_body_length_is_checked_before_allocation() {
        assert_eq!(
            declared_body_length(&serde_json::json!({})).expect("empty"),
            0
        );
        assert_eq!(
            declared_body_length(&serde_json::json!({"bytes": 12})).expect("length"),
            12
        );
        assert!(matches!(
            declared_body_length(&serde_json::json!({"bytes": MAX_UI_PACKAGE_BYTES + 1})),
            Err(ClientError::TooLarge)
        ));
        assert!(matches!(
            declared_body_length(&serde_json::json!({"bytes": "12"})),
            Err(ClientError::Protocol(_))
        ));
    }
}
