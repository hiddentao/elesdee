use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::{AppState, QuestionStatus};

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

pub fn initialize(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "elesdee", "version": env!("CARGO_PKG_VERSION") }
        }
    })
}

pub fn tools_list(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "tools": tools() }
    })
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "ask_me",
            "description": "Ask the user a question in the elesdee app. Use this when you are blocked. The question stays pending until the user answers, you withdraw it, or the session closes. You can continue working after asking.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The question." },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional answers the user can choose."
                    }
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "wait_for_answer",
            "description": "Wait until the user answers a question or you withdraw it. Returns the answer, or says that the question is still pending.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "questionId": { "type": "string" },
                    "timeoutSeconds": { "type": "integer", "minimum": 1, "maximum": 25 }
                },
                "required": ["questionId"]
            }
        }),
        json!({
            "name": "withdraw_question",
            "description": "Withdraw a question that is no longer needed.",
            "inputSchema": {
                "type": "object",
                "properties": { "questionId": { "type": "string" } },
                "required": ["questionId"]
            }
        }),
        json!({
            "name": "notify_me",
            "description": "Send the user a short notification from this session.",
            "inputSchema": {
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"]
            }
        }),
        json!({
            "name": "report_done",
            "description": "Tell the app that this session's task is done. The session stays active until the user closes it.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_sessions",
            "description": "List the sessions on this computer with their project, status and branch.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "read_session",
            "description": "Read another session's status and recent output preview.",
            "inputSchema": {
                "type": "object",
                "properties": { "sessionId": { "type": "string" } },
                "required": ["sessionId"]
            }
        }),
        json!({
            "name": "send_message",
            "description": "Send a line of input to another session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["sessionId", "text"]
            }
        }),
        json!({
            "name": "check_usage",
            "description": "Show the usage rows for an account. With no account id, use this session's account.",
            "inputSchema": {
                "type": "object",
                "properties": { "accountId": { "type": "string" } }
            }
        }),
    ]
}

pub async fn call_tool(
    state: &AppState,
    session_id: &str,
    name: &str,
    arguments: Value,
) -> (String, bool) {
    match name {
        "ask_me" => ask_me(state, session_id, arguments).await,
        "wait_for_answer" => wait_for_answer(state, arguments).await,
        "withdraw_question" => withdraw_question(state, arguments),
        "notify_me" => notify_me(state, session_id, arguments),
        "report_done" => report_done(state, session_id),
        "list_sessions" => list_sessions(state),
        "read_session" => read_session(state, arguments),
        "send_message" => send_message(state, arguments).await,
        "check_usage" => check_usage(state, session_id, arguments),
        other => (format!("Unknown tool: {other}"), true),
    }
}

async fn ask_me(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let Some(text) = arguments
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return ("The question text is required.".to_owned(), true);
    };
    if text.chars().count() > 2000 {
        return ("The question is too long.".to_owned(), true);
    }
    let options = arguments
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|option| !option.is_empty())
                .take(8)
                .map(|option| option.chars().take(120).collect::<String>())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    match state.add_question(session_id, text.to_owned(), options) {
        Ok(question) => (format!("Question asked: {}", question.id), false),
        Err(_) => ("The session is closed.".to_owned(), true),
    }
}

async fn wait_for_answer(state: &AppState, arguments: Value) -> (String, bool) {
    let Some(id) = arguments.get("questionId").and_then(Value::as_str) else {
        return ("The question id is required.".to_owned(), true);
    };
    let timeout = arguments
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 25);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let mut receiver = state.subscribe_aux();

    loop {
        match state.question(id) {
            Some(question) => match question.status {
                QuestionStatus::Answered => {
                    return (
                        format!(
                            "Answer: {}",
                            question.answer.unwrap_or_else(|| "(no answer)".to_owned())
                        ),
                        false,
                    );
                }
                QuestionStatus::Withdrawn => {
                    return ("The question was withdrawn.".to_owned(), false);
                }
                QuestionStatus::Pending => {}
            },
            None => return ("Unknown question.".to_owned(), true),
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return ("Still pending.".to_owned(), false);
        }
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => return ("Still pending.".to_owned(), false),
            Err(_) => return ("Still pending.".to_owned(), false),
        }
    }
}

fn withdraw_question(state: &AppState, arguments: Value) -> (String, bool) {
    let Some(id) = arguments.get("questionId").and_then(Value::as_str) else {
        return ("The question id is required.".to_owned(), true);
    };
    match state.withdraw_question(id) {
        Ok(_) => ("Question withdrawn.".to_owned(), false),
        Err(_) => ("The question is no longer pending.".to_owned(), true),
    }
}

fn notify_me(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let Some(message) = arguments
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
    else {
        return ("The message is required.".to_owned(), true);
    };
    if message.chars().count() > 300 {
        return ("The message is too long.".to_owned(), true);
    }
    match state.notify(session_id, message.to_owned()) {
        Some(_) => ("Notification sent.".to_owned(), false),
        None => ("Unknown session.".to_owned(), true),
    }
}

fn report_done(state: &AppState, session_id: &str) -> (String, bool) {
    let Some(entry) = state.session(session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    match entry.report_done() {
        Some(session) => {
            state.emit(crate::state::SessionEvent::updated(session));
            state.save();
            ("Reported done.".to_owned(), false)
        }
        None => ("The session is closed.".to_owned(), true),
    }
}

fn list_sessions(state: &AppState) -> (String, bool) {
    let sessions = state
        .sessions()
        .iter()
        .map(|entry| entry.session())
        .map(|session| {
            json!({
                "id": session.id,
                "name": session.name,
                "projectId": session.project_id,
                "kind": session.kind,
                "status": session.status,
                "workState": session.work_state,
                "branch": session.branch
            })
        })
        .collect::<Vec<_>>();
    (serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_owned()), false)
}

fn read_session(state: &AppState, arguments: Value) -> (String, bool) {
    let Some(id) = arguments.get("sessionId").and_then(Value::as_str) else {
        return ("The session id is required.".to_owned(), true);
    };
    let Some(entry) = state.session(id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let session = entry.session();
    (
        serde_json::to_string(&json!({
            "id": session.id,
            "name": session.name,
            "status": session.status,
            "workState": session.work_state,
            "waitingFor": session.waiting_for,
            "preview": session.preview
        }))
        .unwrap_or_else(|_| "{}".to_owned()),
        false,
    )
}

async fn send_message(state: &AppState, arguments: Value) -> (String, bool) {
    let Some(id) = arguments.get("sessionId").and_then(Value::as_str) else {
        return ("The session id is required.".to_owned(), true);
    };
    let Some(text) = arguments.get("text").and_then(Value::as_str) else {
        return ("The text is required.".to_owned(), true);
    };
    if text.chars().count() > 8000 {
        return ("The message is too long.".to_owned(), true);
    }
    let Some(entry) = state.session(id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\n');
    match entry.write_input(bytes).await {
        Ok(()) => ("Message sent.".to_owned(), false),
        Err(_) => ("The session is not running.".to_owned(), true),
    }
}

fn check_usage(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let account_id = arguments
        .get("accountId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            state
                .session(session_id)
                .and_then(|entry| entry.session().account_id)
        });
    let Some(account_id) = account_id else {
        return ("No account for this session.".to_owned(), true);
    };
    let Some(account) = state.account(&account_id) else {
        return ("Unknown account.".to_owned(), true);
    };
    (
        serde_json::to_string(&json!({
            "id": account.id,
            "name": account.name,
            "client": account.client,
            "rows": account.rows
        }))
        .unwrap_or_else(|_| "{}".to_owned()),
        false,
    )
}

pub fn write_claude_config(
    data_dir: &Path,
    session_id: &str,
    port: u16,
) -> io::Result<PathBuf> {
    let directory = data_dir.join("mcp");
    crate::private_fs::create_private_dir_all(&directory)?;
    let path = directory.join(format!("{session_id}.json"));
    let url = format!("http://127.0.0.1:{port}/internal/mcp/{session_id}");
    let body = json!({
        "mcpServers": {
            "elesdee": { "type": "http", "url": url }
        }
    });
    crate::private_fs::write_private_file(&path, body.to_string().as_bytes())?;
    Ok(path)
}

pub fn write_opencode_config(
    data_dir: &Path,
    session_id: &str,
    port: u16,
) -> io::Result<PathBuf> {
    let directory = data_dir.join("mcp");
    crate::private_fs::create_private_dir_all(&directory)?;
    let path = directory.join(format!("opencode-{session_id}.json"));
    let url = format!("http://127.0.0.1:{port}/internal/mcp/{session_id}");
    let body = json!({
        "mcp": {
            "elesdee": { "type": "remote", "url": url }
        }
    });
    crate::private_fs::write_private_file(&path, body.to_string().as_bytes())?;
    Ok(path)
}

const SKILL: &str = r#"---
name: elesdee
description: Work with the elesdee app. Ask the user questions, notify them, report done and coordinate other sessions.
---

# elesdee session tools

You run inside elesdee, a desktop app for coding agents. The app gives you tools that talk to the user.

- When you are blocked, call `ask_me` with a clear question. You can continue working after asking. Call `wait_for_answer` when you need the answer. Call `withdraw_question` when a question no longer matters.
- Use `notify_me` for a short update the user should see. Do not use it for every step.
- Call `report_done` when the task is complete. The user closes the session.
- Use `list_sessions`, `read_session` and `send_message` to coordinate with other sessions on this computer.
- Use `check_usage` to see the remaining usage for an account.

Every pending question appears in the app until the user answers it, you withdraw it, or the session closes.
"#;

pub fn write_skill_plugin(data_dir: &Path) -> io::Result<PathBuf> {
    let root = data_dir.join("plugin");
    let manifest_dir = root.join(".claude-plugin");
    let skill_dir = root.join("skills").join("elesdee");
    crate::private_fs::create_private_dir_all(&manifest_dir)?;
    crate::private_fs::create_private_dir_all(&skill_dir)?;
    let manifest = json!({
        "name": "elesdee",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Session tools for the elesdee desktop app."
    });
    crate::private_fs::write_private_file(
        &manifest_dir.join("plugin.json"),
        manifest.to_string().as_bytes(),
    )?;
    crate::private_fs::write_private_file(&skill_dir.join("SKILL.md"), SKILL.as_bytes())?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_an_opencode_config_that_merges_with_the_user_config() {
        let directory =
            std::env::temp_dir().join(format!("elesdee-mcp-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = write_opencode_config(&directory, "session-1", 4319).expect("write config");
        let value: Value =
            serde_json::from_slice(&fs::read(&path).expect("read config")).expect("parse config");
        assert_eq!(
            value["mcp"]["elesdee"]["url"],
            "http://127.0.0.1:4319/internal/mcp/session-1"
        );
        assert_eq!(value["mcp"]["elesdee"]["type"], "remote");
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn writes_the_skill_plugin() {
        let directory =
            std::env::temp_dir().join(format!("elesdee-skill-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        let root = write_skill_plugin(&directory).expect("write plugin");
        assert!(root.join(".claude-plugin").join("plugin.json").is_file());
        let skill = root.join("skills").join("elesdee").join("SKILL.md");
        assert!(
            fs::read_to_string(&skill)
                .expect("read skill")
                .contains("ask_me")
        );
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
