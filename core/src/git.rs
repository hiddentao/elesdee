use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy)]
pub struct GitTimeouts {
    pub local: Duration,
    pub push: Duration,
}

#[derive(Debug)]
pub enum CloseOutcome {
    Cleaned,
    WorktreeMissing,
    StatusFailed,
    CommitFailed,
    PushFailed,
    RemoveFailed,
    Timeout,
}

#[derive(Debug)]
pub enum EnsureFailure {
    BranchMissing,
    Failed,
}

impl CloseOutcome {
    pub fn preview(&self) -> &'static str {
        match self {
            CloseOutcome::Cleaned => "Closed. The worktree was removed.",
            CloseOutcome::WorktreeMissing => "Closed. The worktree folder was already missing.",
            CloseOutcome::StatusFailed => {
                "Closed. The worktree was kept because Git status failed."
            }
            CloseOutcome::CommitFailed => {
                "Closed. The worktree was kept because the commit failed."
            }
            CloseOutcome::PushFailed => "Closed. The worktree was kept because the push failed.",
            CloseOutcome::RemoveFailed => {
                "Closed. The worktree was kept because it could not be removed."
            }
            CloseOutcome::Timeout => "Closed. The worktree was kept because Git timed out.",
        }
    }
}

enum GitRun {
    Output(Output),
    Timeout,
    Failed,
}

impl GitRun {
    fn success(self) -> bool {
        matches!(self, GitRun::Output(output) if output.status.success())
    }
}

enum RemoteFailure {
    Timeout,
    Failed,
}

pub async fn repo_root(folder: PathBuf, timeout: Duration) -> Option<PathBuf> {
    tokio::task::spawn_blocking(move || {
        let output = match run_git(&folder, ["rev-parse", "--show-toplevel"], timeout) {
            GitRun::Output(output) if output.status.success() => output,
            _ => return None,
        };
        let value = String::from_utf8_lossy(&output.stdout);
        let root = value.trim();
        if root.is_empty() {
            None
        } else {
            fs::canonicalize(root).ok()
        }
    })
    .await
    .ok()
    .flatten()
}

pub async fn branch_exists(repo_root: PathBuf, branch: String, timeout: Duration) -> bool {
    tokio::task::spawn_blocking(move || {
        let reference = format!("refs/heads/{branch}");
        run_git(
            &repo_root,
            ["rev-parse", "--verify", reference.as_str()],
            timeout,
        )
        .success()
    })
    .await
    .unwrap_or(false)
}

pub async fn create_worktree(
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    timeout: Duration,
) -> Result<(), ()> {
    tokio::task::spawn_blocking(move || {
        fs::create_dir_all(&worktrees_root).map_err(|_| ())?;
        let output = run_git(
            &repo_root,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("-b"),
                OsStr::new(&branch),
                worktree.as_os_str(),
                OsStr::new("HEAD"),
            ],
            timeout,
        );
        if output.success() { Ok(()) } else { Err(()) }
    })
    .await
    .unwrap_or(Err(()))
}

pub async fn rollback_worktree(
    repo_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    timeout: Duration,
) {
    let _ = tokio::task::spawn_blocking(move || {
        let _ = run_git(
            &repo_root,
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                worktree.as_os_str(),
            ],
            timeout,
        );
        let _ = run_git(
            &repo_root,
            [OsStr::new("branch"), OsStr::new("-D"), OsStr::new(&branch)],
            timeout,
        );
    })
    .await;
}

pub async fn close_worktree(
    repo_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    name: String,
    timeouts: GitTimeouts,
) -> CloseOutcome {
    tokio::task::spawn_blocking(move || {
        close_blocking(&repo_root, &worktree, &branch, &name, timeouts)
    })
    .await
    .unwrap_or(CloseOutcome::Timeout)
}

pub async fn ensure_worktree(
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    timeout: Duration,
) -> Result<(), EnsureFailure> {
    tokio::task::spawn_blocking(move || {
        ensure_blocking(&repo_root, &worktrees_root, &worktree, &branch, timeout)
    })
    .await
    .unwrap_or(Err(EnsureFailure::Failed))
}

pub async fn remove_worktree(repo_root: PathBuf, worktree: PathBuf, timeout: Duration) -> bool {
    tokio::task::spawn_blocking(move || {
        if !worktree.exists() {
            return true;
        }
        run_git(
            &repo_root,
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                worktree.as_os_str(),
            ],
            timeout,
        )
        .success()
    })
    .await
    .unwrap_or(false)
}

fn close_blocking(
    repo_root: &Path,
    worktree: &Path,
    branch: &str,
    name: &str,
    timeouts: GitTimeouts,
) -> CloseOutcome {
    if !worktree.exists() {
        return CloseOutcome::WorktreeMissing;
    }

    let status = match run_git(worktree, ["status", "--porcelain"], timeouts.local) {
        GitRun::Output(output) if output.status.success() => output,
        GitRun::Timeout => return CloseOutcome::Timeout,
        _ => return CloseOutcome::StatusFailed,
    };

    if !status.stdout.is_empty() {
        match run_git(worktree, ["add", "-A"], timeouts.local) {
            GitRun::Output(output) if output.status.success() => {}
            GitRun::Timeout => return CloseOutcome::Timeout,
            _ => return CloseOutcome::CommitFailed,
        }
        let message = format!("elesdee: close {name}");
        match run_git(worktree, ["commit", "-m", message.as_str()], timeouts.local) {
            GitRun::Output(output) if output.status.success() => {}
            GitRun::Timeout => return CloseOutcome::Timeout,
            _ => return CloseOutcome::CommitFailed,
        }
    }

    let remote = match remote_for_branch(worktree, timeouts.local) {
        Ok(remote) => remote,
        Err(RemoteFailure::Timeout) => return CloseOutcome::Timeout,
        Err(RemoteFailure::Failed) => return CloseOutcome::PushFailed,
    };
    if let Some(remote) = remote {
        match run_git(
            worktree,
            ["push", "-u", remote.as_str(), branch],
            timeouts.push,
        ) {
            GitRun::Output(output) if output.status.success() => {}
            GitRun::Timeout => return CloseOutcome::Timeout,
            _ => return CloseOutcome::PushFailed,
        }
    }

    match run_git(
        repo_root,
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            worktree.as_os_str(),
        ],
        timeouts.local,
    ) {
        GitRun::Output(output) if output.status.success() => CloseOutcome::Cleaned,
        GitRun::Timeout => CloseOutcome::Timeout,
        _ => CloseOutcome::RemoveFailed,
    }
}

fn ensure_blocking(
    repo_root: &Path,
    worktrees_root: &Path,
    worktree: &Path,
    branch: &str,
    timeout: Duration,
) -> Result<(), EnsureFailure> {
    if !repo_root.is_dir() {
        return Err(EnsureFailure::Failed);
    }

    let branch_ref = format!("refs/heads/{branch}");
    match run_git(
        repo_root,
        ["rev-parse", "--verify", branch_ref.as_str()],
        timeout,
    ) {
        GitRun::Output(output) if output.status.success() => {}
        GitRun::Output(_) => return Err(EnsureFailure::BranchMissing),
        GitRun::Timeout | GitRun::Failed => return Err(EnsureFailure::Failed),
    }

    if fs::symlink_metadata(worktree).is_ok() {
        let symlink =
            fs::symlink_metadata(worktree).is_ok_and(|metadata| metadata.file_type().is_symlink());
        if !symlink && registered_worktree(repo_root, worktree, branch, timeout)? {
            return Ok(());
        }
        if !worktree.starts_with(worktrees_root) {
            return Err(EnsureFailure::Failed);
        }
        remove_stale_path(worktree)?;
    }

    let _ = run_git(repo_root, ["worktree", "prune"], timeout);
    fs::create_dir_all(worktrees_root).map_err(|_| EnsureFailure::Failed)?;
    let output = run_git(
        repo_root,
        [
            OsStr::new("worktree"),
            OsStr::new("add"),
            worktree.as_os_str(),
            OsStr::new(branch),
        ],
        timeout,
    );
    if output.success() {
        Ok(())
    } else {
        Err(EnsureFailure::Failed)
    }
}

fn registered_worktree(
    repo_root: &Path,
    worktree: &Path,
    branch: &str,
    timeout: Duration,
) -> Result<bool, EnsureFailure> {
    let output = match run_git(repo_root, ["worktree", "list", "--porcelain"], timeout) {
        GitRun::Output(output) if output.status.success() => output,
        GitRun::Timeout | GitRun::Failed => return Err(EnsureFailure::Failed),
        GitRun::Output(_) => return Err(EnsureFailure::Failed),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let expected_branch = format!("refs/heads/{branch}");
    let expected_canonical = fs::canonicalize(worktree).ok();

    for block in text.split("\n\n") {
        let mut listed_path = None;
        let mut listed_branch = None;
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                listed_path = Some(path);
            } else if let Some(found) = line.strip_prefix("branch ") {
                listed_branch = Some(found);
            }
        }
        let Some(listed_path) = listed_path else {
            continue;
        };
        let path_matches = Path::new(listed_path) == worktree
            || (expected_canonical.is_some()
                && fs::canonicalize(listed_path).ok() == expected_canonical);
        if path_matches && listed_branch == Some(expected_branch.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_stale_path(worktree: &Path) -> Result<(), EnsureFailure> {
    let metadata = fs::symlink_metadata(worktree).map_err(|_| EnsureFailure::Failed)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(worktree).map_err(|_| EnsureFailure::Failed)
    } else {
        fs::remove_dir_all(worktree).map_err(|_| EnsureFailure::Failed)
    }
}

fn remote_for_branch(dir: &Path, timeout: Duration) -> Result<Option<String>, RemoteFailure> {
    match run_git(
        dir,
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        timeout,
    ) {
        GitRun::Output(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout);
            let upstream = value.trim();
            if let Some((remote, _)) = upstream.split_once('/') {
                if !remote.is_empty() {
                    return Ok(Some(remote.to_owned()));
                }
            }
        }
        GitRun::Timeout => return Err(RemoteFailure::Timeout),
        _ => {}
    }

    match run_git(dir, ["remote"], timeout) {
        GitRun::Output(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let remotes = text
                .lines()
                .map(str::trim)
                .filter(|remote| !remote.is_empty())
                .collect::<Vec<_>>();
            if remotes.len() == 1 {
                Ok(Some(remotes[0].to_owned()))
            } else {
                Ok(None)
            }
        }
        GitRun::Timeout => Err(RemoteFailure::Timeout),
        _ => Err(RemoteFailure::Failed),
    }
}

fn run_git<I, S>(dir: &Path, arguments: I, timeout: Duration) -> GitRun
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let spawned = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => return GitRun::Failed,
    };
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return match child.wait_with_output() {
                    Ok(output) => GitRun::Output(output),
                    Err(_) => GitRun::Failed,
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return GitRun::Timeout;
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) => return GitRun::Failed,
        }
    }
}
