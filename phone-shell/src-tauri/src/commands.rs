//! The bridge between the shell screens and the link client.
//!
//! Every method name from the screens is parsed against the shared allowlist
//! before it reaches the computer. The connection manager reconnects after a
//! drop with bounded delays, resumes each output stream from the last sequence
//! and stops on a revoked device. No state lock is held across a network wait.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use elesdee_link::{
    AppConnection, BRIDGE_VERSION, ClientError, ComputerRecord, EventStream, HistoryStream,
    LinkClient, Method, OutputStream, PROTOCOL_VERSION, PairingCancel, PairingStart,
    ReconnectPolicy, Response, is_terminal, port_allowed,
};
use serde::Serialize;
use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, watch};

use crate::forward::{ForwardAddress, ForwardRegistry, check_port};
use crate::store::ShellStore;

/// Reports a boolean field only when it is true.
fn is_false(value: &bool) -> bool {
    !*value
}

/// The Tauri event that reports connection state changes.
pub const CONNECTION_EVENT: &str = "phone:connection";

/// The Tauri event that carries one subscription value.
pub const SUBSCRIPTION_EVENT: &str = "phone:subscription";

/// A failure that the screens can explain in the phone's language.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// A stable code, such as `revoked` or `timeout`.
    pub code: String,
    /// The pairing status, when the computer refused the pairing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// A short diagnostic message. The screens use the code, not this text.
    pub message: String,
}

impl CommandError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            status: None,
            message: message.into(),
        }
    }
}

impl From<ClientError> for CommandError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::InvalidTicket => {
                Self::new("invalid_ticket", "The pairing ticket is not valid.")
            }
            ClientError::Expired => Self::new("expired", "The pairing ticket expired."),
            ClientError::Rejected(rejection) => Self {
                code: "rejected".to_owned(),
                status: Some(rejection.status),
                message: rejection.message.unwrap_or_default(),
            },
            ClientError::Revoked => Self::new("revoked", "This computer removed this phone."),
            ClientError::NotPaired => Self::new("not_paired", "This phone is not paired."),
            ClientError::Forbidden(method) => {
                Self::new("forbidden", format!("The method is not allowed: {method}"))
            }
            ClientError::Remote(body) => Self::new(body.code().as_str(), body.message().to_owned()),
            ClientError::TooLarge => Self::new("too_large", "The message is too large."),
            ClientError::Timeout => Self::new("timeout", "The computer did not answer."),
            ClientError::ConnectionClosed => {
                Self::new("connection_closed", "The connection closed.")
            }
            ClientError::Protocol(reason) => Self::new("protocol", reason),
            ClientError::Store(message) => Self::new("store", message),
            ClientError::Transport(message) => Self::new("transport", message),
            ClientError::Link(error) => Self::new(error.code().as_str(), error.to_string()),
            _ => Self::new("unknown", error.to_string()),
        }
    }
}

/// A message for the screens about one computer connection.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    /// The computer endpoint id.
    pub computer_id: String,
    /// One of `connected`, `disconnected`, `reconnecting` or `stopped`.
    pub state: &'static str,
    /// The failure, when the state is `disconnected` or `stopped`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
}

/// One value from a live event or output subscription.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionValue {
    /// The subscription id chosen by the screens.
    pub id: String,
    /// The event value, when the computer sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
    /// The failure, when the subscription stopped for an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
    /// True when the computer ended the stream.
    #[serde(skip_serializing_if = "is_false")]
    pub ended: bool,
}

/// The live state of one shell process.
pub struct ShellState {
    inner: Mutex<PhoneShell>,
    pairing_gate: Mutex<()>,
    pairing_cancelled: AtomicBool,
    forwards: ForwardRegistry,
}

struct PhoneShell {
    client: Option<Arc<LinkClient>>,
    startup_error: Option<CommandError>,
    pairing: Option<PairingStart>,
    pairing_cancel: Option<PairingCancel>,
    managers: HashMap<String, Arc<ConnectionManager>>,
    subscriptions: HashMap<String, Subscription>,
}

struct Subscription {
    computer_id: String,
    handle: JoinHandle<()>,
}

impl ShellState {
    fn ready(client: LinkClient) -> Self {
        Self {
            inner: Mutex::new(PhoneShell {
                client: Some(Arc::new(client)),
                startup_error: None,
                pairing: None,
                pairing_cancel: None,
                managers: HashMap::new(),
                subscriptions: HashMap::new(),
            }),
            pairing_gate: Mutex::new(()),
            pairing_cancelled: AtomicBool::new(false),
            forwards: ForwardRegistry::new(),
        }
    }

    fn failed(code: &str, message: String) -> Self {
        Self {
            inner: Mutex::new(PhoneShell {
                client: None,
                startup_error: Some(CommandError::new(code, message)),
                pairing: None,
                pairing_cancel: None,
                managers: HashMap::new(),
                subscriptions: HashMap::new(),
            }),
            pairing_gate: Mutex::new(()),
            pairing_cancelled: AtomicBool::new(false),
            forwards: ForwardRegistry::new(),
        }
    }
}

impl PhoneShell {
    fn client(&self) -> Result<Arc<LinkClient>, CommandError> {
        self.client.as_ref().map(Arc::clone).ok_or_else(|| {
            self.startup_error
                .clone()
                .unwrap_or_else(|| CommandError::new("startup", "The phone link did not start."))
        })
    }

    fn cancel_subscriptions(&mut self, computer_id: &str) {
        let ids = self
            .subscriptions
            .iter()
            .filter(|(_, subscription)| subscription.computer_id == computer_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(subscription) = self.subscriptions.remove(&id) {
                subscription.handle.abort();
            }
        }
    }
}

/// Starts the identity, the keychain and the link endpoint.
pub fn init_state(app: &tauri::AppHandle) -> ShellState {
    if let Err(error) = ShellStore::install_keychain() {
        return ShellState::failed("keychain", error);
    }
    let data_dir = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(error) => {
            return ShellState::failed(
                "storage",
                format!("The app data folder is not available: {error}"),
            );
        }
    };
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        return ShellState::failed(
            "storage",
            format!("The app data folder could not be created: {error}"),
        );
    }
    let store = Arc::new(ShellStore::new(data_dir));
    match tauri::async_runtime::block_on(LinkClient::start(store)) {
        Ok(client) => ShellState::ready(client),
        Err(error) => {
            let converted = CommandError::from(error);
            ShellState::failed(&converted.code, converted.message)
        }
    }
}

/// The phone identity details that the screens show.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneInfo {
    /// The phone endpoint id as z-base-32.
    pub phone_id: String,
    /// The phone platform, such as `android`.
    pub platform: String,
    /// The shell bridge version.
    pub bridge_version: u32,
    /// The protocol version that the shell speaks.
    pub protocol_version: u8,
}

/// The confirmation details for the pairing screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairConfirm {
    /// The six-digit code that both devices show.
    pub code: String,
    /// The computer identity.
    pub computer_id: String,
    /// The computer name.
    pub computer_name: String,
}

/// One stored computer and its live state.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerView {
    /// The stored computer record.
    pub computer: ComputerRecord,
    /// True when the shell holds an open or reconnecting manager.
    pub connected: bool,
}

/// The result of opening a live subscription.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionOpened {
    /// The subscription id chosen by the screens.
    pub id: String,
}

#[tauri::command]
pub async fn phone_info(state: State<'_, ShellState>) -> Result<PhoneInfo, CommandError> {
    let shell = state.inner.lock().await;
    let client = shell.client()?;
    Ok(PhoneInfo {
        phone_id: client.endpoint_id(),
        platform: platform().to_owned(),
        bridge_version: BRIDGE_VERSION,
        protocol_version: PROTOCOL_VERSION,
    })
}

#[tauri::command]
pub async fn phone_computers(
    state: State<'_, ShellState>,
) -> Result<Vec<ComputerView>, CommandError> {
    let (client, managers) = {
        let shell = state.inner.lock().await;
        (shell.client()?, shell.managers.clone())
    };
    let computers = client.computers().map_err(CommandError::from)?;
    let mut views = Vec::with_capacity(computers.len());
    for computer in computers {
        let connected = match managers.get(&computer.id) {
            Some(manager) => manager.connection().await.is_some(),
            None => false,
        };
        views.push(ComputerView {
            computer,
            connected,
        });
    }
    Ok(views)
}

#[tauri::command]
pub async fn phone_pair_start(
    state: State<'_, ShellState>,
    ticket: String,
    name: String,
    platform: String,
) -> Result<PairConfirm, CommandError> {
    let _gate = state.pairing_gate.lock().await;
    state.pairing_cancelled.store(false, Ordering::SeqCst);
    let client = {
        let mut shell = state.inner.lock().await;
        if let Some(cancel) = shell.pairing_cancel.take() {
            cancel.cancel();
        }
        shell.pairing = None;
        shell.client()?
    };

    let pairing = client
        .pair(&ticket, &name, &platform)
        .await
        .map_err(CommandError::from)?;
    let confirm = pairing.confirm().map_err(CommandError::from)?.clone();
    let mut shell = state.inner.lock().await;
    shell.pairing_cancel = Some(pairing.cancel_handle());
    shell.pairing = Some(pairing);
    Ok(PairConfirm {
        code: confirm.code,
        computer_id: confirm.computer.id,
        computer_name: confirm.computer.name,
    })
}

#[tauri::command]
pub async fn phone_pair_finish(
    state: State<'_, ShellState>,
) -> Result<ComputerRecord, CommandError> {
    let _gate = state.pairing_gate.lock().await;
    let pairing = {
        let mut shell = state.inner.lock().await;
        shell
            .pairing
            .take()
            .ok_or_else(|| CommandError::new("no_pairing", "No pairing is in progress."))?
    };

    let result = pairing.finish().await;

    let mut shell = state.inner.lock().await;
    shell.pairing_cancel = None;
    drop(shell);

    match result {
        Ok(record) => Ok(record),
        Err(error) => {
            if state.pairing_cancelled.swap(false, Ordering::SeqCst) {
                Err(CommandError::new("cancelled", "Pairing was cancelled."))
            } else {
                Err(CommandError::from(error))
            }
        }
    }
}

#[tauri::command]
pub async fn phone_pair_cancel(state: State<'_, ShellState>) -> Result<(), CommandError> {
    let mut shell = state.inner.lock().await;
    state.pairing_cancelled.store(true, Ordering::SeqCst);
    shell.pairing = None;
    if let Some(cancel) = shell.pairing_cancel.take() {
        cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn phone_forget(
    state: State<'_, ShellState>,
    computer_id: String,
) -> Result<(), CommandError> {
    state.forwards.stop_for_computer(&computer_id);
    let (manager, client) = {
        let mut shell = state.inner.lock().await;
        shell.cancel_subscriptions(&computer_id);
        (shell.managers.remove(&computer_id), shell.client()?)
    };
    if let Some(manager) = manager {
        manager.stop().await;
    }
    client.forget(&computer_id).map_err(CommandError::from)
}

#[tauri::command]
pub async fn phone_connect(
    state: State<'_, ShellState>,
    app: AppHandle,
    computer_id: String,
) -> Result<ComputerRecord, CommandError> {
    let (client, record) = {
        let mut shell = state.inner.lock().await;
        let client = shell.client()?;
        if let Some(manager) = shell.managers.get(&computer_id) {
            if !manager.is_terminal() {
                return Ok(manager.record().clone());
            }
            shell.managers.remove(&computer_id);
        }
        let record = client
            .computers()
            .map_err(CommandError::from)?
            .into_iter()
            .find(|computer| computer.id == computer_id)
            .ok_or_else(|| CommandError::new("not_found", "This computer is not in the list."))?;
        (client, record)
    };

    let manager = ConnectionManager::new(client, record.clone());
    {
        let mut shell = state.inner.lock().await;
        if let Some(existing) = shell.managers.get(&computer_id) {
            return Ok(existing.record().clone());
        }
        shell
            .managers
            .insert(computer_id.clone(), Arc::clone(&manager));
    }

    if let Err(error) = manager.connect_first(&app).await {
        let mut shell = state.inner.lock().await;
        shell.managers.remove(&computer_id);
        return Err(error);
    }
    manager.spawn_supervisor(app).await;
    Ok(record)
}

#[tauri::command]
pub async fn phone_disconnect(
    state: State<'_, ShellState>,
    computer_id: String,
) -> Result<(), CommandError> {
    state.forwards.stop_for_computer(&computer_id);
    let manager = {
        let mut shell = state.inner.lock().await;
        shell.cancel_subscriptions(&computer_id);
        shell.managers.remove(&computer_id)
    };
    if let Some(manager) = manager {
        manager.stop().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn phone_request(
    state: State<'_, ShellState>,
    computer_id: String,
    method: String,
    params: Option<Value>,
) -> Result<Value, CommandError> {
    let method = Method::parse(&method).ok_or_else(|| {
        CommandError::new("forbidden", format!("The method is not allowed: {method}"))
    })?;
    if method.is_streaming() {
        return Err(CommandError::new(
            "streaming",
            "Use a subscription for this method.",
        ));
    }
    let manager = {
        let shell = state.inner.lock().await;
        shell.managers.get(&computer_id).map(Arc::clone)
    }
    .ok_or_else(|| CommandError::new("not_connected", "Connect to this computer first."))?;
    let connection = manager.connection().await.ok_or_else(|| {
        CommandError::new("not_connected", "The computer connection is not ready.")
    })?;
    let response = connection
        .request(method, params, Vec::new())
        .await
        .map_err(CommandError::from)?;
    match response {
        Response::Ok { result, .. } => Ok(result),
        Response::Error(body) => Err(CommandError::new(
            body.code().as_str(),
            body.message().to_owned(),
        )),
        Response::StreamStart => Err(CommandError::new(
            "streaming",
            "Use a subscription for this method.",
        )),
    }
}

/// Opens a live event or output subscription.
///
/// The screens choose the id. They listen for the same id before they call
/// this command, so no value is lost between the call and the listener.
#[tauri::command]
pub async fn phone_subscribe(
    state: State<'_, ShellState>,
    app: AppHandle,
    computer_id: String,
    id: String,
    channel: String,
    params: Option<Value>,
) -> Result<SubscriptionOpened, CommandError> {
    let channel = Channel::parse(&channel)?;
    let session_id = match channel {
        Channel::Events => None,
        Channel::Output | Channel::History => Some(
            params
                .as_ref()
                .and_then(|params| params.get("sessionId"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    CommandError::new("bad_request", "The session stream needs a session id.")
                })?
                .to_owned(),
        ),
    };
    let after = params
        .as_ref()
        .and_then(|params| params.get("after"))
        .and_then(Value::as_u64)
        .unwrap_or_default();

    let manager = {
        let mut shell = state.inner.lock().await;
        if let Some(previous) = shell.subscriptions.remove(&id) {
            previous.handle.abort();
        }
        shell.managers.get(&computer_id).map(Arc::clone)
    }
    .ok_or_else(|| CommandError::new("not_connected", "Connect to this computer first."))?;

    let last_seq = Arc::new(AtomicU64::new(after));
    let generation = manager.generation();
    let task_owner = Arc::clone(&manager);
    let task_id = id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_subscription(
            app, task_owner, task_id, channel, session_id, last_seq, generation,
        )
        .await;
    });

    let mut shell = state.inner.lock().await;
    shell.subscriptions.insert(
        id.clone(),
        Subscription {
            computer_id,
            handle,
        },
    );
    Ok(SubscriptionOpened { id })
}

/// Closes a live subscription.
#[tauri::command]
pub async fn phone_unsubscribe(
    state: State<'_, ShellState>,
    id: String,
) -> Result<(), CommandError> {
    let mut shell = state.inner.lock().await;
    if let Some(subscription) = shell.subscriptions.remove(&id) {
        subscription.handle.abort();
    }
    Ok(())
}

/// Starts the phone listener for one registered demo-server port.
///
/// The port is checked against the computer's active list before the listener
/// binds, so the screens cannot forward another port.
#[tauri::command]
pub async fn phone_forward_start(
    state: State<'_, ShellState>,
    computer_id: String,
    port: u16,
) -> Result<ForwardAddress, CommandError> {
    let (client, record, connection) = {
        let shell = state.inner.lock().await;
        let client = shell.client()?;
        let record = client
            .computers()
            .map_err(CommandError::from)?
            .into_iter()
            .find(|computer| computer.id == computer_id)
            .ok_or_else(|| CommandError::new("not_found", "This computer is not in the list."))?;
        let manager = shell
            .managers
            .get(&computer_id)
            .map(Arc::clone)
            .ok_or_else(|| CommandError::new("not_connected", "Connect to this computer first."))?;
        (client, record, manager)
    };
    let connection = connection.connection().await.ok_or_else(|| {
        CommandError::new("not_connected", "The computer connection is not ready.")
    })?;
    let response = connection
        .request(Method::DemoServersList, None, Vec::new())
        .await
        .map_err(CommandError::from)?;
    let list = match response {
        Response::Ok { result, .. } => result,
        Response::Error(body) => {
            return Err(CommandError::new(
                body.code().as_str(),
                body.message().to_owned(),
            ));
        }
        Response::StreamStart => {
            return Err(CommandError::new(
                "streaming",
                "The demo-server list must not stream.",
            ));
        }
    };
    let registered = registered_ports(&list);
    check_port(port, &registered).map_err(|error| {
        CommandError::new(error.code(), "That port is not an active demo server.")
    })?;
    if let Some(address) = state.forwards.address(&computer_id, port) {
        return Ok(address);
    }
    let forward = client.forward(&record).await.map_err(CommandError::from)?;
    state
        .forwards
        .start(&computer_id, port, &registered, forward)
        .await
        .map_err(|error| CommandError::new(error.code(), "The demo-server link did not start."))
}

/// Stops one demo-server listener.
#[tauri::command]
pub async fn phone_forward_stop(
    state: State<'_, ShellState>,
    port: u16,
) -> Result<(), CommandError> {
    state.forwards.stop(port);
    Ok(())
}

/// Selects the active demo-server ports from a `demoServers.list` result.
fn registered_ports(value: &Value) -> HashSet<u16> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            if entry.get("state").and_then(Value::as_str) != Some("active") {
                return None;
            }
            let port = u16::try_from(entry.get("port").and_then(Value::as_u64)?).ok()?;
            port_allowed(port).then_some(port)
        })
        .collect()
}

/// Stops every demo-server listener at app exit.
pub fn shutdown(state: &ShellState) {
    state.forwards.close_all();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Events,
    Output,
    History,
}

impl Channel {
    fn parse(value: &str) -> Result<Self, CommandError> {
        match value {
            "events" => Ok(Self::Events),
            "output" => Ok(Self::Output),
            "history" => Ok(Self::History),
            _ => Err(CommandError::new(
                "bad_request",
                "The subscription channel is not valid.",
            )),
        }
    }
}

async fn run_subscription(
    app: AppHandle,
    manager: Arc<ConnectionManager>,
    id: String,
    channel: Channel,
    session_id: Option<String>,
    last_seq: Arc<AtomicU64>,
    mut generation: watch::Receiver<u64>,
) {
    loop {
        if manager.is_terminal() {
            return;
        }
        let Some(connection) = manager.connection().await else {
            if generation.changed().await.is_err() {
                return;
            }
            continue;
        };

        match open_stream(&connection, channel, session_id.as_deref(), &last_seq).await {
            Ok(mut stream) => {
                let mut clean_end = false;
                loop {
                    match stream.next().await {
                        Ok(Some(value)) => {
                            if let Some(seq) = value.get("seq").and_then(Value::as_u64) {
                                last_seq.store(seq, Ordering::SeqCst);
                            }
                            emit_subscription(
                                &app,
                                SubscriptionValue {
                                    id: id.clone(),
                                    event: Some(value),
                                    error: None,
                                    ended: false,
                                },
                            );
                        }
                        Ok(None) => {
                            clean_end = true;
                            emit_subscription(
                                &app,
                                SubscriptionValue {
                                    id: id.clone(),
                                    event: None,
                                    error: None,
                                    ended: true,
                                },
                            );
                            break;
                        }
                        Err(error) => {
                            let terminal = is_terminal(&error);
                            emit_subscription(
                                &app,
                                SubscriptionValue {
                                    id: id.clone(),
                                    event: None,
                                    error: Some(CommandError::from(error)),
                                    ended: false,
                                },
                            );
                            if terminal {
                                return;
                            }
                            break;
                        }
                    }
                }
                if clean_end {
                    // A closed session stream stays closed. An event stream
                    // that fell behind is reopened on the same connection.
                    if matches!(channel, Channel::Output | Channel::History) {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            }
            Err(error) => {
                let terminal = is_terminal(&error);
                emit_subscription(
                    &app,
                    SubscriptionValue {
                        id: id.clone(),
                        event: None,
                        error: Some(CommandError::from(error)),
                        ended: false,
                    },
                );
                if terminal {
                    return;
                }
            }
        }

        if manager.is_terminal() {
            return;
        }
        // Wait for the supervisor to install a new connection. The next
        // output subscription replays from the last sequence seen.
        if generation.changed().await.is_err() {
            return;
        }
    }
}

enum LiveStream {
    Events(EventStream),
    Output(OutputStream),
    History(HistoryStream),
}

impl LiveStream {
    async fn next(&mut self) -> Result<Option<Value>, ClientError> {
        match self {
            Self::Events(stream) => stream.next().await,
            Self::Output(stream) => stream.next().await,
            Self::History(stream) => stream.next().await,
        }
    }
}

async fn open_stream(
    connection: &AppConnection,
    channel: Channel,
    session_id: Option<&str>,
    last_seq: &AtomicU64,
) -> Result<LiveStream, ClientError> {
    match channel {
        Channel::Events => Ok(LiveStream::Events(connection.events().await?)),
        Channel::Output => {
            let session_id =
                session_id.ok_or(ClientError::Protocol("the session id is missing"))?;
            Ok(LiveStream::Output(
                connection
                    .output(session_id, last_seq.load(Ordering::SeqCst))
                    .await?,
            ))
        }
        Channel::History => {
            let session_id =
                session_id.ok_or(ClientError::Protocol("the session id is missing"))?;
            Ok(LiveStream::History(
                connection
                    .history(session_id, last_seq.load(Ordering::SeqCst))
                    .await?,
            ))
        }
    }
}

fn emit_subscription(app: &AppHandle, value: SubscriptionValue) {
    if let Err(error) = app.emit(SUBSCRIPTION_EVENT, value) {
        tracing::warn!(%error, "could not emit a subscription value");
    }
}

fn emit_connection(app: &AppHandle, state: ConnectionState) {
    if let Err(error) = app.emit(CONNECTION_EVENT, state) {
        tracing::warn!(%error, "could not emit a connection state");
    }
}

/// One live connection with its reconnect loop and its subscriptions.
struct ConnectionManager {
    client: Arc<LinkClient>,
    record: ComputerRecord,
    current: Mutex<Option<Arc<AppConnection>>>,
    generation: watch::Sender<u64>,
    terminal: AtomicBool,
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

impl ConnectionManager {
    fn new(client: Arc<LinkClient>, record: ComputerRecord) -> Arc<Self> {
        let (generation, _) = watch::channel(0);
        Arc::new(Self {
            client,
            record,
            current: Mutex::new(None),
            generation,
            terminal: AtomicBool::new(false),
            supervisor: Mutex::new(None),
        })
    }

    fn record(&self) -> &ComputerRecord {
        &self.record
    }

    async fn connection(&self) -> Option<Arc<AppConnection>> {
        self.current.lock().await.clone()
    }

    fn generation(&self) -> watch::Receiver<u64> {
        self.generation.subscribe()
    }

    fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::SeqCst)
    }

    async fn connect_first(self: &Arc<Self>, app: &AppHandle) -> Result<(), CommandError> {
        let connection = self
            .client
            .connect(&self.record)
            .await
            .map_err(CommandError::from)?;
        self.install(Arc::new(connection)).await;
        emit_connection(
            app,
            ConnectionState {
                computer_id: self.record.id.clone(),
                state: "connected",
                error: None,
            },
        );
        Ok(())
    }

    async fn install(&self, connection: Arc<AppConnection>) {
        *self.current.lock().await = Some(connection);
        self.generation.send_modify(|generation| *generation += 1);
    }

    async fn clear(&self) {
        *self.current.lock().await = None;
        self.generation.send_modify(|generation| *generation += 1);
    }

    async fn spawn_supervisor(self: Arc<Self>, app: AppHandle) {
        let manager = Arc::clone(&self);
        let handle = tauri::async_runtime::spawn(async move {
            let mut policy = ReconnectPolicy::new();
            loop {
                if manager.is_terminal() {
                    break;
                }
                let Some(connection) = manager.connection().await else {
                    match manager.client.connect(&manager.record).await {
                        Ok(connection) => {
                            manager.install(Arc::new(connection)).await;
                            emit_connection(
                                &app,
                                ConnectionState {
                                    computer_id: manager.record.id.clone(),
                                    state: "connected",
                                    error: None,
                                },
                            );
                            continue;
                        }
                        Err(error) => {
                            let terminal = is_terminal(&error);
                            emit_connection(
                                &app,
                                ConnectionState {
                                    computer_id: manager.record.id.clone(),
                                    state: if terminal { "stopped" } else { "disconnected" },
                                    error: Some(CommandError::from(error)),
                                },
                            );
                            if terminal {
                                manager.terminal.store(true, Ordering::SeqCst);
                                break;
                            }
                            emit_connection(
                                &app,
                                ConnectionState {
                                    computer_id: manager.record.id.clone(),
                                    state: "reconnecting",
                                    error: None,
                                },
                            );
                            tokio::time::sleep(policy.next_delay()).await;
                            continue;
                        }
                    }
                };

                let connected_at = Instant::now();
                let reason = connection.wait_closed().await;
                manager.clear().await;
                let terminal = is_terminal(&reason);
                emit_connection(
                    &app,
                    ConnectionState {
                        computer_id: manager.record.id.clone(),
                        state: if terminal { "stopped" } else { "disconnected" },
                        error: Some(CommandError::from(reason)),
                    },
                );
                if terminal {
                    manager.terminal.store(true, Ordering::SeqCst);
                    break;
                }
                policy.record_connection(connected_at.elapsed());
                emit_connection(
                    &app,
                    ConnectionState {
                        computer_id: manager.record.id.clone(),
                        state: "reconnecting",
                        error: None,
                    },
                );
                tokio::time::sleep(policy.next_delay()).await;
            }
        });
        *self.supervisor.lock().await = Some(handle);
    }

    async fn stop(&self) {
        self.terminal.store(true, Ordering::SeqCst);
        if let Some(handle) = self.supervisor.lock().await.take() {
            handle.abort();
        }
        if let Some(connection) = self.current.lock().await.take() {
            connection.close();
        }
        self.generation.send_modify(|generation| *generation += 1);
    }
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "desktop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_channels_parse() {
        assert_eq!(Channel::parse("events").expect("events"), Channel::Events);
        assert_eq!(Channel::parse("output").expect("output"), Channel::Output);
        assert_eq!(
            Channel::parse("history").expect("history"),
            Channel::History
        );
        assert!(Channel::parse("terminal").is_err());
    }

    #[test]
    fn a_revoked_error_is_terminal() {
        assert!(is_terminal(&ClientError::Revoked));
        assert!(is_terminal(&ClientError::NotPaired));
        assert!(!is_terminal(&ClientError::ConnectionClosed));
    }

    #[test]
    fn only_active_demo_server_ports_are_forwardable() {
        let list = serde_json::json!([
            {"sessionId": "s1", "port": 3000, "state": "active"},
            {"sessionId": "s1", "port": 4000, "state": "stopped"},
            {"sessionId": "s2", "port": 5000, "state": "active"},
            {"sessionId": "s2", "port": 80, "state": "active"},
            {"sessionId": "s2", "port": "6000", "state": "active"}
        ]);
        let ports = registered_ports(&list);
        assert_eq!(ports, HashSet::from([3000, 5000]));
        assert!(!ports.contains(&4000));
        assert!(!ports.contains(&80));
        assert_eq!(registered_ports(&serde_json::json!({})), HashSet::new());
    }
}
