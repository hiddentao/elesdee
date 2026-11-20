use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::{AppState, QuestionStatus, Session, SessionState, WaitingFor, WorkState};

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
            "name": "write_handoff_note",
            "description": "Save a handoff note for this session. Write it before a switch to a different client, a usage stop, or a close. The app keeps the note and gives it to the next agent that works on the session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note": { "type": "string", "description": "The handoff note, with the standard headings." }
                },
                "required": ["note"]
            }
        }),
        json!({
            "name": "report_limit",
            "description": "Tell the app that this session's provider hit a usage limit. The app switches the session to an allowed config with room or pauses it now, without waiting for the next usage poll.",
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
            "description": "Show the usage rows for a provider. With no provider id, use this session's provider.",
            "inputSchema": {
                "type": "object",
                "properties": { "providerId": { "type": "string" } }
            }
        }),
        json!({
            "name": "start_session",
            "description": "Ask the user to start another session. The user must approve the request first. With the user's permission, later requests from this session can start without asking.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projectId": { "type": "string" },
                    "configId": { "type": "string" },
                    "name": { "type": "string" },
                    "firstPrompt": { "type": "string" },
                    "sourceBranch": { "type": "string" }
                },
                "required": ["projectId"]
            }
        }),
        json!({
            "name": "wait_for_session",
            "description": "Wait until another session needs attention, reaches a chosen state, or changes state. The call returns at the timeout with the session's status; call it again to keep waiting. It also stops when this session closes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "The other session." },
                    "until": {
                        "type": "string",
                        "enum": ["attention", "waiting", "done", "closed", "change"],
                        "description": "attention (default): needs input, a permission or a question, pauses, finishes or closes. waiting: needs input, a permission or a question. change: any change of status or work state."
                    },
                    "timeoutSeconds": { "type": "integer", "minimum": 1, "maximum": 25 }
                },
                "required": ["sessionId"]
            }
        }),
        json!({
            "name": "share_file",
            "description": "Share a file or screenshot from this session's folder with the user. The file is copied into the app's private shared store and appears in the session's Files panel on every device. Use a path relative to the session folder. Secret files, folders and paths outside the folder are refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "A file path relative to the session folder." },
                    "caption": { "type": "string", "description": "An optional short label for the file." }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "browser_open",
            "description": "Open a page in this session's own browser and return its visible text. Each session has a separate browser with its own tabs, cookies and logins.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "A full http or https address." }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "browser_read",
            "description": "Read the visible text of the page that is open in this session's browser.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "browser_click",
            "description": "Click an element on the open page by CSS selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": { "type": "string", "description": "A CSS selector for the element." }
                },
                "required": ["selector"]
            }
        }),
        json!({
            "name": "browser_type",
            "description": "Focus an element on the open page and type text into it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": { "type": "string", "description": "A CSS selector for the element." },
                    "text": { "type": "string", "description": "The text to type." }
                },
                "required": ["selector", "text"]
            }
        }),
        json!({
            "name": "browser_wait",
            "description": "Wait for a selector or text to appear on the open page, or for a short time. The call returns when the condition holds or the timeout ends.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": { "type": "string", "description": "Wait until this CSS selector exists." },
                    "text": { "type": "string", "description": "Wait until the visible text contains this text." },
                    "timeoutSeconds": { "type": "integer", "minimum": 1, "maximum": 15 }
                }
            }
        }),
        json!({
            "name": "browser_screenshot",
            "description": "Take a screenshot of the open page and share it with the user. The image appears in the session's view on every device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "caption": { "type": "string", "description": "An optional short label for the screenshot." }
                }
            }
        }),
        json!({
            "name": "browser_console",
            "description": "Read the recent console messages of this session's own browser. Messages are short, secret values are removed, and older messages drop off. Use this to find page errors.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                }
            }
        }),
        json!({
            "name": "browser_network",
            "description": "Read the recent network requests of this session's own browser: method, address, kind, status or failure. Login details and sensitive address values are removed, and older requests drop off.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                }
            }
        }),
        json!({
            "name": "register_demo_server",
            "description": "Tell the app that a demo server runs on a loopback port. The app records it for this session and returns the local address and, when Tailscale is running, a tailnet address to print. The user can open the local address on this computer, a paired phone can open the same port, and a device on the tailnet can open the tailnet address. Register again after a stop, pause or close.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "A short name, such as Preview." },
                    "port": { "type": "integer", "minimum": 1024, "maximum": 65535 }
                },
                "required": ["name", "port"]
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
        "write_handoff_note" => write_handoff_note(state, session_id, arguments),
        "report_limit" => report_limit(state, session_id).await,
        "list_sessions" => list_sessions(state),
        "read_session" => read_session(state, arguments),
        "send_message" => send_message(state, arguments).await,
        "check_usage" => check_usage(state, session_id, arguments),
        "start_session" => start_session(state, session_id, arguments).await,
        "wait_for_session" => wait_for_session(state, session_id, arguments).await,
        "share_file" => share_file(state, session_id, arguments).await,
        "browser_open" => browser_open(state, session_id, arguments).await,
        "browser_read" => browser_read(state, session_id).await,
        "browser_click" => browser_click(state, session_id, arguments).await,
        "browser_type" => browser_type(state, session_id, arguments).await,
        "browser_wait" => browser_wait(state, session_id, arguments).await,
        "browser_screenshot" => browser_screenshot(state, session_id, arguments).await,
        "browser_console" => browser_console(state, session_id, arguments).await,
        "browser_network" => browser_network(state, session_id, arguments).await,
        "register_demo_server" => register_demo_server(state, session_id, arguments),
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
            state.notify_done(session_id);
            state.save();
            ("Reported done.".to_owned(), false)
        }
        None => ("The session is closed.".to_owned(), true),
    }
}

fn write_handoff_note(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let Some(note) = arguments
        .get("note")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|note| !note.is_empty())
    else {
        return ("The handoff note is required.".to_owned(), true);
    };
    if note.chars().count() > 8000 {
        return ("The handoff note is too long.".to_owned(), true);
    }
    let Some(entry) = state.session(session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    match entry.set_handoff_note(note.to_owned()) {
        Some(session) => {
            state.emit(crate::state::SessionEvent::updated(session));
            state.save();
            ("Handoff note saved.".to_owned(), false)
        }
        None => ("The session is closed.".to_owned(), true),
    }
}

async fn report_limit(state: &AppState, session_id: &str) -> (String, bool) {
    let Some(entry) = state.session(session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let session = entry.session();
    if session.status != SessionState::Active {
        return ("The session is closed.".to_owned(), true);
    }
    let Some(provider_id) = session.provider_id else {
        return ("The session has no provider.".to_owned(), true);
    };
    match crate::usage::report_limit(state, &provider_id).await {
        Some(_) => (
            "Reported the limit. The app will switch or pause the session.".to_owned(),
            false,
        ),
        None => ("Unknown provider.".to_owned(), true),
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
    (
        serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_owned()),
        false,
    )
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

async fn start_session(
    state: &AppState,
    parent_session_id: &str,
    arguments: Value,
) -> (String, bool) {
    let Some(parent_entry) = state.session(parent_session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let parent = parent_entry.session();
    let Some(project_id) = arguments.get("projectId").and_then(Value::as_str) else {
        return ("The project id is required.".to_owned(), true);
    };
    let Some(project) = state.project(project_id) else {
        return ("Unknown project.".to_owned(), true);
    };
    let config_id = arguments
        .get("configId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| parent.config_id.clone());
    let Some(config_id) = config_id else {
        return ("The session needs an AI config.".to_owned(), true);
    };
    let Some(config) = state.config(&config_id) else {
        return ("Unknown AI config.".to_owned(), true);
    };
    let name = arguments
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(64).collect::<String>());
    let first_prompt = arguments
        .get("firstPrompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| prompt.chars().take(8000).collect::<String>());
    let source_branch = arguments
        .get("sourceBranch")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_owned);
    let request_id = uuid::Uuid::new_v4().to_string();
    let request = crate::state::SessionRequest {
        id: request_id.clone(),
        parent_session_id: parent.id.clone(),
        parent_session_name: parent.name.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        name,
        folder: project.folder.clone(),
        source_branch,
        first_prompt,
        config_id: config_id.clone(),
        provider_id: config.provider_id.clone(),
        client: config.client,
        status: crate::state::RequestStatus::Pending,
        child_session_id: None,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };

    if parent_entry.auto_approves_children() {
        let body = serde_json::json!({
            "projectId": request.project_id,
            "kind": "agent",
            "configId": request.config_id,
            "name": request.name,
            "folder": request.folder,
            "sourceBranch": request.source_branch,
            "firstPrompt": request.first_prompt,
            "parentSessionId": request.parent_session_id,
            "parentSessionName": request.parent_session_name
        });
        return match crate::api::create_session_via_api(state, body).await {
            Ok(session) => {
                state.finish_session_request(
                    &request.id,
                    crate::state::RequestStatus::Approved,
                    Some(session.id.clone()),
                );
                (format!("Session started: {}", session.id), false)
            }
            Err(error) => (format!("The session did not start: {error}"), true),
        };
    }

    state.add_session_request(request);
    (format!("Session request: {request_id}"), false)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct SessionSnapshot {
    status: SessionState,
    work_state: Option<WorkState>,
    waiting_for: Option<WaitingFor>,
}

impl SessionSnapshot {
    fn of(session: &Session) -> Self {
        Self {
            status: session.status,
            work_state: session.work_state,
            waiting_for: session.waiting_for,
        }
    }

    fn needs_attention(self) -> bool {
        self.status == SessionState::Closed
            || self.waiting_for.is_some()
            || matches!(
                self.work_state,
                Some(WorkState::Waiting | WorkState::Input | WorkState::Paused | WorkState::Done)
            )
    }

    fn is_waiting(self) -> bool {
        self.status == SessionState::Active
            && (self.waiting_for.is_some()
                || matches!(self.work_state, Some(WorkState::Waiting | WorkState::Input)))
    }
}

async fn wait_for_session(state: &AppState, caller_id: &str, arguments: Value) -> (String, bool) {
    let Some(target_id) = arguments
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return ("The session id is required.".to_owned(), true);
    };
    if target_id == caller_id {
        return ("A session cannot wait for itself.".to_owned(), true);
    }
    let until = arguments
        .get("until")
        .and_then(Value::as_str)
        .unwrap_or("attention");
    if !matches!(
        until,
        "attention" | "waiting" | "done" | "closed" | "change"
    ) {
        return (
            "The until value must be attention, waiting, done, closed or change.".to_owned(),
            true,
        );
    }
    let timeout = arguments
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 25);
    let Some(entry) = state.session(target_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let initial = SessionSnapshot::of(&entry.session());
    if wait_condition(until, initial, initial) {
        return (wait_result(&entry.session(), false), false);
    }

    let mut events = state.subscribe_events();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if state
            .session(caller_id)
            .is_none_or(|entry| entry.is_closed())
        {
            return ("The caller session was closed.".to_owned(), true);
        }
        let Some(target) = state.session(target_id) else {
            return ("The session was deleted.".to_owned(), true);
        };
        let current = target.session();
        if wait_condition(until, SessionSnapshot::of(&current), initial) {
            return (wait_result(&current, false), false);
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return (wait_result(&current, true), false);
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) => {
                if event.session.id == caller_id && event.session.status == SessionState::Closed {
                    return ("The caller session was closed.".to_owned(), true);
                }
                if event.session.id == target_id
                    && wait_condition(until, SessionSnapshot::of(&event.session), initial)
                {
                    return (wait_result(&event.session, false), false);
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                return ("The event stream closed.".to_owned(), true);
            }
            Err(_) => {
                let Some(target) = state.session(target_id) else {
                    return ("The session was deleted.".to_owned(), true);
                };
                return (wait_result(&target.session(), true), false);
            }
        }
    }
}

fn wait_condition(until: &str, current: SessionSnapshot, initial: SessionSnapshot) -> bool {
    match until {
        "change" => current != initial,
        "waiting" => current.is_waiting(),
        "done" => current.work_state == Some(WorkState::Done),
        "closed" => current.status == SessionState::Closed,
        _ => current.needs_attention(),
    }
}

fn wait_result(session: &Session, timed_out: bool) -> String {
    serde_json::to_string(&json!({
        "timedOut": timed_out,
        "id": session.id,
        "name": session.name,
        "status": session.status,
        "workState": session.work_state,
        "waitingFor": session.waiting_for,
        "preview": session.preview
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

async fn share_file(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let Some(path) = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return ("The file path is required.".to_owned(), true);
    };
    if path.chars().count() > 4096 {
        return ("The file path is too long.".to_owned(), true);
    }
    let caption = arguments
        .get("caption")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|caption| !caption.is_empty())
        .map(|caption| caption.chars().take(200).collect::<String>());
    let Some(entry) = state.session(session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let session = entry.session();
    let Some(data_dir) = state.data_dir().map(Path::to_path_buf) else {
        return ("The shared file store is not available.".to_owned(), true);
    };
    let secrets = shared_secret_values(state, entry.redactions()).await;
    let root = PathBuf::from(session.folder);
    let session_name = session.name;
    let target_id = session.id;
    let path = path.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        crate::shared::share(
            &root,
            &data_dir,
            &target_id,
            &session_name,
            &path,
            caption,
            &secrets,
        )
    })
    .await;

    match result {
        Ok(Ok(file)) => {
            let body = serde_json::to_string(&file).unwrap_or_else(|_| "{}".to_owned());
            state.emit_shared_file(file);
            (body, false)
        }
        Ok(Err(error)) => (share_error_message(error), true),
        Err(error) => {
            tracing::error!(%error, "the shared file task failed");
            ("The file could not be shared.".to_owned(), true)
        }
    }
}

fn register_demo_server(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let Some(name) = arguments
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return ("The demo server name is required.".to_owned(), true);
    };
    let Some(port) = arguments.get("port").and_then(Value::as_u64) else {
        return ("The port is required.".to_owned(), true);
    };
    let Ok(port) = u16::try_from(port) else {
        return ("The port must be between 1024 and 65535.".to_owned(), true);
    };
    match state.register_demo_server(session_id, name, port) {
        Ok((session, server)) => {
            state.emit(crate::state::SessionEvent::updated(session));
            state.save();
            let mut text = format!("Registered \"{}\" at {}.", server.name, server.address);
            if let Some(tailscale_address) = &server.tailscale_address {
                text.push_str(&format!(" On Tailscale, open {tailscale_address}."));
            }
            text.push_str(
                " Open the local address on this computer, or the same port on a paired phone.",
            );
            (text, false)
        }
        Err(error) => (error.to_string(), true),
    }
}

async fn shared_secret_values(state: &AppState, mut secrets: Vec<String>) -> Vec<String> {
    let store = state.secret_store();
    let provider_ids = state
        .providers()
        .into_iter()
        .map(|provider| provider.id)
        .collect::<Vec<_>>();
    let keys = tokio::task::spawn_blocking(move || {
        provider_ids
            .into_iter()
            .filter_map(|id| store.get_key(&id).ok().flatten())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    secrets.extend(keys);
    secrets.retain(|secret| !secret.is_empty());
    secrets.sort();
    secrets.dedup();
    secrets
}

fn share_error_message(error: crate::shared::ShareError) -> String {
    match error {
        crate::shared::ShareError::InvalidPath | crate::shared::ShareError::OutsideFolder => {
            "The path must be a file inside the session folder.".to_owned()
        }
        crate::shared::ShareError::NotFound => "The file does not exist.".to_owned(),
        crate::shared::ShareError::NotAFile => "The path is a folder, not a file.".to_owned(),
        crate::shared::ShareError::TooLarge(size) => format!(
            "The file is too large. The limit is {} bytes; this file is {size} bytes.",
            crate::shared::MAX_SHARED_BYTES
        ),
        crate::shared::ShareError::Linked => "Hard-linked files are not supported.".to_owned(),
        crate::shared::ShareError::Secret => {
            "The file looks like a key or a secret file. It was not shared.".to_owned()
        }
        crate::shared::ShareError::Unavailable => {
            "The shared file store is not available.".to_owned()
        }
        crate::shared::ShareError::Io(error) => {
            tracing::error!(%error, "sharing a file failed");
            "The file could not be shared.".to_owned()
        }
    }
}

fn browser_refusal(state: &AppState, session_id: &str) -> Option<(String, bool)> {
    match state.session(session_id) {
        None => Some(("Unknown session.".to_owned(), true)),
        Some(entry) if entry.is_closed() => Some(("The session is closed.".to_owned(), true)),
        Some(_) => None,
    }
}

async fn browser_open(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let Some(url) = arguments
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return ("The URL is required.".to_owned(), true);
    };
    match state.browser().open(session_id, url).await {
        Ok(state_value) => {
            let title = state_value
                .active_tab_id
                .as_ref()
                .and_then(|id| state_value.tabs.iter().find(|tab| &tab.id == id))
                .map(|tab| tab.title.clone())
                .unwrap_or_default();
            let text = state
                .browser()
                .read_text(session_id)
                .await
                .unwrap_or_default();
            let text = text.chars().take(4000).collect::<String>();
            (format!("Opened {title} at {url}.\n\n{text}"), false)
        }
        Err(error) => (error.message(), true),
    }
}

async fn browser_read(state: &AppState, session_id: &str) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    match state.browser().read_text(session_id).await {
        Ok(text) if text.is_empty() => ("The page has no visible text.".to_owned(), false),
        Ok(text) => (text, false),
        Err(error) => (error.message(), true),
    }
}

async fn browser_click(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let Some(selector) = arguments
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
    else {
        return ("The selector is required.".to_owned(), true);
    };
    match state.browser().click(session_id, selector).await {
        Ok(state_value) => (
            format!("Clicked {selector}. {}", page_summary(&state_value)),
            false,
        ),
        Err(error) => (error.message(), true),
    }
}

async fn browser_type(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let Some(selector) = arguments
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
    else {
        return ("The selector is required.".to_owned(), true);
    };
    let Some(text) = arguments.get("text").and_then(Value::as_str) else {
        return ("The text is required.".to_owned(), true);
    };
    match state.browser().type_text(session_id, selector, text).await {
        Ok(_) => (format!("Typed into {selector}."), false),
        Err(error) => (error.message(), true),
    }
}

async fn browser_wait(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let selector = arguments
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|selector| !selector.is_empty());
    let text = arguments
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let timeout = arguments
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, crate::browser::MAX_WAIT.as_secs());
    match state
        .browser()
        .wait(
            session_id,
            selector,
            text,
            std::time::Duration::from_secs(timeout),
        )
        .await
    {
        Ok(state_value) => (
            format!("Wait finished. {}", page_summary(&state_value)),
            false,
        ),
        Err(error) => (error.message(), true),
    }
}

async fn browser_screenshot(
    state: &AppState,
    session_id: &str,
    arguments: Value,
) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let Some(entry) = state.session(session_id) else {
        return ("Unknown session.".to_owned(), true);
    };
    let session = entry.session();
    let Some(data_dir) = state.data_dir().map(std::path::Path::to_path_buf) else {
        return ("The shared file store is not available.".to_owned(), true);
    };
    let caption = arguments
        .get("caption")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|caption| !caption.is_empty())
        .map(|caption| caption.chars().take(200).collect::<String>());
    let (bytes, _) = match state.browser().screenshot(session_id).await {
        Ok(result) => result,
        Err(error) => return (error.message(), true),
    };
    let name = format!("browser-{}.png", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    match crate::shared::share_bytes(
        &data_dir,
        &session.id,
        &session.name,
        &name,
        caption,
        &bytes,
    ) {
        Ok(file) => {
            let body = serde_json::to_string(&file).unwrap_or_else(|_| "{}".to_owned());
            state.emit_shared_file(file);
            (body, false)
        }
        Err(error) => (share_error_message(error), true),
    }
}

async fn browser_console(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let limit = log_limit(&arguments);
    browser_log(state, session_id, limit, false).await
}

async fn browser_network(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let limit = log_limit(&arguments);
    browser_log(state, session_id, limit, true).await
}

async fn browser_log(
    state: &AppState,
    session_id: &str,
    limit: usize,
    network: bool,
) -> (String, bool) {
    if let Some(refusal) = browser_refusal(state, session_id) {
        return refusal;
    }
    let result = if network {
        state.browser().network_log(session_id, limit)
    } else {
        state.browser().console_log(session_id, limit)
    };
    match result {
        Ok(text) => {
            let secrets = match state.session(session_id) {
                Some(entry) => shared_secret_values(state, entry.redactions()).await,
                None => Vec::new(),
            };
            (crate::text::redact_secrets(text, &secrets), false)
        }
        Err(error) => (error.message(), true),
    }
}

fn log_limit(arguments: &Value) -> usize {
    arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|limit| limit as usize)
        .unwrap_or(50)
        .clamp(1, crate::browser::MAX_LOG_LIMIT)
}

fn page_summary(state: &crate::browser::BrowserState) -> String {
    match state
        .active_tab_id
        .as_ref()
        .and_then(|id| state.tabs.iter().find(|tab| &tab.id == id))
    {
        Some(tab) if !tab.title.is_empty() => {
            format!("Now at {} ({}).", tab.title, tab.url)
        }
        Some(tab) if !tab.url.is_empty() => format!("Now at {}.", tab.url),
        _ => "No page is open.".to_owned(),
    }
}

fn check_usage(state: &AppState, session_id: &str, arguments: Value) -> (String, bool) {
    let provider_id = arguments
        .get("providerId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            state
                .session(session_id)
                .and_then(|entry| entry.session().provider_id)
        });
    let Some(provider_id) = provider_id else {
        return ("No provider for this session.".to_owned(), true);
    };
    let Some(provider) = state.provider(&provider_id) else {
        return ("Unknown provider.".to_owned(), true);
    };
    (
        serde_json::to_string(&json!({
            "id": provider.id,
            "name": provider.name,
            "vendor": provider.vendor,
            "rows": provider.rows
        }))
        .unwrap_or_else(|_| "{}".to_owned()),
        false,
    )
}

pub fn write_claude_config(data_dir: &Path, session_id: &str, port: u16) -> io::Result<PathBuf> {
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
    model: Option<&str>,
    config: &crate::overrides::ConfigOverrides,
    skills_root: &Path,
) -> io::Result<PathBuf> {
    let directory = data_dir.join("mcp");
    crate::private_fs::create_private_dir_all(&directory)?;
    let path = directory.join(format!("opencode-{session_id}.json"));
    let url = format!("http://127.0.0.1:{port}/internal/mcp/{session_id}");
    let mut body = serde_json::Map::new();
    crate::overrides::apply_json_config(&mut body, config);
    body.insert(
        "mcp".to_owned(),
        json!({
            "elesdee": { "type": "remote", "url": url }
        }),
    );
    body.insert("skills".to_owned(), json!([skills_root.to_string_lossy()]));
    if let Some(model) = model {
        body.insert("model".to_owned(), Value::String(model.to_owned()));
    }
    crate::private_fs::write_private_file(&path, Value::Object(body).to_string().as_bytes())?;
    Ok(path)
}

const SKILLS: &[(&str, &str)] = &[
    ("elesdee", include_str!("../assets/skills/elesdee/SKILL.md")),
    (
        "ask-and-notify",
        include_str!("../assets/skills/ask-and-notify/SKILL.md"),
    ),
    (
        "coordinate-sessions",
        include_str!("../assets/skills/coordinate-sessions/SKILL.md"),
    ),
    ("handoff", include_str!("../assets/skills/handoff/SKILL.md")),
    (
        "demo-servers",
        include_str!("../assets/skills/demo-servers/SKILL.md"),
    ),
    (
        "frontend-design",
        include_str!("../assets/skills/frontend-design/SKILL.md"),
    ),
    (
        "browser-checks",
        include_str!("../assets/skills/browser-checks/SKILL.md"),
    ),
];

#[cfg(test)]
pub fn skill_names() -> Vec<&'static str> {
    SKILLS.iter().map(|(name, _)| *name).collect()
}

pub fn skill_kit_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir.join("skills").join(session_id)
}

fn write_skills(root: &Path) -> io::Result<()> {
    crate::private_fs::create_private_dir_all(root)?;
    for (name, body) in SKILLS {
        let directory = root.join(name);
        crate::private_fs::create_private_dir_all(&directory)?;
        crate::private_fs::write_private_file(&directory.join("SKILL.md"), body.as_bytes())?;
    }
    Ok(())
}

pub fn prepare_skill_kit(data_dir: &Path, session_id: &str) -> io::Result<PathBuf> {
    let root = skill_kit_path(data_dir, session_id);
    let _ = fs::remove_dir_all(&root);
    crate::private_fs::create_private_dir_all(&root)?;
    write_skills(&root.join("skills"))?;
    let manifest_dir = root.join(".claude-plugin");
    crate::private_fs::create_private_dir_all(&manifest_dir)?;
    let manifest = json!({
        "name": "elesdee",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "The elesdee session skills."
    });
    crate::private_fs::write_private_file(
        &manifest_dir.join("plugin.json"),
        manifest.to_string().as_bytes(),
    )?;
    Ok(root)
}

pub fn user_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".codex"))
        })
}

pub fn prepare_codex_home(
    data_dir: &Path,
    session_id: &str,
    user_codex_home: Option<&Path>,
) -> io::Result<PathBuf> {
    let kit = skill_kit_path(data_dir, session_id);
    crate::private_fs::create_private_dir_all(&kit)?;
    let home = kit.join("codex");
    let _ = fs::remove_dir_all(&home);
    crate::private_fs::create_private_dir_all(&home)?;
    write_skills(&home.join("skills"))?;
    if let Some(user_home) = user_codex_home {
        link_user_file(&user_home.join("auth.json"), &home.join("auth.json"));
        link_user_file(&user_home.join("config.toml"), &home.join("config.toml"));
    }
    Ok(home)
}

#[cfg(unix)]
fn link_user_file(source: &Path, target: &Path) {
    if !source.is_file() || target.exists() {
        return;
    }
    if let Err(error) = std::os::unix::fs::symlink(source, target) {
        tracing::warn!(
            source = %source.display(),
            target = %target.display(),
            %error,
            "could not link the Codex user file"
        );
    }
}

#[cfg(not(unix))]
fn link_user_file(_source: &Path, _target: &Path) {}

pub fn sweep(data_dir: &Path) {
    let _ = fs::remove_dir_all(data_dir.join("skills"));
    let _ = fs::remove_dir_all(data_dir.join("plugin"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temporary_directory(label: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("elesdee-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        directory
    }

    fn assert_skill_set(skills_root: &Path) {
        for name in skill_names() {
            let skill = skills_root.join(name).join("SKILL.md");
            assert!(
                skill.is_file(),
                "the {name} skill was not written at {}",
                skill.display()
            );
        }
    }

    #[test]
    fn writes_an_opencode_config_that_merges_with_the_user_config() {
        let directory = temporary_directory("mcp");
        let root = prepare_skill_kit(&directory, "session-1").expect("write skill kit");
        let path = write_opencode_config(
            &directory,
            "session-1",
            4319,
            None,
            &crate::overrides::ConfigOverrides::new(),
            &root.join("skills"),
        )
        .expect("write config");
        let value: Value =
            serde_json::from_slice(&fs::read(&path).expect("read config")).expect("parse config");
        assert_eq!(
            value["mcp"]["elesdee"]["url"],
            "http://127.0.0.1:4319/internal/mcp/session-1"
        );
        assert_eq!(value["mcp"]["elesdee"]["type"], "remote");
        let skills = value["skills"].as_array().expect("skills");
        assert_eq!(skills.len(), 1);
        let skills_root = PathBuf::from(skills[0].as_str().expect("skill path"));
        assert_eq!(skills_root, root.join("skills"));
        assert_skill_set(&skills_root);
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn writes_the_claude_plugin_with_every_skill() {
        let directory = temporary_directory("claude-skill");
        let root = prepare_skill_kit(&directory, "session-1").expect("write skill kit");
        assert!(root.join(".claude-plugin").join("plugin.json").is_file());
        assert_skill_set(&root.join("skills"));
        let skill = root.join("skills").join("elesdee").join("SKILL.md");
        assert!(
            fs::read_to_string(&skill)
                .expect("read skill")
                .contains("ask_me")
        );
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn writes_private_skill_files_and_directories() {
        let directory = temporary_directory("skill-mode");
        let root = prepare_skill_kit(&directory, "session-1").expect("write skill kit");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = fs::metadata(root.join("skills/elesdee/SKILL.md"))
                .expect("skill metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
            let directory_mode = fs::metadata(root.join("skills/elesdee"))
                .expect("skill directory metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(directory_mode, 0o700);
        }

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn prepares_a_private_codex_home_with_every_skill() {
        let directory = temporary_directory("codex-skill");
        let user_home = temporary_directory("codex-user");
        fs::write(user_home.join("auth.json"), b"{}").expect("write auth file");
        fs::write(user_home.join("config.toml"), b"model = \"test\"\n").expect("write config");

        let home = prepare_codex_home(&directory, "session-1", Some(&user_home))
            .expect("prepare codex home");
        assert_skill_set(&home.join("skills"));
        assert!(home.join("auth.json").is_file());
        assert!(home.join("config.toml").is_file());

        #[cfg(unix)]
        assert!(
            fs::symlink_metadata(home.join("auth.json"))
                .expect("auth metadata")
                .file_type()
                .is_symlink(),
            "the user auth file was not linked"
        );

        fs::remove_dir_all(&directory).expect("remove test directory");
        fs::remove_dir_all(&user_home).expect("remove test user directory");
    }

    #[test]
    fn sweeps_old_skill_folders() {
        let directory = temporary_directory("skill-sweep");
        let kit = prepare_skill_kit(&directory, "session-1").expect("write skill kit");
        fs::create_dir_all(directory.join("plugin")).expect("create old plugin folder");
        assert!(kit.exists());

        sweep(&directory);

        assert!(!directory.join("skills").exists());
        assert!(!directory.join("plugin").exists());
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
