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
    env::split_paths(&path_value)
        .map(|directory| directory.join(binary))
        .find_map(canonical_executable)
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
    let Some(path) = find_on_path(client.binary()) else {
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

fn is_executable(path: &Path) -> bool {
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
