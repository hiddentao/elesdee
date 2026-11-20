use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use portable_pty::CommandBuilder;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::sessions::SessionEntry;
use crate::state::{AppState, PendingPermission, SessionEvent};

pub struct Decision {
    pub request_id: String,
    pub allow: bool,
}

pub struct CodexServer {
    pub url: String,
    child: Option<Child>,
}

impl Drop for CodexServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn free_port() -> Option<u16> {
    if let Ok(value) = std::env::var("ELESDEE_CODEX_PORT")
        && let Ok(port) = value.trim().parse::<u16>()
        && port > 0
    {
        return Some(port);
    }
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|address| address.port())
}

pub fn external_url() -> Option<String> {
    std::env::var("ELESDEE_CODEX_WS")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn start_server(port: u16) -> Result<CodexServer, String> {
    let url = format!("ws://127.0.0.1:{port}");
    let child = Command::new("codex")
        .args(["app-server", "--listen", url.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start the Codex app server: {error}"))?;
    Ok(CodexServer {
        url,
        child: Some(child),
    })
}

pub fn launch_args(command: &mut CommandBuilder, url: &str) {
    command.arg("--remote");
    command.arg(url);
}

const RATE_LIMITS_REQUEST: &str = "elesdee-rate-limits";

pub async fn run(
    url: String,
    entry: Arc<SessionEntry>,
    mut decisions: mpsc::UnboundedReceiver<Decision>,
    state: AppState,
) {
    let socket = connect(&url).await;
    let Ok(socket) = socket else {
        tracing::warn!(session = %entry.id(), %url, "could not connect to the Codex app server");
        return;
    };
    let account_id = entry.session().account_id;
    let (mut writer, mut reader) = socket.split();
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": { "name": "elesdee", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": true, "requiredHumanApproval": true }
        }
    });
    let initialized = json!({ "jsonrpc": "2.0", "method": "initialized" });
    let read_limits = json!({
        "jsonrpc": "2.0",
        "id": RATE_LIMITS_REQUEST,
        "method": "account/rateLimits/read",
        "params": {}
    });
    for message in [initialize, initialized, read_limits] {
        if writer
            .send(Message::Text(message.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            decision = decisions.recv() => {
                let Some(decision) = decision else { break };
                let Some(response) = decision_response(&decision) else { continue };
                if writer.send(Message::Text(response.to_string().into())).await.is_err() {
                    break;
                }
            }
            message = reader.next() => {
                let Some(Ok(message)) = message else { break };
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };

                if value.get("method").is_none() {
                    let is_rate_reply =
                        value.get("id").and_then(Value::as_str) == Some(RATE_LIMITS_REQUEST);
                    if is_rate_reply
                        && let Some(account_id) = account_id.as_deref()
                    {
                        let rows = crate::usage::codex_rows(&value);
                        if !rows.is_empty() {
                            crate::usage::apply_usage(
                                &state,
                                account_id,
                                crate::usage::APP_SERVER,
                                rows,
                            )
                            .await;
                        }
                    }
                    continue;
                }

                let Some(method) = value.get("method").and_then(Value::as_str) else { continue };
                if method == "account/rateLimits/updated"
                    && let Some(account_id) = account_id.as_deref()
                {
                    let rows = crate::usage::codex_rows(&value);
                    if !rows.is_empty() {
                        crate::usage::apply_usage(
                            &state,
                            account_id,
                            crate::usage::APP_SERVER,
                            rows,
                        )
                        .await;
                    }
                }
                if let Some(thread_id) = thread_id(method, value.get("params"))
                    && let Some(session) = entry.note_agent_session_id(thread_id)
                {
                    state.emit(SessionEvent::updated(session));
                    state.save();
                }
                let Some(id) = value.get("id") else { continue };
                let Some(permission) = approval(method, value.get("params")) else { continue };
                let request_id = id_key(id);
                entry.mark_external_permission(PendingPermission {
                    request_id,
                    tool: permission.0,
                    summary: permission.1,
                });
            }
        }
    }
}

fn thread_id<'a>(method: &str, params: Option<&'a Value>) -> Option<&'a str> {
    let params = params?;
    let value = if method == "thread/started" {
        params
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
    } else {
        params.get("threadId").and_then(Value::as_str)
    }
    .or_else(|| params.get("threadId").and_then(Value::as_str))
    .or_else(|| {
        params
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
    })?;

    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= 128 && !value.chars().any(char::is_control))
        .then_some(value)
}

async fn connect(
    url: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    (),
> {
    for _ in 0..20 {
        match tokio_tungstenite::connect_async(url).await {
            Ok((socket, _)) => return Ok(socket),
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    Err(())
}

fn approval(method: &str, params: Option<&Value>) -> Option<(String, String)> {
    let params = params?;
    let tool = if method.contains("fileChange") || method.contains("applyPatch") {
        "Edit"
    } else if method.contains("commandExecution")
        || method.contains("execCommand")
        || method.contains("permissions")
    {
        "Bash"
    } else {
        return None;
    };
    let summary = params
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| params.get("reason").and_then(Value::as_str))
        .or_else(|| params.get("itemId").and_then(Value::as_str))
        .unwrap_or(method)
        .to_owned();
    Some((tool.to_owned(), summary))
}

fn id_key(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn decision_response(decision: &Decision) -> Option<Value> {
    let id = if let Ok(number) = decision.request_id.parse::<i64>() {
        json!(number)
    } else {
        json!(decision.request_id)
    };
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "decision": if decision.allow { "accept" } else { "decline" }
        }
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::sessions::{OutputBudget, SessionRuntimeConfig};
    use crate::state::{
        Account, Auth, Client, Session, SessionKind, SessionState, UsageStatus, WorkState,
    };

    #[test]
    fn approval_maps_methods_to_tools_and_keeps_the_reply_id() {
        let params = json!({ "command": "rm -rf build", "itemId": "item-1" });
        assert_eq!(
            approval("item/commandExecution/requestApproval", Some(&params)),
            Some(("Bash".to_owned(), "rm -rf build".to_owned()))
        );
        assert_eq!(
            approval("item/fileChange/requestApproval", Some(&params)),
            Some(("Edit".to_owned(), "rm -rf build".to_owned()))
        );
        assert_eq!(id_key(&json!(42)), "42");
        let response = decision_response(&Decision {
            request_id: "42".to_owned(),
            allow: true,
        })
        .expect("response");
        assert_eq!(response["id"], 42);
        assert_eq!(response["result"]["decision"], "accept");
    }

    #[test]
    fn thread_ids_come_from_started_notifications_and_approval_params() {
        let started = json!({
            "method": "thread/started",
            "params": { "thread": { "id": "019c-thread" } }
        });
        assert_eq!(
            thread_id("thread/started", started.get("params")),
            Some("019c-thread")
        );

        let approval = json!({
            "method": "item/commandExecution/requestApproval",
            "params": { "threadId": "turn-thread", "itemId": "item-1" }
        });
        assert_eq!(
            thread_id(
                "item/commandExecution/requestApproval",
                approval.get("params")
            ),
            Some("turn-thread")
        );
        assert_eq!(thread_id("turn/started", Some(&json!({}))), None);
        assert_eq!(
            thread_id("turn/started", Some(&json!({ "threadId": " " }))),
            None
        );
    }

    async fn next_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> Value {
        loop {
            let message = socket.next().await.expect("message").expect("socket");
            if let Message::Text(text) = message {
                return serde_json::from_str(&text).expect("json");
            }
        }
    }

    #[tokio::test]
    async fn the_app_server_read_and_update_feed_the_account() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("handshake");
            assert_eq!(next_json(&mut socket).await["method"], "initialize");
            socket
                .send(Message::Text(
                    json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
                        .to_string()
                        .into(),
                ))
                .await
                .expect("initialize reply");
            assert_eq!(next_json(&mut socket).await["method"], "initialized");
            assert_eq!(
                next_json(&mut socket).await["method"],
                "account/rateLimits/read"
            );
            socket
                .send(Message::Text(
                    json!({
                        "jsonrpc": "2.0",
                        "id": RATE_LIMITS_REQUEST,
                        "result": { "rateLimits": {
                            "primary": { "usedPercent": 25, "windowDurationMins": 300, "resetsAt": 10 },
                            "secondary": { "usedPercent": 61, "windowDurationMins": 10080, "resetsAt": 20 }
                        }}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("rate limits reply");
            socket
                .send(Message::Text(
                    json!({
                        "jsonrpc": "2.0",
                        "method": "account/rateLimits/updated",
                        "params": { "rateLimits": {
                            "primary": { "usedPercent": 95, "windowDurationMins": 300, "resetsAt": 10 },
                            "secondary": { "usedPercent": 61, "windowDurationMins": 10080, "resetsAt": 20 }
                        }}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("rate limits update");
            socket.close(None).await.expect("close");
        });

        let state = AppState::ephemeral();
        let account = state.add_account(Account {
            id: String::new(),
            name: "Codex usage".to_owned(),
            plan: "subscription".to_owned(),
            client: Client::Codex,
            provider: "openai".to_owned(),
            auth: Auth::Subscription,
            sample: false,
            rows: Vec::new(),
            foot: "No official usage source.".to_owned(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            server: None,
            model: None,
            api_style: None,
        });
        let (events, _) = tokio::sync::broadcast::channel(8);
        let entry = Arc::new(SessionEntry::new(
            Session {
                id: "codex-usage".to_owned(),
                name: "Codex usage".to_owned(),
                short: "codex-usage".to_owned(),
                project_id: "web".to_owned(),
                kind: SessionKind::Agent,
                status: SessionState::Active,
                work_state: Some(WorkState::Working),
                client: Some(Client::Codex),
                account_id: Some(account.id.clone()),
                branch: None,
                folder: "/tmp".to_owned(),
                preview: String::new(),
                waiting_for: None,
                pending_permission: None,
                first_prompt: None,
                parent_session_id: None,
                parent_session_name: None,
                env_overrides: Vec::new(),
                preset_id: None,
                model: None,
                config: BTreeMap::new(),
                agent_session_id: None,
                handoff_note: None,
                context_left: None,
                started_at: "2026-01-01T00:00:00Z".to_owned(),
                shortcuts: Vec::new(),
            },
            Arc::new(OutputBudget::new(1024 * 1024)),
            SessionRuntimeConfig::new(events, Duration::from_secs(2)),
        ));
        let (sender, receiver) = mpsc::unbounded_channel();
        let url = format!("ws://{address}");

        tokio::time::timeout(
            Duration::from_secs(5),
            run(url, Arc::clone(&entry), receiver, state.clone()),
        )
        .await
        .expect("the app-server run finished");
        drop(sender);
        server.await.expect("server");

        let stored = state.account(&account.id).expect("account");
        assert_eq!(
            stored.usage_source.as_deref(),
            Some(crate::usage::APP_SERVER)
        );
        assert_eq!(stored.usage_status, Some(UsageStatus::Fresh));
        assert!(stored.usage_read_at.is_some());
        assert!(stored.foot.is_empty());
        assert_eq!(stored.rows.len(), 2);
        assert_eq!(
            stored.rows[0].window.as_deref(),
            Some(crate::usage::FIVE_HOUR)
        );
        assert_eq!(stored.rows[0].used, Some(95));
        assert_eq!(
            stored.rows[1].window.as_deref(),
            Some(crate::usage::SEVEN_DAY)
        );
        assert_eq!(stored.rows[1].used, Some(61));
    }
}
