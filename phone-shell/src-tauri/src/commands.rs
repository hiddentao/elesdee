//! The bridge between the shell screens and the link client.
//!
//! Every method name from the screens is parsed against the shared allowlist
//! before it reaches the computer. The pairing screen shows the code while a
//! separate call waits for the user's answer on the computer.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use elesdee_link::{
    AppConnection, BRIDGE_VERSION, ClientError, ComputerRecord, LinkClient, Method,
    PROTOCOL_VERSION, PairingCancel, PairingStart, Response,
};
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;
use tauri::State;
use tokio::sync::Mutex;

use crate::store::ShellStore;

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

/// The live state of one shell process.
pub struct ShellState {
    inner: Mutex<PhoneShell>,
    pairing_cancelled: AtomicBool,
}

struct PhoneShell {
    client: Option<LinkClient>,
    startup_error: Option<CommandError>,
    pairing: Option<PairingStart>,
    pairing_cancel: Option<PairingCancel>,
    connections: HashMap<String, AppConnection>,
}

impl ShellState {
    fn ready(client: LinkClient) -> Self {
        Self {
            inner: Mutex::new(PhoneShell {
                client: Some(client),
                startup_error: None,
                pairing: None,
                pairing_cancel: None,
                connections: HashMap::new(),
            }),
            pairing_cancelled: AtomicBool::new(false),
        }
    }

    fn failed(code: &str, message: String) -> Self {
        Self {
            inner: Mutex::new(PhoneShell {
                client: None,
                startup_error: Some(CommandError::new(code, message)),
                pairing: None,
                pairing_cancel: None,
                connections: HashMap::new(),
            }),
            pairing_cancelled: AtomicBool::new(false),
        }
    }
}

impl PhoneShell {
    fn client(&self) -> Result<&LinkClient, CommandError> {
        self.client.as_ref().ok_or_else(|| {
            self.startup_error
                .clone()
                .unwrap_or_else(|| CommandError::new("startup", "The phone link did not start."))
        })
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
    /// True when the shell holds an open connection.
    pub connected: bool,
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
    let shell = state.inner.lock().await;
    let client = shell.client()?;
    let computers = client.computers().map_err(CommandError::from)?;
    Ok(computers
        .into_iter()
        .map(|computer| ComputerView {
            connected: shell.connections.contains_key(&computer.id),
            computer,
        })
        .collect())
}

#[tauri::command]
pub async fn phone_pair_start(
    state: State<'_, ShellState>,
    ticket: String,
    name: String,
    platform: String,
) -> Result<PairConfirm, CommandError> {
    let mut shell = state.inner.lock().await;
    if let Some(cancel) = shell.pairing_cancel.take() {
        cancel.cancel();
    }
    shell.pairing = None;
    state.pairing_cancelled.store(false, Ordering::SeqCst);

    let client = shell.client()?;
    let pairing = client
        .pair(&ticket, &name, &platform)
        .await
        .map_err(CommandError::from)?;
    let confirm = pairing.confirm().map_err(CommandError::from)?.clone();
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
    let mut shell = state.inner.lock().await;
    if let Some(connection) = shell.connections.remove(&computer_id) {
        connection.close();
    }
    let client = shell.client()?;
    client.forget(&computer_id).map_err(CommandError::from)
}

#[tauri::command]
pub async fn phone_connect(
    state: State<'_, ShellState>,
    computer_id: String,
) -> Result<ComputerRecord, CommandError> {
    let mut shell = state.inner.lock().await;
    let client = shell.client()?;
    let record = client
        .computers()
        .map_err(CommandError::from)?
        .into_iter()
        .find(|computer| computer.id == computer_id)
        .ok_or_else(|| CommandError::new("not_found", "This computer is not in the list."))?;
    let connection = client.connect(&record).await.map_err(CommandError::from)?;
    if let Some(previous) = shell.connections.insert(record.id.clone(), connection) {
        previous.close();
    }
    Ok(record)
}

#[tauri::command]
pub async fn phone_disconnect(
    state: State<'_, ShellState>,
    computer_id: String,
) -> Result<(), CommandError> {
    let mut shell = state.inner.lock().await;
    if let Some(connection) = shell.connections.remove(&computer_id) {
        connection.close();
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
    let shell = state.inner.lock().await;
    let method = Method::parse(&method).ok_or_else(|| {
        CommandError::new("forbidden", format!("The method is not allowed: {method}"))
    })?;
    let connection = shell
        .connections
        .get(&computer_id)
        .ok_or_else(|| CommandError::new("not_connected", "Connect to this computer first."))?;
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
            "unsupported",
            "A streaming response is not handled here.",
        )),
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
