use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use serde_json::json;

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
    fs::create_dir_all(&directory)?;
    set_directory_permissions(&directory)?;

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

    let path = directory.join(format!("{session_id}.json"));
    let written = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        set_file_mode(&mut options);
        let mut file = options.open(&path)?;
        set_file_permissions(&file)?;
        file.write_all(&bytes)?;
        file.sync_all()
    })();
    if let Err(error) = written {
        remove(&path);
        return Err(error);
    }

    Ok(ClaudeSettings { token, path })
}

pub fn remove(path: &Path) {
    let _ = fs::remove_file(path);
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

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_file_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn set_file_permissions(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_file_permissions(_file: &fs::File) -> io::Result<()> {
    Ok(())
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
}
