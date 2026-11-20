use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use serde_json::{Map, Value, json};

use crate::overrides::{ConfigOverrides, apply_json_config};
use crate::private_fs;

const SETTINGS_DIR: &str = "claude-settings";
const HOOK_EVENTS: [&str; 6] = [
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "UserPromptSubmit",
    "MessageDisplay",
];
const CARD_TOOLS: [&str; 6] = [
    "Bash",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "WebFetch",
];

pub struct ClaudeSettings {
    pub token: String,
    pub path: PathBuf,
}

pub fn write_settings(
    data_dir: &Path,
    session_id: &str,
    port: u16,
    model: Option<&str>,
    config: &ConfigOverrides,
) -> io::Result<ClaudeSettings> {
    let directory = data_dir.join(SETTINGS_DIR);
    private_fs::create_private_dir_all(&directory)?;

    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let url = format!("http://127.0.0.1:{port}/internal/hooks/claude/{session_id}?token={token}");
    let http_hook = json!({ "type": "http", "url": url });
    let matcher = permission_matcher();
    let mut hooks = serde_json::Map::new();
    for event in HOOK_EVENTS {
        let groups = if event == "PreToolUse" || event == "PostToolUse" {
            json!([{ "matcher": matcher, "hooks": [http_hook.clone()] }])
        } else {
            json!([{ "hooks": [http_hook.clone()] }])
        };
        hooks.insert(event.to_owned(), groups);
    }
    let mut settings = Map::new();
    apply_json_config(&mut settings, config);
    settings.insert("hooks".to_owned(), Value::Object(hooks));
    settings.insert(
        "statusLine".to_owned(),
        json!({ "type": "command", "command": status_line_command(port, session_id, &token) }),
    );
    if let Some(model) = model {
        settings.insert("model".to_owned(), Value::String(model.to_owned()));
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(settings)).map_err(io::Error::other)?;

    let path = session_settings_path(data_dir, session_id);
    if let Err(error) = private_fs::write_private_file(&path, &bytes) {
        remove(&path);
        return Err(error);
    }

    Ok(ClaudeSettings { token, path })
}

/// The status line command forwards the JSON that Claude Code writes on
/// standard input to the core service and prints nothing.
pub fn status_line_command(port: u16, session_id: &str, token: &str) -> String {
    format!(
        "curl -s --max-time 2 -X POST -H 'content-type: application/json' --data-binary @- \
         'http://127.0.0.1:{port}/internal/statusline/claude/{session_id}?token={token}' \
         >/dev/null 2>&1 || true"
    )
}

pub fn session_settings_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir
        .join(SETTINGS_DIR)
        .join(format!("{session_id}.json"))
}

pub fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn remove_session_settings(data_dir: &Path, session_id: &str) {
    remove(&session_settings_path(data_dir, session_id));
}

pub fn sweep(data_dir: &Path) {
    let directory = data_dir.join(SETTINGS_DIR);
    let Ok(entries) = fs::read_dir(&directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            remove(&path);
        }
    }
}

pub fn add_settings_argument(command: &mut CommandBuilder, path: &Path) {
    if !command
        .get_argv()
        .iter()
        .any(|argument| argument == "--settings")
    {
        command.arg("--settings");
        command.arg(path);
    }
}

pub fn permission_matcher() -> String {
    CARD_TOOLS.join("|")
}

pub fn tool_needs_card(tool: &str) -> bool {
    CARD_TOOLS.contains(&tool.trim())
}

pub fn token_matches(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in expected.iter().zip(provided) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_argument_is_added_once() {
        let mut command = CommandBuilder::new("claude");
        let path = Path::new("/tmp/settings.json");
        add_settings_argument(&mut command, path);
        add_settings_argument(&mut command, path);
        let arguments = command
            .get_argv()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(arguments, ["claude", "--settings", "/tmp/settings.json"]);
    }

    #[test]
    fn permission_matcher_covers_mutating_tools_only() {
        assert_eq!(
            permission_matcher(),
            "Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch"
        );
        for tool in [
            "Bash",
            "Edit",
            "Write",
            "MultiEdit",
            "NotebookEdit",
            "WebFetch",
        ] {
            assert!(tool_needs_card(tool), "{tool}");
        }
        for tool in [
            "Read",
            "Glob",
            "Grep",
            "AskUserQuestion",
            "ExitPlanMode",
            "TodoWrite",
        ] {
            assert!(!tool_needs_card(tool), "{tool}");
        }
    }

    #[test]
    fn status_line_posts_to_the_core_and_prints_nothing() {
        let command = status_line_command(4319, "session-1", "abc123");
        assert!(
            command.contains(
                "http://127.0.0.1:4319/internal/statusline/claude/session-1?token=abc123"
            )
        );
        assert!(command.contains("--max-time 2"));
        assert!(command.contains("--data-binary @-"));
        assert!(command.contains(">/dev/null 2>&1"));
    }

    #[test]
    fn tokens_match_only_when_equal() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "abc1234"));
        assert!(!token_matches("abc123", ""));
    }
}
