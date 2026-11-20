use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::state::{AgentCheck, Client};

const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const VERSION_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub fn find_on_path(binary: &str) -> Option<PathBuf> {
    if binary.contains('/') || binary.contains('\\') {
        return canonical_executable(PathBuf::from(binary));
    }

    let path_value = env::var_os("PATH")?;
    find_in_directories(env::split_paths(&path_value), binary)
}

pub fn find_client(client: Client) -> Option<PathBuf> {
    find_on_path(client.binary()).or_else(|| {
        find_in_directories(
            common_paths(client, crate::seed::home_dir()),
            client.binary(),
        )
    })
}

fn find_in_directories(
    directories: impl IntoIterator<Item = PathBuf>,
    binary: &str,
) -> Option<PathBuf> {
    directories
        .into_iter()
        .map(|directory| directory.join(binary))
        .find_map(canonical_executable)
}

fn common_paths(client: Client, home: Option<PathBuf>) -> Vec<PathBuf> {
    if client != Client::Antigravity {
        return Vec::new();
    }

    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join(".antigravity").join("bin").join("agy"));
        paths.push(
            home.join(".gemini")
                .join("antigravity-cli")
                .join("bin")
                .join("agy"),
        );
        paths.push(home.join(".gemini").join("antigravity-cli").join("agy"));
        paths.push(home.join(".local").join("bin").join("agy"));
    }
    paths.push(PathBuf::from("/opt/homebrew/bin/agy"));
    paths.push(PathBuf::from("/usr/local/bin/agy"));
    paths
}

fn canonical_executable(path: PathBuf) -> Option<PathBuf> {
    if !is_executable(&path) {
        return None;
    }
    path.canonicalize().ok()
}

pub fn check_all() -> Vec<AgentCheck> {
    thread::scope(|scope| {
        let handles = Client::ALL.map(|client| scope.spawn(move || check(client)));
        Client::ALL
            .into_iter()
            .zip(handles)
            .map(|(client, handle)| {
                handle.join().unwrap_or(AgentCheck {
                    name: client,
                    installed: false,
                    version: None,
                })
            })
            .collect()
    })
}

pub fn missing_checks() -> Vec<AgentCheck> {
    Client::ALL
        .into_iter()
        .map(|client| AgentCheck {
            name: client,
            installed: false,
            version: None,
        })
        .collect()
}

fn check(client: Client) -> AgentCheck {
    let Some(path) = find_client(client) else {
        return AgentCheck {
            name: client,
            installed: false,
            version: None,
        };
    };

    let spawned = Command::new(&path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let Ok(mut child) = spawned else {
        return AgentCheck {
            name: client,
            installed: false,
            version: None,
        };
    };

    let deadline = Instant::now() + VERSION_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                let version = child
                    .wait_with_output()
                    .ok()
                    .and_then(|output| version_from_output(&output.stdout, &output.stderr));
                return AgentCheck {
                    name: client,
                    installed: true,
                    version,
                };
            }
            Ok(None) => thread::sleep(VERSION_POLL_INTERVAL),
            Err(_) => {
                return AgentCheck {
                    name: client,
                    installed: true,
                    version: None,
                };
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    AgentCheck {
        name: client,
        installed: true,
        version: None,
    }
}

fn version_from_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    for bytes in [stdout, stderr] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
            return Some(line.chars().take(200).collect());
        }
    }
    None
}

pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn clients_have_expected_binaries() {
        assert_eq!(Client::ClaudeCode.binary(), "claude");
        assert_eq!(Client::Codex.binary(), "codex");
        assert_eq!(Client::OpenCode2.binary(), "opencode");
        assert_eq!(Client::Antigravity.binary(), "agy");
        assert!(Client::ALL.contains(&Client::Antigravity));
    }

    #[test]
    fn antigravity_has_common_install_paths() {
        let home = PathBuf::from("/home/me");
        let paths = common_paths(Client::Antigravity, Some(home.clone()));
        assert!(paths.contains(&home.join(".antigravity/bin/agy")));
        assert!(paths.contains(&home.join(".gemini/antigravity-cli/bin/agy")));
        assert!(paths.contains(&home.join(".local/bin/agy")));
        assert!(common_paths(Client::ClaudeCode, Some(home)).is_empty());
    }

    #[test]
    fn finds_an_executable_in_a_known_directory() {
        let directory =
            std::env::temp_dir().join(format!("elesdee-clients-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("agy");
        fs::write(&path, b"#!/bin/sh\n").expect("write test executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make executable");
        }

        let found = find_in_directories([directory.clone()], "agy");
        assert_eq!(
            found,
            Some(path.canonicalize().expect("canonical test executable"))
        );
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
