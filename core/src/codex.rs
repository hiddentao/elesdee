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
use crate::state::PendingPermission;

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

pub async fn run(
    url: String,
    entry: Arc<SessionEntry>,
    mut decisions: mpsc::UnboundedReceiver<Decision>,
) {
    let socket = connect(&url).await;
    let Ok(socket) = socket else {
        tracing::warn!(session = %entry.id(), %url, "could not connect to the Codex app server");
        return;
    };
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
    if writer
        .send(Message::Text(initialize.to_string().into()))
        .await
        .is_err()
    {
        return;
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
                let Some(method) = value.get("method").and_then(Value::as_str) else { continue };
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
    use super::*;

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
}
