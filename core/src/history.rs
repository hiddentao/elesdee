//! Bounded structured conversation history for the phone message view.
//!
//! Each agent session keeps a short log of prompts, replies and tool rows. The
//! log is built from each client's own event interface: Claude Code hooks, the
//! Codex app-server notifications and the OpenCode 2 server message records.
//! Antigravity has no structured source and stays terminal-only.
//!
//! The log is append-only by sequence. An update to an existing item is a new
//! version with a higher sequence number and the same id, so a reader that
//! reconnects can replay from its last sequence and keep the newest version of
//! each id.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sessions::SnapshotFailure;
use crate::text::{clean_label, redact_secrets};

/// Stored versions per session. A long task can produce many tool rows, so the
/// cap is generous but bounded.
pub const HISTORY_ITEM_LIMIT: usize = 400;
/// Bytes of text and ids retained per session.
pub const HISTORY_BYTE_LIMIT: usize = 128 * 1024;
/// Characters retained for one prompt or reply.
pub const HISTORY_TEXT_LIMIT: usize = 16 * 1024;
/// Characters retained for one tool summary.
pub const HISTORY_SUMMARY_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryKind {
    Prompt,
    Reply,
    Tool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryState {
    Streaming,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub seq: u64,
    pub id: String,
    pub kind: HistoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub text: String,
    pub state: HistoryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<HistoryGap>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGap {
    pub after: u64,
    pub oldest_seq: u64,
    pub newest_seq: u64,
}

pub struct HistorySnapshot {
    pub items: Vec<HistoryItem>,
    pub newest_seq: u64,
}

/// The first event on a history stream. It says whether the session has a
/// structured source, so the phone can show the terminal-only note.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatus {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub structured: bool,
    pub client: Option<String>,
}

impl HistoryStatus {
    pub fn new(structured: bool, client: Option<String>) -> Self {
        Self {
            event_type: "history.state",
            structured,
            client,
        }
    }
}

/// One change to the history. A parser turns a client payload into changes and
/// the session applies them to its bounded log.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryChange {
    Prompt {
        id: String,
        text: String,
    },
    Reply {
        id: String,
        text: String,
        state: HistoryState,
    },
    ReplyDelta {
        id: String,
        delta: String,
        state: HistoryState,
    },
    Tool {
        id: String,
        tool: String,
        text: String,
        state: HistoryState,
    },
    FinalReply {
        id: Option<String>,
        text: Option<String>,
    },
    Finish {
        id: Option<String>,
    },
}

/// Returns true when the client publishes a structured conversation.
pub fn client_supports_history(client: crate::state::Client) -> bool {
    client != crate::state::Client::Antigravity
}

impl HistoryChange {
    /// Replaces a client item id with a stable opaque id.
    ///
    /// A client id can carry a request identifier, so the stored form is a
    /// fixed hash. The same client id always maps to the same stored id, which
    /// keeps version updates together without retaining the client value.
    fn normalized(self) -> Self {
        match self {
            HistoryChange::Prompt { id, text } => HistoryChange::Prompt {
                id: clean_id(&id),
                text,
            },
            HistoryChange::Reply { id, text, state } => HistoryChange::Reply {
                id: clean_id(&id),
                text,
                state,
            },
            HistoryChange::ReplyDelta { id, delta, state } => HistoryChange::ReplyDelta {
                id: clean_id(&id),
                delta,
                state,
            },
            HistoryChange::Tool {
                id,
                tool,
                text,
                state,
            } => HistoryChange::Tool {
                id: clean_id(&id),
                tool,
                text,
                state,
            },
            HistoryChange::FinalReply { id, text } => HistoryChange::FinalReply {
                id: id.map(|id| clean_id(&id)),
                text,
            },
            HistoryChange::Finish { id } => HistoryChange::Finish {
                id: id.map(|id| clean_id(&id)),
            },
        }
    }

    pub(crate) fn redact(self, secrets: &[String]) -> Self {
        match self {
            HistoryChange::Prompt { id, text } => HistoryChange::Prompt {
                id,
                text: redact_secrets(text, secrets),
            },
            HistoryChange::Reply { id, text, state } => HistoryChange::Reply {
                id,
                text: redact_secrets(text, secrets),
                state,
            },
            HistoryChange::ReplyDelta { id, delta, state } => HistoryChange::ReplyDelta {
                id,
                delta: redact_secrets(delta, secrets),
                state,
            },
            HistoryChange::Tool {
                id,
                tool,
                text,
                state,
            } => HistoryChange::Tool {
                id,
                tool,
                text: redact_secrets(text, secrets),
                state,
            },
            HistoryChange::FinalReply { id, text } => HistoryChange::FinalReply {
                id,
                text: text.map(|text| redact_secrets(text, secrets)),
            },
            HistoryChange::Finish { id } => HistoryChange::Finish { id },
        }
    }
}

pub struct HistoryLog {
    items: VecDeque<HistoryItem>,
    bytes: usize,
    next_seq: u64,
}

impl Default for HistoryLog {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryLog {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
        }
    }

    pub fn restored(items: Vec<HistoryItem>, next_seq: u64) -> Self {
        let mut log = Self::new();
        let mut last_seq = 0_u64;
        for mut item in items {
            if item.seq <= last_seq {
                continue;
            }
            last_seq = item.seq;
            item.gap = None;
            item.text = clean_text(&item.text, HISTORY_TEXT_LIMIT);
            log.bytes = log.bytes.saturating_add(item_size(&item));
            log.items.push_back(item);
        }
        log.next_seq = next_seq.max(last_seq.saturating_add(1)).max(1);
        log.enforce();
        log
    }

    pub fn items(&self) -> Vec<HistoryItem> {
        self.items.iter().cloned().collect()
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Applies one change and returns the new version, or `None` when nothing
    /// changed.
    pub fn apply(&mut self, change: HistoryChange) -> Option<HistoryItem> {
        match change.normalized() {
            HistoryChange::Prompt { id, text } => self.push(
                id,
                HistoryKind::Prompt,
                None,
                clean_text(&text, HISTORY_TEXT_LIMIT),
                HistoryState::Done,
            ),
            HistoryChange::Reply { id, text, state } => self.push(
                id,
                HistoryKind::Reply,
                None,
                clean_text(&text, HISTORY_TEXT_LIMIT),
                state,
            ),
            HistoryChange::ReplyDelta { id, delta, state } => {
                let base = self
                    .last_for(&id)
                    .filter(|item| item.kind == HistoryKind::Reply)
                    .map(|item| item.text.clone())
                    .unwrap_or_default();
                let text = clean_text(&format!("{base}{delta}"), HISTORY_TEXT_LIMIT);
                self.push(id, HistoryKind::Reply, None, text, state)
            }
            HistoryChange::Tool {
                id,
                tool,
                text,
                state,
            } => {
                let tool = clean_label(&tool, 40).unwrap_or_else(|| "Tool".to_owned());
                self.push(
                    id,
                    HistoryKind::Tool,
                    Some(tool),
                    clean_label(&text, HISTORY_SUMMARY_LIMIT).unwrap_or_default(),
                    state,
                )
            }
            HistoryChange::FinalReply { id, text } => self.final_reply(id, text),
            HistoryChange::Finish { id } => self.finish(id),
        }
    }

    /// Returns the retained versions after `after`.
    pub fn snapshot(&self, after: u64) -> Result<HistorySnapshot, SnapshotFailure> {
        let newest_seq = self.next_seq.saturating_sub(1);
        if after > newest_seq {
            return Err(SnapshotFailure::AfterAhead { newest: newest_seq });
        }

        let mut items = self
            .items
            .iter()
            .filter(|item| item.seq > after)
            .cloned()
            .collect::<Vec<_>>();

        if items.is_empty() {
            if after < newest_seq {
                return Err(SnapshotFailure::LogUnavailable);
            }
            return Ok(HistorySnapshot { items, newest_seq });
        }

        if let Some(oldest_seq) = self
            .items
            .front()
            .map(|item| item.seq)
            .filter(|oldest| after.saturating_add(1) < *oldest)
            && let Some(first) = items.first_mut()
        {
            first.gap = Some(HistoryGap {
                after,
                oldest_seq,
                newest_seq,
            });
        }

        Ok(HistorySnapshot { items, newest_seq })
    }

    fn last_for(&self, id: &str) -> Option<&HistoryItem> {
        self.items.iter().rev().find(|item| item.id == id)
    }

    fn push(
        &mut self,
        id: String,
        kind: HistoryKind,
        tool: Option<String>,
        text: String,
        state: HistoryState,
    ) -> Option<HistoryItem> {
        if let Some(previous) = self.last_for(&id)
            && previous.kind == kind
            && previous.tool == tool
            && previous.text == text
            && previous.state == state
        {
            return None;
        }
        Some(self.append(HistoryItem {
            seq: self.next_seq,
            id,
            kind,
            tool,
            text,
            state,
            gap: None,
        }))
    }

    fn final_reply(&mut self, id: Option<String>, text: Option<String>) -> Option<HistoryItem> {
        let text = text
            .map(|text| clean_text(&text, HISTORY_TEXT_LIMIT))
            .filter(|text| !text.trim().is_empty());
        let Some(text) = text else {
            return self.finish(id);
        };

        let target = id
            .as_deref()
            .and_then(|id| self.last_for(id))
            .filter(|item| item.kind == HistoryKind::Reply)
            .map(|item| item.id.clone())
            .or_else(|| {
                self.items
                    .iter()
                    .rev()
                    .find(|item| item.kind == HistoryKind::Reply)
                    .filter(|item| {
                        let previous = item.text.trim();
                        previous.is_empty()
                            || text.starts_with(previous)
                            || previous.starts_with(text.trim())
                            || previous.contains(text.trim())
                    })
                    .map(|item| item.id.clone())
            });

        match target {
            Some(id) => self.push(id, HistoryKind::Reply, None, text, HistoryState::Done),
            None => {
                let id = id.unwrap_or_else(|| clean_id(&format!("final-{}", self.next_seq)));
                self.push(id, HistoryKind::Reply, None, text, HistoryState::Done)
            }
        }
    }

    fn finish(&mut self, id: Option<String>) -> Option<HistoryItem> {
        let target = match id {
            Some(id) => self.last_for(&id).map(|item| item.id.clone()),
            None => self
                .items
                .iter()
                .rev()
                .find(|item| item.state == HistoryState::Streaming)
                .map(|item| item.id.clone()),
        }?;
        let previous = self.last_for(&target)?.clone();
        if previous.state == HistoryState::Done {
            return None;
        }
        self.push(
            target,
            previous.kind,
            previous.tool,
            previous.text,
            HistoryState::Done,
        )
    }

    fn append(&mut self, item: HistoryItem) -> HistoryItem {
        self.next_seq = self.next_seq.saturating_add(1);
        self.bytes = self.bytes.saturating_add(item_size(&item));
        self.items.push_back(item.clone());
        self.enforce();
        item
    }

    fn enforce(&mut self) {
        while self.items.len() > HISTORY_ITEM_LIMIT {
            let Some(removed) = self.items.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(item_size(&removed));
        }
        while self.bytes > HISTORY_BYTE_LIMIT {
            let Some(removed) = self.items.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(item_size(&removed));
        }
    }
}

fn item_size(item: &HistoryItem) -> usize {
    item.text.len() + item.id.len() + item.tool.as_ref().map_or(0, String::len)
}

fn clean_id(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("h{hash:016x}")
}

fn clean_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| *character == '\n' || *character == '\t' || !character.is_control())
        .take(limit)
        .collect()
}

/// The Claude Code hook payload. Only the fields that the message view uses
/// are kept; the full payload is never stored.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeHookPayload {
    pub hook_event_name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<Value>,
    #[serde(default)]
    pub tool_response: Option<Value>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub delta: Option<String>,
    #[serde(default, rename = "final")]
    pub is_final: bool,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
    #[serde(default)]
    pub notification_type: Option<String>,
    #[serde(default)]
    pub message: Option<Value>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub text: Option<Value>,
}

/// Turns one Claude Code hook payload into history changes.
pub fn claude_changes(payload: &ClaudeHookPayload) -> Vec<HistoryChange> {
    match payload.hook_event_name.as_str() {
        "UserPromptSubmit" => payload
            .prompt
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                vec![HistoryChange::Prompt {
                    id: payload
                        .prompt_id
                        .clone()
                        .or_else(|| payload.turn_id.clone())
                        .unwrap_or_else(|| "prompt".to_owned()),
                    text: text.to_owned(),
                }]
            })
            .unwrap_or_default(),
        "MessageDisplay" => {
            let id = payload
                .message_id
                .clone()
                .or_else(|| payload.turn_id.clone())
                .unwrap_or_else(|| "reply".to_owned());
            let text = payload
                .delta
                .clone()
                .filter(|text| !text.is_empty())
                .or_else(|| first_payload_string(payload.message.as_ref()))
                .or_else(|| first_payload_string(payload.content.as_ref()))
                .or_else(|| first_payload_string(payload.text.as_ref()))
                .unwrap_or_default();
            let state = if payload.is_final {
                HistoryState::Done
            } else {
                HistoryState::Streaming
            };
            if text.is_empty() {
                if payload.is_final {
                    vec![HistoryChange::Finish { id: Some(id) }]
                } else {
                    Vec::new()
                }
            } else {
                vec![HistoryChange::ReplyDelta {
                    id,
                    delta: text,
                    state,
                }]
            }
        }
        "PreToolUse" => {
            let Some(tool) = payload.tool_name.as_deref() else {
                return Vec::new();
            };
            vec![HistoryChange::Tool {
                id: tool_id(payload),
                tool: tool.to_owned(),
                text: tool_summary(tool, payload.tool_input.as_ref()),
                state: HistoryState::Running,
            }]
        }
        "PostToolUse" => {
            let Some(tool) = payload.tool_name.as_deref() else {
                return Vec::new();
            };
            vec![HistoryChange::Tool {
                id: tool_id(payload),
                tool: tool.to_owned(),
                text: tool_summary(tool, payload.tool_input.as_ref()),
                state: if tool_failed(payload.tool_response.as_ref()) {
                    HistoryState::Failed
                } else {
                    HistoryState::Done
                },
            }]
        }
        "Stop" => vec![HistoryChange::FinalReply {
            id: payload.turn_id.clone(),
            text: payload.last_assistant_message.clone(),
        }],
        _ => Vec::new(),
    }
}

fn tool_id(payload: &ClaudeHookPayload) -> String {
    payload
        .tool_use_id
        .clone()
        .or_else(|| payload.turn_id.clone())
        .unwrap_or_else(|| "tool".to_owned())
}

fn tool_summary(tool: &str, input: Option<&Value>) -> String {
    let Some(input) = input.and_then(Value::as_object) else {
        return String::new();
    };
    let preferred: &[&str] = match tool {
        "Bash" | "BashOutput" => &["command"],
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Read" => {
            &["file_path", "notebook_path", "path"]
        }
        "Glob" | "Grep" => &["pattern", "path"],
        "WebFetch" => &["url"],
        "WebSearch" => &["query"],
        _ => &[],
    };
    let fallback = [
        "command",
        "file_path",
        "path",
        "query",
        "url",
        "description",
        "pattern",
        "name",
    ];
    preferred
        .iter()
        .chain(fallback.iter())
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn tool_failed(response: Option<&Value>) -> bool {
    let Some(response) = response.and_then(Value::as_object) else {
        return false;
    };
    response.get("is_error").and_then(Value::as_bool) == Some(true)
        || response.get("error").is_some_and(|value| !value.is_null())
}

fn first_payload_string(value: Option<&Value>) -> Option<String> {
    fn find(value: &Value, depth: usize) -> Option<String> {
        if depth >= 4 {
            return None;
        }
        match value {
            Value::String(text) if !text.is_empty() => Some(text.clone()),
            Value::Array(items) => items.iter().find_map(|item| find(item, depth + 1)),
            Value::Object(map) => ["text", "content", "message", "delta"]
                .iter()
                .find_map(|key| map.get(*key).and_then(|item| find(item, depth + 1))),
            _ => None,
        }
    }
    value.and_then(|value| find(value, 0))
}

/// Turns one Codex app-server notification into history changes.
///
/// The app-server item notifications are the authoritative state. A completed
/// item carries the final text, so it replaces the streamed version.
pub fn codex_changes(method: &str, params: Option<&Value>) -> Vec<HistoryChange> {
    let Some(params) = params else {
        return Vec::new();
    };
    match method {
        "item/started" | "item/completed" => {
            let Some(item) = params.get("item") else {
                return Vec::new();
            };
            let completed = method == "item/completed";
            codex_item_changes(item, completed)
        }
        "item/agentMessage/delta" => {
            let Some(id) = params.get("itemId").and_then(Value::as_str).or_else(|| {
                params
                    .get("item")
                    .and_then(|item| item.get("id"))
                    .and_then(Value::as_str)
            }) else {
                return Vec::new();
            };
            let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                return Vec::new();
            };
            vec![HistoryChange::ReplyDelta {
                id: id.to_owned(),
                delta: delta.to_owned(),
                state: HistoryState::Streaming,
            }]
        }
        "turn/completed" => vec![HistoryChange::Finish { id: None }],
        _ => Vec::new(),
    }
}

fn codex_item_changes(item: &Value, completed: bool) -> Vec<HistoryChange> {
    let Some(kind) = item.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(id) = item.get("id").and_then(Value::as_str) else {
        return Vec::new();
    };
    match kind {
        "userMessage" => {
            let text = item_text(item);
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![HistoryChange::Prompt {
                    id: id.to_owned(),
                    text,
                }]
            }
        }
        "agentMessage" => vec![HistoryChange::Reply {
            id: id.to_owned(),
            text: item_text(item),
            state: if completed {
                HistoryState::Done
            } else {
                HistoryState::Streaming
            },
        }],
        "commandExecution" => vec![HistoryChange::Tool {
            id: id.to_owned(),
            tool: "Bash".to_owned(),
            text: item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            state: codex_state(item, completed),
        }],
        "fileChange" => vec![HistoryChange::Tool {
            id: id.to_owned(),
            tool: "Edit".to_owned(),
            text: codex_file_summary(item),
            state: codex_state(item, completed),
        }],
        "mcpToolCall" => vec![HistoryChange::Tool {
            id: id.to_owned(),
            tool: item
                .get("tool")
                .and_then(Value::as_str)
                .or_else(|| item.get("server").and_then(Value::as_str))
                .unwrap_or("Tool")
                .to_owned(),
            text: item.get("arguments").map(compact_value).unwrap_or_default(),
            state: codex_state(item, completed),
        }],
        "webSearch" => vec![HistoryChange::Tool {
            id: id.to_owned(),
            tool: "WebSearch".to_owned(),
            text: item
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            state: codex_state(item, completed),
        }],
        _ => Vec::new(),
    }
}

fn codex_state(item: &Value, completed: bool) -> HistoryState {
    if !completed {
        return HistoryState::Running;
    }
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status == "failed" || status == "error" {
        HistoryState::Failed
    } else {
        HistoryState::Done
    }
}

fn codex_file_summary(item: &Value) -> String {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .map(|changes| {
            changes
                .iter()
                .map(|change| {
                    change
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
                .filter(|path| !path.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match changes.len() {
        0 => String::new(),
        1 => changes[0].to_owned(),
        count => format!("{} files", count),
    }
}

fn item_text(item: &Value) -> String {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    item.get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Turns the OpenCode 2 server message records into history changes.
///
/// A message holds an `info` record with the role and a `parts` list. Text
/// parts become prompts or replies; tool parts become one-line rows.
pub fn opencode_changes(messages: &Value) -> Vec<HistoryChange> {
    let Some(messages) = messages.as_array() else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for message in messages {
        let info = message.get("info").unwrap_or(message);
        let Some(id) = info.get("id").and_then(Value::as_str) else {
            continue;
        };
        let role = info.get("role").and_then(Value::as_str).unwrap_or_default();
        let parts = message
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let text = parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        match role {
            "user" if !text.is_empty() => changes.push(HistoryChange::Prompt {
                id: id.to_owned(),
                text,
            }),
            "assistant" => {
                let state = if info.get("error").is_some_and(|error| !error.is_null()) {
                    HistoryState::Failed
                } else if info
                    .get("time")
                    .and_then(|time| time.get("completed"))
                    .is_some_and(|value| !value.is_null())
                {
                    HistoryState::Done
                } else {
                    HistoryState::Streaming
                };
                changes.push(HistoryChange::Reply {
                    id: id.to_owned(),
                    text,
                    state,
                });
            }
            _ => {}
        }

        for (index, part) in parts.iter().enumerate() {
            if part.get("type").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            let part_id = part
                .get("id")
                .and_then(Value::as_str)
                .map(|value| value.to_owned())
                .unwrap_or_else(|| format!("{id}-tool-{index}"));
            let tool = part
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("Tool")
                .to_owned();
            let state = match part
                .get("state")
                .and_then(|state| state.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "completed" => HistoryState::Done,
                "error" | "failed" => HistoryState::Failed,
                _ => HistoryState::Running,
            };
            changes.push(HistoryChange::Tool {
                id: part_id,
                tool,
                text: opencode_tool_summary(part),
                state,
            });
        }
    }
    changes
}

fn opencode_tool_summary(part: &Value) -> String {
    let state = part.get("state").unwrap_or(part);
    state
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| {
            state
                .get("input")
                .and_then(Value::as_object)
                .and_then(|input| {
                    ["command", "description", "filePath", "path", "pattern"]
                        .iter()
                        .find_map(|key| input.get(*key).and_then(Value::as_str))
                })
        })
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude(payload: Value) -> Vec<HistoryChange> {
        let payload: ClaudeHookPayload = serde_json::from_value(payload).expect("claude payload");
        claude_changes(&payload)
    }

    #[test]
    fn claude_prompt_reply_and_tool_payloads_become_items() {
        let prompt = claude(serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "abc",
            "prompt_id": "prompt-1",
            "prompt": "Fix the failing test"
        }));
        assert_eq!(
            prompt,
            vec![HistoryChange::Prompt {
                id: "prompt-1".to_owned(),
                text: "Fix the failing test".to_owned(),
            }]
        );

        let streamed = claude(serde_json::json!({
            "hook_event_name": "MessageDisplay",
            "message_id": "msg-1",
            "turn_id": "turn-1",
            "index": 1,
            "final": false,
            "delta": "Looking at the test...\n"
        }));
        assert_eq!(
            streamed,
            vec![HistoryChange::ReplyDelta {
                id: "msg-1".to_owned(),
                delta: "Looking at the test...\n".to_owned(),
                state: HistoryState::Streaming,
            }]
        );

        let final_line = claude(serde_json::json!({
            "hook_event_name": "MessageDisplay",
            "message_id": "msg-1",
            "turn_id": "turn-1",
            "final": true,
            "delta": "All tests pass."
        }));
        assert_eq!(
            final_line,
            vec![HistoryChange::ReplyDelta {
                id: "msg-1".to_owned(),
                delta: "All tests pass.".to_owned(),
                state: HistoryState::Done,
            }]
        );

        let before = claude(serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": "toolu-1",
            "tool_input": { "command": "cargo test" }
        }));
        assert_eq!(
            before,
            vec![HistoryChange::Tool {
                id: "toolu-1".to_owned(),
                tool: "Bash".to_owned(),
                text: "cargo test".to_owned(),
                state: HistoryState::Running,
            }]
        );

        let after = claude(serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_use_id": "toolu-1",
            "tool_input": { "command": "cargo test" },
            "tool_response": { "is_error": false }
        }));
        assert_eq!(
            after,
            vec![HistoryChange::Tool {
                id: "toolu-1".to_owned(),
                tool: "Bash".to_owned(),
                text: "cargo test".to_owned(),
                state: HistoryState::Done,
            }]
        );

        let stop = claude(serde_json::json!({
            "hook_event_name": "Stop",
            "turn_id": "turn-1",
            "last_assistant_message": "Fixed the test."
        }));
        assert_eq!(
            stop,
            vec![HistoryChange::FinalReply {
                id: Some("turn-1".to_owned()),
                text: Some("Fixed the test.".to_owned()),
            }]
        );
    }

    #[test]
    fn claude_tool_failures_are_marked() {
        let after = claude(serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_use_id": "toolu-2",
            "tool_input": { "command": "false" },
            "tool_response": { "is_error": true }
        }));
        assert_eq!(
            after,
            vec![HistoryChange::Tool {
                id: "toolu-2".to_owned(),
                tool: "Bash".to_owned(),
                text: "false".to_owned(),
                state: HistoryState::Failed,
            }]
        );
    }

    #[test]
    fn codex_notifications_become_items() {
        let prompt = codex_changes(
            "item/started",
            Some(&serde_json::json!({
                "threadId": "thread-1",
                "item": { "type": "userMessage", "id": "item-1", "text": "Add a route" }
            })),
        );
        assert_eq!(
            prompt,
            vec![HistoryChange::Prompt {
                id: "item-1".to_owned(),
                text: "Add a route".to_owned(),
            }]
        );

        let delta = codex_changes(
            "item/agentMessage/delta",
            Some(
                &serde_json::json!({ "threadId": "thread-1", "itemId": "item-2", "delta": "Done" }),
            ),
        );
        assert_eq!(
            delta,
            vec![HistoryChange::ReplyDelta {
                id: "item-2".to_owned(),
                delta: "Done".to_owned(),
                state: HistoryState::Streaming,
            }]
        );

        let completed = codex_changes(
            "item/completed",
            Some(&serde_json::json!({
                "threadId": "thread-1",
                "item": { "type": "agentMessage", "id": "item-2", "text": "Done." }
            })),
        );
        assert_eq!(
            completed,
            vec![HistoryChange::Reply {
                id: "item-2".to_owned(),
                text: "Done.".to_owned(),
                state: HistoryState::Done,
            }]
        );

        let command = codex_changes(
            "item/completed",
            Some(&serde_json::json!({
                "threadId": "thread-1",
                "item": {
                    "type": "commandExecution",
                    "id": "item-3",
                    "command": "npm test",
                    "status": "failed"
                }
            })),
        );
        assert_eq!(
            command,
            vec![HistoryChange::Tool {
                id: "item-3".to_owned(),
                tool: "Bash".to_owned(),
                text: "npm test".to_owned(),
                state: HistoryState::Failed,
            }]
        );

        let finished = codex_changes("turn/completed", Some(&serde_json::json!({})));
        assert_eq!(finished, vec![HistoryChange::Finish { id: None }]);
    }

    #[test]
    fn opencode_message_records_become_items() {
        let messages = serde_json::json!([
            {
                "info": { "id": "msg-1", "role": "user", "time": { "created": 1 } },
                "parts": [{ "id": "part-1", "type": "text", "text": "Run the tests" }]
            },
            {
                "info": {
                    "id": "msg-2",
                    "role": "assistant",
                    "time": { "created": 2, "completed": 3 }
                },
                "parts": [
                    { "id": "part-2", "type": "text", "text": "Running them now." },
                    {
                        "id": "part-3",
                        "type": "tool",
                        "tool": "bash",
                        "state": {
                            "status": "completed",
                            "input": { "command": "cargo test" }
                        }
                    }
                ]
            }
        ]);

        let changes = opencode_changes(&messages);
        assert_eq!(
            changes,
            vec![
                HistoryChange::Prompt {
                    id: "msg-1".to_owned(),
                    text: "Run the tests".to_owned(),
                },
                HistoryChange::Reply {
                    id: "msg-2".to_owned(),
                    text: "Running them now.".to_owned(),
                    state: HistoryState::Done,
                },
                HistoryChange::Tool {
                    id: "part-3".to_owned(),
                    tool: "bash".to_owned(),
                    text: "cargo test".to_owned(),
                    state: HistoryState::Done,
                },
            ]
        );
    }

    #[test]
    fn antigravity_has_no_structured_source() {
        assert!(!client_supports_history(crate::state::Client::Antigravity));
        for client in [
            crate::state::Client::ClaudeCode,
            crate::state::Client::Codex,
            crate::state::Client::OpenCode2,
        ] {
            assert!(client_supports_history(client));
        }
    }

    #[test]
    fn the_log_updates_one_item_with_new_versions() {
        let mut log = HistoryLog::new();
        log.apply(HistoryChange::ReplyDelta {
            id: "reply-1".to_owned(),
            delta: "Hello".to_owned(),
            state: HistoryState::Streaming,
        })
        .expect("first version");
        log.apply(HistoryChange::ReplyDelta {
            id: "reply-1".to_owned(),
            delta: " world".to_owned(),
            state: HistoryState::Streaming,
        })
        .expect("second version");
        assert!(
            log.apply(HistoryChange::ReplyDelta {
                id: "reply-1".to_owned(),
                delta: String::new(),
                state: HistoryState::Streaming,
            })
            .is_none(),
            "an identical version was stored"
        );
        let snapshot = log.snapshot(0).expect("snapshot");
        assert_eq!(snapshot.items.len(), 2);
        assert_eq!(snapshot.items.last().expect("last").text, "Hello world");
    }

    #[test]
    fn the_log_is_bounded_and_reports_a_gap() {
        let mut log = HistoryLog::new();
        for index in 0..(HISTORY_ITEM_LIMIT + 10) {
            log.apply(HistoryChange::Prompt {
                id: format!("prompt-{index}"),
                text: format!("prompt {index}"),
            });
        }
        let snapshot = log.snapshot(0).expect("snapshot");
        assert_eq!(snapshot.items.len(), HISTORY_ITEM_LIMIT);
        assert!(snapshot.items[0].seq > 1, "old items were not dropped");
        assert_eq!(snapshot.items[0].gap.as_ref().map(|gap| gap.after), Some(0));
        assert_eq!(snapshot.newest_seq, (HISTORY_ITEM_LIMIT + 10) as u64);

        let replay = log.snapshot(390).expect("replay");
        assert_eq!(replay.items.first().map(|item| item.seq), Some(391));
        assert!(
            replay
                .items
                .first()
                .and_then(|item| item.gap.as_ref())
                .is_none()
        );
    }

    #[test]
    fn a_position_ahead_or_dropped_is_rejected() {
        let mut log = HistoryLog::new();
        log.apply(HistoryChange::Prompt {
            id: "prompt-1".to_owned(),
            text: "hello".to_owned(),
        });
        assert!(matches!(
            log.snapshot(2),
            Err(SnapshotFailure::AfterAhead { newest: 1 })
        ));
        assert!(log.snapshot(0).is_ok());
    }

    #[test]
    fn redaction_keeps_a_client_key_out_of_history() {
        let change = HistoryChange::Prompt {
            id: "prompt-1".to_owned(),
            text: "use sk-secret-value now".to_owned(),
        }
        .redact(&["sk-secret-value".to_owned()]);
        let HistoryChange::Prompt { text, .. } = change else {
            panic!("wrong change kind");
        };
        assert_eq!(text, "use [redacted] now");
    }

    #[test]
    fn a_final_reply_replaces_the_streamed_text() {
        let mut log = HistoryLog::new();
        log.apply(HistoryChange::ReplyDelta {
            id: "reply-1".to_owned(),
            delta: "All tests".to_owned(),
            state: HistoryState::Streaming,
        });
        log.apply(HistoryChange::FinalReply {
            id: None,
            text: Some("All tests pass.".to_owned()),
        })
        .expect("final reply");
        let snapshot = log.snapshot(0).expect("snapshot");
        let replies = snapshot
            .items
            .iter()
            .filter(|item| item.kind == HistoryKind::Reply)
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies.last().expect("reply").text, "All tests pass.");
        assert_eq!(replies.last().expect("reply").state, HistoryState::Done);
    }

    #[test]
    fn a_finish_marks_a_streaming_reply_done() {
        let mut log = HistoryLog::new();
        log.apply(HistoryChange::ReplyDelta {
            id: "reply-1".to_owned(),
            delta: "Still working".to_owned(),
            state: HistoryState::Streaming,
        });
        log.apply(HistoryChange::Finish { id: None })
            .expect("finish");
        let snapshot = log.snapshot(0).expect("snapshot");
        assert_eq!(
            snapshot.items.last().expect("last").state,
            HistoryState::Done
        );
    }
}
