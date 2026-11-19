use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use serde_json::json;

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

pub struct ClaudeSettings {
    pub token: String,
    pub path: PathBuf,
}

pub fn write_settings(data_dir: &Path, session_id: &str, port: u16) -> io::Result<ClaudeSettings> {
    let directory = data_dir.join(SETTINGS_DIR);
    private_fs::create_private_dir_all(&directory)?;

    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let url = format!("http://127.0.0.1:{port}/internal/hooks/claude/{session_id}?token={token}");
    let handler = json!({
        "hooks": [{ "type": "http", "url": url }]
    });
    let mut hooks = serde_json::Map::new();
    for event in HOOK_EVENTS {
        hooks.insert(event.to_owned(), json!([handler.clone()]));
    }
    let bytes = serde_json::to_vec_pretty(&json!({ "hooks": hooks })).map_err(io::Error::other)?;

    let path = session_settings_path(data_dir, session_id);
    if let Err(error) = private_fs::write_private_file(&path, &bytes) {
        remove(&path);
        return Err(error);
    }

    Ok(ClaudeSettings { token, path })
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
    fn tokens_match_only_when_equal() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "abc1234"));
        assert!(!token_matches("abc123", ""));
    }
}
