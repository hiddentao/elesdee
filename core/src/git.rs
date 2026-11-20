use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

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
    TooLarge,
    Failed,
}

const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

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
            [
                "rev-parse",
                "--verify",
                "--end-of-options",
                reference.as_str(),
            ],
            timeout,
        )
        .success()
    })
    .await
    .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
}

pub async fn list_branches(repo_root: PathBuf, timeout: Duration) -> Vec<BranchInfo> {
    tokio::task::spawn_blocking(move || {
        let current = run_git(&repo_root, ["symbolic-ref", "--short", "HEAD"], timeout);
        let current = match current {
            GitRun::Output(output) if output.status.success() => {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
            }
            _ => None,
        };
        let output = run_git(
            &repo_root,
            [
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/remotes",
            ],
            timeout,
        );
        let GitRun::Output(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }

        let mut branches = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !name.ends_with("/HEAD"))
            .map(|name| BranchInfo {
                name: name.to_owned(),
                current: current.as_deref() == Some(name),
            })
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| {
            right
                .current
                .cmp(&left.current)
                .then_with(|| left.name.cmp(&right.name))
        });
        branches.dedup_by(|left, right| left.name == right.name);
        branches
    })
    .await
    .unwrap_or_default()
}

pub async fn branch_or_remote_exists(
    repo_root: PathBuf,
    branch: String,
    timeout: Duration,
) -> bool {
    tokio::task::spawn_blocking(move || {
        ["refs/heads/", "refs/remotes/"].iter().any(|prefix| {
            let revision = format!("{prefix}{branch}^{{commit}}");
            run_git(
                &repo_root,
                [
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    revision.as_str(),
                ],
                timeout,
            )
            .success()
        })
    })
    .await
    .unwrap_or(false)
}

pub async fn create_worktree(
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    base: String,
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
                OsStr::new(&base),
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
        GitRun::Timeout | GitRun::TooLarge | GitRun::Failed => {
            return Err(EnsureFailure::Failed);
        }
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
        GitRun::Timeout | GitRun::TooLarge | GitRun::Failed => {
            return Err(EnsureFailure::Failed);
        }
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
            if let Some((remote, _)) = upstream.split_once('/')
                && !remote.is_empty()
            {
                return Ok(Some(remote.to_owned()));
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
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => return GitRun::Failed,
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().map(|pipe| {
        let overflow = Arc::clone(&overflow);
        thread::spawn(move || drain_output(pipe, &overflow))
    });
    let stderr = child.stderr.take().map(|pipe| {
        let overflow = Arc::clone(&overflow);
        thread::spawn(move || drain_output(pipe, &overflow))
    });
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout
                    .and_then(|handle| handle.join().ok())
                    .unwrap_or_default();
                let stderr = stderr
                    .and_then(|handle| handle.join().ok())
                    .unwrap_or_default();
                if overflow.load(Ordering::Relaxed) {
                    return GitRun::TooLarge;
                }
                return GitRun::Output(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if overflow.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout.and_then(|handle| handle.join().ok());
                    let _ = stderr.and_then(|handle| handle.join().ok());
                    return GitRun::TooLarge;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout.and_then(|handle| handle.join().ok());
                    let _ = stderr.and_then(|handle| handle.join().ok());
                    return GitRun::Timeout;
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.and_then(|handle| handle.join().ok());
                let _ = stderr.and_then(|handle| handle.join().ok());
                return GitRun::Failed;
            }
        }
    }
}

fn drain_output<R: Read>(mut pipe: R, overflow: &AtomicBool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let room = MAX_GIT_OUTPUT_BYTES.saturating_sub(bytes.len());
        if read > room {
            bytes.extend_from_slice(&buffer[..room]);
            overflow.store(true, Ordering::Relaxed);
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    bytes
}

const MAX_CHANGED_FILES: usize = 500;
const MAX_DIFF_BYTES: usize = 512 * 1024;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub additions: u64,
    pub deletions: u64,
    pub binary: bool,
    pub diff: String,
    pub diff_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub git: bool,
    pub base: Option<String>,
    pub files: Vec<ChangedFile>,
    pub truncated: bool,
    pub error: Option<String>,
}

impl Changes {
    fn not_git() -> Self {
        Self {
            git: false,
            base: None,
            files: Vec::new(),
            truncated: false,
            error: None,
        }
    }
}

/// Collects the differences between a session folder and its branch point.
///
/// Agent sessions compare against the fork point of their branch. Terminal
/// sessions compare the working tree against `HEAD`.
pub async fn changes(
    folder: PathBuf,
    agent_repo_root: Option<PathBuf>,
    timeout: Duration,
) -> Changes {
    tokio::task::spawn_blocking(move || {
        changes_blocking(&folder, agent_repo_root.as_deref(), timeout)
    })
    .await
    .unwrap_or_else(|_| Changes {
        git: true,
        base: None,
        files: Vec::new(),
        truncated: false,
        error: Some("The change check failed.".to_owned()),
    })
}

fn changes_blocking(folder: &Path, agent_repo_root: Option<&Path>, timeout: Duration) -> Changes {
    if !folder.is_dir() {
        return Changes::not_git();
    }

    let deadline = Instant::now() + timeout;
    let budget = || deadline.saturating_duration_since(Instant::now());
    let Some(repo_root) = repo_root_blocking(folder, budget()) else {
        return Changes::not_git();
    };
    let prefix = match run_git(folder, ["rev-parse", "--show-prefix"], budget()) {
        GitRun::Output(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => String::new(),
    };
    let base = choose_base(folder, agent_repo_root, budget())
        .or_else(|| resolve_commit(folder, "HEAD", budget()));
    let base_rev = match base.as_deref() {
        Some(base) => base.to_owned(),
        None => empty_tree(folder, budget()),
    };

    let diff_status = match run_git(
        folder,
        [
            "diff",
            "--name-status",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            base_rev.as_str(),
            "--",
            ".",
        ],
        budget(),
    ) {
        GitRun::Output(output) if output.status.success() => output.stdout,
        GitRun::Output(output) => {
            return Changes {
                git: true,
                base,
                files: Vec::new(),
                truncated: false,
                error: Some(git_error("diff", &output)),
            };
        }
        GitRun::TooLarge => {
            return Changes {
                git: true,
                base,
                files: Vec::new(),
                truncated: true,
                error: Some("The change list is too large.".to_owned()),
            };
        }
        GitRun::Timeout | GitRun::Failed => {
            return Changes {
                git: true,
                base,
                files: Vec::new(),
                truncated: false,
                error: Some("Git did not answer in time.".to_owned()),
            };
        }
    };

    let mut skipped = false;
    let mut entries = parse_name_status(&diff_status, &mut skipped);
    if let GitRun::Output(output) = run_git(
        folder,
        [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
        budget(),
    ) && output.status.success()
    {
        for entry in parse_status(&output.stdout, &mut skipped) {
            if entry.status != ChangeStatus::Untracked
                || entries
                    .iter()
                    .any(|known| known.repo_path == entry.repo_path)
            {
                continue;
            }
            entries.push(entry);
        }
    }

    let stats = match run_git(
        folder,
        [
            "diff",
            "--numstat",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            base_rev.as_str(),
            "--",
            ".",
        ],
        budget(),
    ) {
        GitRun::Output(output) if output.status.success() => {
            parse_numstat(&output.stdout, &mut skipped)
        }
        _ => HashMap::new(),
    };

    let mut files = entries
        .into_iter()
        .filter_map(|entry| {
            let display = strip_prefix(&entry.repo_path, &prefix)?;
            let old_path = entry
                .old_path
                .as_deref()
                .and_then(|old| strip_prefix(old, &prefix));
            let stat = stats.get(&entry.repo_path).copied().unwrap_or_default();
            Some(ChangedFile {
                path: display,
                old_path,
                status: entry.status,
                additions: stat.additions,
                deletions: stat.deletions,
                binary: stat.binary,
                diff: String::new(),
                diff_truncated: false,
            })
        })
        .collect::<Vec<_>>();

    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut truncated = skipped;
    if files.len() > MAX_CHANGED_FILES {
        files.truncate(MAX_CHANGED_FILES);
        truncated = true;
    }

    let mut diff_budget = MAX_DIFF_BYTES;
    for file in &mut files {
        if budget().is_zero() {
            truncated = true;
            break;
        }
        let repo_path = if prefix.is_empty() {
            file.path.clone()
        } else {
            format!("{prefix}{}", file.path)
        };
        if file.status == ChangeStatus::Untracked {
            let untracked = untracked_file(&repo_root, &repo_path, &file.path);
            file.additions = untracked.additions;
            file.deletions = 0;
            file.binary = untracked.binary;
            file.diff = untracked.diff;
            file.diff_truncated = untracked.truncated;
            truncated |= untracked.truncated;
        } else if !file.binary {
            let diff = match run_git(
                &repo_root,
                [
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    base_rev.as_str(),
                    "--",
                    repo_path.as_str(),
                ],
                budget(),
            ) {
                GitRun::Output(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).into_owned()
                }
                GitRun::Output(output) => {
                    return Changes {
                        git: true,
                        base,
                        files: Vec::new(),
                        truncated: false,
                        error: Some(git_error("diff", &output)),
                    };
                }
                GitRun::TooLarge => {
                    file.diff_truncated = true;
                    truncated = true;
                    continue;
                }
                GitRun::Timeout | GitRun::Failed => {
                    return Changes {
                        git: true,
                        base,
                        files: Vec::new(),
                        truncated: false,
                        error: Some("Git did not answer in time.".to_owned()),
                    };
                }
            };
            file.diff = diff;
        }

        let (diff, cut) = take_diff(&file.diff, &mut diff_budget);
        file.diff = diff;
        file.diff_truncated |= cut;
        truncated |= cut;
    }

    Changes {
        git: true,
        base,
        files,
        truncated,
        error: None,
    }
}

fn repo_root_blocking(folder: &Path, timeout: Duration) -> Option<PathBuf> {
    match run_git(folder, ["rev-parse", "--show-toplevel"], timeout) {
        GitRun::Output(output) if output.status.success() => {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if root.is_empty() {
                None
            } else {
                fs::canonicalize(root).ok()
            }
        }
        _ => None,
    }
}

fn choose_base(folder: &Path, agent_repo_root: Option<&Path>, timeout: Duration) -> Option<String> {
    let repo_root = agent_repo_root?;
    let mut candidates = Vec::new();
    if let GitRun::Output(output) = run_git(repo_root, ["symbolic-ref", "--short", "HEAD"], timeout)
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !branch.is_empty() {
            candidates.push(branch);
        }
    }
    if let GitRun::Output(output) = run_git(
        repo_root,
        ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        timeout,
    ) && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !branch.is_empty() {
            candidates.push(branch);
        }
    }
    candidates.push("main".to_owned());
    candidates.push("master".to_owned());
    candidates.dedup();

    for candidate in candidates {
        if !commit_exists(folder, &candidate, timeout) {
            continue;
        }
        if let Some(base) = merge_base(folder, &candidate, timeout) {
            return Some(base);
        }
    }

    match run_git(folder, ["rev-list", "--max-parents=0", "HEAD"], timeout) {
        GitRun::Output(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::trim)
                .filter(|root| !root.is_empty())
                .map(str::to_owned)
        }
        _ => None,
    }
}

fn resolve_commit(dir: &Path, reference: &str, timeout: Duration) -> Option<String> {
    match run_git(
        dir,
        ["rev-parse", "--verify", "--end-of-options", reference],
        timeout,
    ) {
        GitRun::Output(output) if output.status.success() => {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!commit.is_empty()).then_some(commit)
        }
        _ => None,
    }
}

fn commit_exists(dir: &Path, reference: &str, timeout: Duration) -> bool {
    let revision = format!("{reference}^{{commit}}");
    run_git(
        dir,
        [
            "rev-parse",
            "--verify",
            "--end-of-options",
            revision.as_str(),
        ],
        timeout,
    )
    .success()
}

fn empty_tree(folder: &Path, timeout: Duration) -> String {
    match run_git(folder, ["hash-object", "-t", "tree", "--stdin"], timeout) {
        GitRun::Output(output) if output.status.success() => {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if hash.is_empty() {
                EMPTY_TREE.to_owned()
            } else {
                hash
            }
        }
        _ => EMPTY_TREE.to_owned(),
    }
}

fn merge_base(dir: &Path, reference: &str, timeout: Duration) -> Option<String> {
    match run_git(
        dir,
        ["merge-base", "--end-of-options", "HEAD", reference],
        timeout,
    ) {
        GitRun::Output(output) if output.status.success() => {
            let base = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!base.is_empty()).then_some(base)
        }
        _ => None,
    }
}

struct StatusEntry {
    repo_path: String,
    old_path: Option<String>,
    status: ChangeStatus,
}

fn decoded(bytes: &[u8]) -> Option<String> {
    String::from_utf8(bytes.to_vec())
        .ok()
        .filter(|text| !text.is_empty())
}

fn parse_status(bytes: &[u8], skipped: &mut bool) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut tokens = bytes.split(|byte| *byte == 0);

    while let Some(record) = tokens.next() {
        if record.len() < 4 {
            continue;
        }
        let index = record[0];
        let worktree = record[1];
        let status = classify(index, worktree);
        let renamed = status == ChangeStatus::Renamed
            || (status == ChangeStatus::Added && (index == b'C' || worktree == b'C'));
        let old_path = if renamed {
            tokens.next().and_then(decoded)
        } else {
            None
        };
        let Some(path) = decoded(&record[3..]) else {
            *skipped = true;
            continue;
        };
        if renamed && old_path.is_none() {
            *skipped = true;
        }
        entries.push(StatusEntry {
            repo_path: path,
            old_path,
            status,
        });
    }

    entries
}

fn parse_name_status(bytes: &[u8], skipped: &mut bool) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut tokens = bytes.split(|byte| *byte == 0);

    while let Some(status) = tokens.next() {
        let Some(letter) = status.first().copied() else {
            continue;
        };
        let Some(first) = tokens.next() else {
            break;
        };

        let (status, repo_path, old_path) = match letter {
            b'R' | b'C' => {
                let Some(second) = tokens.next() else {
                    break;
                };
                let (Some(old), Some(new)) = (decoded(first), decoded(second)) else {
                    *skipped = true;
                    continue;
                };
                let kind = if letter == b'R' {
                    ChangeStatus::Renamed
                } else {
                    ChangeStatus::Added
                };
                (kind, new, Some(old))
            }
            _ => {
                let Some(path) = decoded(first) else {
                    *skipped = true;
                    continue;
                };
                let status = match letter {
                    b'A' => ChangeStatus::Added,
                    b'D' => ChangeStatus::Deleted,
                    b'U' => ChangeStatus::Conflicted,
                    _ => ChangeStatus::Modified,
                };
                (status, path, None)
            }
        };

        entries.push(StatusEntry {
            repo_path,
            old_path,
            status,
        });
    }

    entries
}

fn classify(index: u8, worktree: u8) -> ChangeStatus {
    if index == b'?' && worktree == b'?' {
        ChangeStatus::Untracked
    } else if index == b'U'
        || worktree == b'U'
        || (index == b'A' && worktree == b'A')
        || (index == b'D' && worktree == b'D')
    {
        ChangeStatus::Conflicted
    } else if index == b'R' || worktree == b'R' {
        ChangeStatus::Renamed
    } else if index == b'C' || worktree == b'C' || index == b'A' {
        ChangeStatus::Added
    } else if index == b'D' || worktree == b'D' {
        ChangeStatus::Deleted
    } else {
        ChangeStatus::Modified
    }
}

#[derive(Clone, Copy, Default)]
struct NumStat {
    additions: u64,
    deletions: u64,
    binary: bool,
}

fn parse_numstat(bytes: &[u8], skipped: &mut bool) -> HashMap<String, NumStat> {
    let mut stats = HashMap::new();
    let mut tokens = bytes.split(|byte| *byte == 0);

    while let Some(record) = tokens.next() {
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let additions = fields.next().unwrap_or_default();
        let deletions = fields.next().unwrap_or_default();
        let path = fields.next().unwrap_or_default();
        let stat = NumStat {
            additions: std::str::from_utf8(additions)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            deletions: std::str::from_utf8(deletions)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            binary: additions == b"-" && deletions == b"-",
        };

        if path.is_empty() {
            let _old = tokens.next();
            let Some(new) = tokens.next() else {
                break;
            };
            match decoded(new) {
                Some(new) => {
                    stats.insert(new, stat);
                }
                None => *skipped = true,
            }
        } else {
            match decoded(path) {
                Some(path) => {
                    stats.insert(path, stat);
                }
                None => *skipped = true,
            }
        }
    }

    stats
}

fn strip_prefix(path: &str, prefix: &str) -> Option<String> {
    if prefix.is_empty() {
        return Some(path.to_owned());
    }
    path.strip_prefix(prefix)
        .filter(|rest| !rest.is_empty())
        .map(str::to_owned)
}

struct UntrackedFile {
    additions: u64,
    binary: bool,
    diff: String,
    truncated: bool,
}

fn untracked_file(repo_root: &Path, repo_path: &str, display_path: &str) -> UntrackedFile {
    let target = repo_root.join(repo_path);
    let Ok(metadata) = fs::symlink_metadata(&target) else {
        return UntrackedFile {
            additions: 0,
            binary: false,
            diff: String::new(),
            truncated: false,
        };
    };
    if metadata.file_type().is_symlink() {
        return UntrackedFile {
            additions: 0,
            binary: false,
            diff: String::new(),
            truncated: true,
        };
    }
    if !metadata.is_file() || metadata.len() > crate::files::MAX_FILE_BYTES {
        return UntrackedFile {
            additions: 0,
            binary: false,
            diff: String::new(),
            truncated: true,
        };
    }

    let Ok(bytes) = fs::read(&target) else {
        return UntrackedFile {
            additions: 0,
            binary: false,
            diff: String::new(),
            truncated: false,
        };
    };
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return UntrackedFile {
            additions: 0,
            binary: true,
            diff: String::new(),
            truncated: false,
        };
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return UntrackedFile {
            additions: 0,
            binary: true,
            diff: String::new(),
            truncated: false,
        };
    };

    let lines = content.lines().collect::<Vec<_>>();
    let mut diff = format!(
        "--- /dev/null\n+++ b/{display_path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in &lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    UntrackedFile {
        additions: lines.len() as u64,
        binary: false,
        diff,
        truncated: false,
    }
}

fn take_diff(diff: &str, budget: &mut usize) -> (String, bool) {
    if *budget == 0 {
        return (String::new(), !diff.is_empty());
    }
    if diff.len() <= *budget {
        *budget -= diff.len();
        return (diff.to_owned(), false);
    }

    let mut end = *budget;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    *budget = 0;
    (format!("{}\n… diff truncated …", &diff[..end]), true)
}

fn git_error(command: &str, output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if !detail.is_empty() {
        tracing::warn!(command, %detail, "Git command failed");
    }
    format!("Git {command} failed.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn output_drain_stops_at_the_cap() {
        let overflow = AtomicBool::new(false);
        let input = vec![b'a'; MAX_GIT_OUTPUT_BYTES + 1];
        let output = drain_output(Cursor::new(input), &overflow);

        assert_eq!(output.len(), MAX_GIT_OUTPUT_BYTES);
        assert!(overflow.load(Ordering::Relaxed));
    }

    #[test]
    fn paths_that_are_not_utf8_are_skipped() {
        let mut skipped = false;
        let entries = parse_name_status(b"M\0ok.txt\0M\0\xff\xfe\0", &mut skipped);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].repo_path, "ok.txt");
        assert!(skipped);
    }
}
