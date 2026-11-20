use std::net::TcpListener;
use std::time::Duration;

use portable_pty::CommandBuilder;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
pub struct Permission {
    pub id: String,
    pub permission: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

impl Permission {
    pub fn summary(&self) -> String {
        if let Some(command) = self.metadata.get("command").and_then(Value::as_str) {
            return command.to_owned();
        }
        if let Some(path) = self.metadata.get("filePath").and_then(Value::as_str) {
            return path.to_owned();
        }
        if !self.patterns.is_empty() {
            return self.patterns.join(", ");
        }
        self.permission.clone()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub time: SessionTime,
}

impl SessionSummary {
    pub fn updated_at(&self) -> u64 {
        self.time.updated.max(self.time.created)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SessionTime {
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
}

pub fn free_port() -> Option<u16> {
    if let Ok(value) = std::env::var("ELESDEE_OPENCODE_PORT")
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

pub fn launch_args(command: &mut CommandBuilder, port: u16) {
    command.arg("--port");
    command.arg(port.to_string());
    command.arg("--hostname");
    command.arg("127.0.0.1");
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default()
}

pub async fn permissions(base_url: &str) -> Result<Vec<Permission>, String> {
    let response = client()
        .get(format!("{base_url}/permission"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    response.json().await.map_err(|error| error.to_string())
}

pub async fn sessions(base_url: &str) -> Result<Vec<SessionSummary>, String> {
    let response = client()
        .get(format!("{base_url}/session"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    response.json().await.map_err(|error| error.to_string())
}

/// Reads the server's own message records for one session.
///
/// This is the local server API, not the client's on-disk files. The records
/// hold the role, the text parts and the tool parts that the message view
/// shows.
pub async fn messages(base_url: &str, session_id: &str) -> Result<Value, String> {
    if !safe_segment(session_id) {
        return Err("invalid session id".to_owned());
    }
    let response = client()
        .get(format!("{base_url}/session/{session_id}/message"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    response.json().await.map_err(|error| error.to_string())
}

/// Keeps a session id inside one URL path segment.
fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

pub async fn reply(base_url: &str, request_id: &str, allow: bool) -> Result<(), String> {
    let response = client()
        .post(format!("{base_url}/permission/{request_id}/reply"))
        .json(&serde_json::json!({
            "reply": if allow { "once" } else { "reject" }
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("status {}", response.status()))
    }
}

/// The work state that the newest message record implies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    Working,
    WaitingForInput,
}

/// Reads the newest message record to tell a running turn from a finished one.
///
/// An assistant record with a completion time means the turn ended and the
/// agent now waits for the user's next prompt. A user record or a streaming
/// assistant record means the turn is still running.
pub fn activity(messages: &Value) -> Option<Activity> {
    let last = messages.as_array()?.last()?;
    let info = last.get("info").unwrap_or(last);
    match info.get("role").and_then(Value::as_str)? {
        "user" => Some(Activity::Working),
        "assistant" => {
            let finished = info
                .get("time")
                .and_then(|time| time.get("completed"))
                .is_some_and(|value| !value.is_null());
            let failed = info.get("error").is_some_and(|error| !error.is_null());
            if finished || failed {
                Some(Activity::WaitingForInput)
            } else {
                Some(Activity::Working)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_summary_prefers_a_command_then_a_path() {
        let command = Permission {
            id: "per_1".to_owned(),
            permission: "bash".to_owned(),
            patterns: vec!["*".to_owned()],
            metadata: serde_json::json!({ "command": "rm -rf build" }),
        };
        assert_eq!(command.summary(), "rm -rf build");

        let path = Permission {
            metadata: serde_json::json!({ "filePath": "/tmp/file.txt" }),
            ..command.clone()
        };
        assert_eq!(path.summary(), "/tmp/file.txt");

        let patterns = Permission {
            metadata: serde_json::json!({}),
            ..command
        };
        assert_eq!(patterns.summary(), "*");
    }

    #[test]
    fn session_records_tell_a_running_turn_from_a_finished_one() {
        let user = serde_json::json!([
            { "info": { "id": "msg-1", "role": "user" }, "parts": [] }
        ]);
        assert_eq!(activity(&user), Some(Activity::Working));

        let streaming = serde_json::json!([
            { "info": { "id": "msg-1", "role": "user" }, "parts": [] },
            {
                "info": { "id": "msg-2", "role": "assistant", "time": { "created": 2 } },
                "parts": []
            }
        ]);
        assert_eq!(activity(&streaming), Some(Activity::Working));

        let finished = serde_json::json!([
            { "info": { "id": "msg-1", "role": "user" }, "parts": [] },
            {
                "info": {
                    "id": "msg-2",
                    "role": "assistant",
                    "time": { "created": 2, "completed": 3 }
                },
                "parts": []
            }
        ]);
        assert_eq!(activity(&finished), Some(Activity::WaitingForInput));

        let failed = serde_json::json!([
            {
                "info": {
                    "id": "msg-2",
                    "role": "assistant",
                    "error": { "message": "stopped" }
                },
                "parts": []
            }
        ]);
        assert_eq!(activity(&failed), Some(Activity::WaitingForInput));

        assert_eq!(activity(&serde_json::json!([])), None);
        assert_eq!(activity(&serde_json::json!(null)), None);
    }

    #[test]
    fn a_session_id_stays_in_one_path_segment() {
        assert!(safe_segment("ses_abc-123"));
        assert!(!safe_segment("../secret"));
        assert!(!safe_segment("ses/other"));
        assert!(!safe_segment(""));
    }

    #[test]
    fn session_summaries_parse_the_server_shape() {
        let sessions: Vec<SessionSummary> = serde_json::from_value(serde_json::json!([
            {
                "id": "ses_1",
                "directory": "/tmp/work",
                "time": { "created": 10, "updated": 20 }
            },
            { "id": "ses_2" }
        ]))
        .expect("parse sessions");

        assert_eq!(sessions[0].updated_at(), 20);
        assert_eq!(sessions[0].directory.as_deref(), Some("/tmp/work"));
        assert_eq!(sessions[1].updated_at(), 0);
        assert!(sessions[1].directory.is_none());
    }
}
