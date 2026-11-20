#[cfg(unix)]
use std::collections::HashSet;
use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tokio::sync::{Mutex as AsyncMutex, broadcast, oneshot, watch};

use crate::claude;
use crate::demo::{DemoFailure, DemoServerState, MAX_DEMO_SERVERS};
use crate::history::{HistoryChange, HistoryItem, HistoryLog, HistorySnapshot};
use crate::state::{
    AuxEvent, Client, PendingPermission, PermissionDecision, Session, SessionEvent, SessionKind,
    SessionState, Shortcut, WaitingFor, WorkState, lock,
};

const OUTPUT_LOG_LIMIT: usize = 256 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 1024;
const MIN_COLS: u16 = 20;
const MAX_COLS: u16 = 500;
const MIN_ROWS: u16 = 5;
const MAX_ROWS: u16 = 300;
const RESTART_PREVIEW: &str = "Paused. The app restarted. Resume to continue.";
const HANDOFF_LOG_TAIL_CHARS: usize = 4000;
const HANDOFF_CONTEXT_MAX_CHARS: usize = 6000;
pub(crate) const GROUP_KILL_GRACE: Duration = Duration::from_millis(200);

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type StoredWriter = Mutex<Option<SharedWriter>>;
type StoredMaster = Mutex<Option<Box<dyn MasterPty + Send>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub const DEFAULT: Self = Self {
        cols: 120,
        rows: 32,
    };

    pub fn new(cols: u16, rows: u16) -> Option<Self> {
        if (MIN_COLS..=MAX_COLS).contains(&cols) && (MIN_ROWS..=MAX_ROWS).contains(&rows) {
            Some(Self { cols, rows })
        } else {
            None
        }
    }

    pub fn range_error() -> String {
        format!(
            "cols must be between {MIN_COLS} and {MAX_COLS} and rows between {MIN_ROWS} and {MAX_ROWS}."
        )
    }

    fn pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

pub fn prompt_arguments(client: Client, prompt: &str) -> Vec<String> {
    match client {
        Client::ClaudeCode | Client::Codex => vec![prompt.to_owned()],
        Client::Antigravity => vec!["--prompt-interactive".to_owned(), prompt.to_owned()],
        Client::OpenCode2 => vec!["--prompt".to_owned(), prompt.to_owned()],
    }
}

pub fn client_needs_prompt_input(_client: Client) -> bool {
    false
}

pub fn resume_arguments(client: Client, session_id: Option<&str>) -> Vec<String> {
    let session_id = session_id.map(str::trim).filter(|id| !id.is_empty());
    match client {
        Client::ClaudeCode => match session_id {
            Some(id) => vec!["--resume".to_owned(), id.to_owned()],
            None => vec!["--continue".to_owned()],
        },
        Client::Antigravity => vec!["--continue".to_owned()],
        Client::Codex => match session_id {
            Some(id) => vec!["resume".to_owned(), id.to_owned()],
            None => vec!["resume".to_owned(), "--last".to_owned()],
        },
        Client::OpenCode2 => match session_id {
            Some(id) => vec!["--session".to_owned(), id.to_owned()],
            None => vec!["--continue".to_owned()],
        },
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct OutputChunk {
    pub seq: u64,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<OutputGap>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputGap {
    pub after: u64,
    pub oldest_seq: u64,
    pub newest_seq: u64,
}

pub struct OutputSnapshot {
    pub chunks: Vec<OutputChunk>,
    pub newest_seq: u64,
}

#[derive(Debug)]
pub enum SnapshotFailure {
    AfterAhead { newest: u64 },
    LogUnavailable,
}

#[derive(Clone)]
pub struct SessionRuntimeConfig {
    events: broadcast::Sender<SessionEvent>,
    aux: broadcast::Sender<AuxEvent>,
    preview_throttle: Duration,
}

impl SessionRuntimeConfig {
    pub fn new(
        events: broadcast::Sender<SessionEvent>,
        aux: broadcast::Sender<AuxEvent>,
        preview_throttle: Duration,
    ) -> Self {
        Self {
            events,
            aux,
            preview_throttle,
        }
    }
}

pub struct SessionEntry {
    data: Mutex<SessionData>,
    writer: StoredWriter,
    master: StoredMaster,
    command: Mutex<Option<CommandBuilder>>,
    size: Mutex<TerminalSize>,
    repo_root: Mutex<Option<String>>,
    write_gate: AsyncMutex<()>,
    lifecycle: AsyncMutex<()>,
    output_tx: broadcast::Sender<OutputChunk>,
    events: broadcast::Sender<SessionEvent>,
    aux: broadcast::Sender<AuxEvent>,
    close_tx: watch::Sender<bool>,
    reader_stopped: AtomicBool,
    output_budget: Arc<OutputBudget>,
    preview_throttle: Duration,
    claude: Mutex<Option<ClaudeRuntime>>,
    home: Mutex<Option<PathBuf>>,
    mcp_config: Mutex<Option<PathBuf>>,
    skill_dir: Mutex<Option<PathBuf>>,
    opencode: Mutex<Option<OpenCodeRuntime>>,
    codex: Mutex<Option<CodexRuntime>>,
    auto_approve_children: Mutex<bool>,
    redactions: Mutex<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct OpenCodeRuntime {
    pub base_url: String,
    pub port: u16,
}

pub struct CodexRuntime {
    pub url: String,
    pub sender: tokio::sync::mpsc::UnboundedSender<crate::codex::Decision>,
    pub server: Option<crate::codex::CodexServer>,
}

struct ClaudeRuntime {
    token: String,
    settings_path: PathBuf,
}

struct SessionData {
    session: Session,
    output: OutputLog,
    history: HistoryLog,
    runtime: Option<PtyRuntime>,
    process_exited: bool,
    closing: bool,
    preview_tail: String,
    last_preview_at: Option<Instant>,
    pending_hook: Option<PendingHook>,
}

struct PendingHook {
    permission: PendingPermission,
    sender: Option<oneshot::Sender<PermissionReply>>,
}

#[derive(Debug)]
pub struct PermissionReply {
    pub decision: PermissionDecision,
    pub reason: String,
}

pub enum BeginPermission {
    Held(oneshot::Receiver<PermissionReply>),
    Busy,
    Unavailable,
}

#[derive(Debug)]
pub enum PermissionAnswerFailure {
    Closed,
    NoPending,
    Mismatch,
    AlreadyAnswered,
}

struct OutputLog {
    chunks: VecDeque<OutputChunk>,
    bytes: usize,
    next_seq: u64,
}

struct PtyRuntime {
    killer: Box<dyn ChildKiller + Send + Sync>,
    process_group: Option<u32>,
    leader_pid: Option<u32>,
    stalled_release: Option<Arc<AtomicBool>>,
}

struct StalledWriter {
    _inner: Box<dyn Write + Send>,
    release: Arc<AtomicBool>,
}

impl Write for StalledWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        while !self.release.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(10));
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct OutputBudget {
    limit: usize,
    used: AtomicUsize,
    entries: Mutex<Vec<Weak<SessionEntry>>>,
}

#[derive(Debug)]
pub enum CloseFailure {
    AlreadyClosed,
    Closing,
}

#[derive(Debug)]
pub enum InputFailure {
    Closed,
    Closing,
    NotRunning,
}

#[derive(Debug)]
pub enum ResizeFailure {
    Closed,
    Closing,
    NotRunning,
}

#[derive(Debug)]
pub enum ReopenFailure {
    Active,
    Closing,
}

/// The bounded terminal output and conversation history restored from disk.
pub struct RestoredLogs {
    pub output: Vec<(u64, String)>,
    pub next_seq: u64,
    pub history: Vec<HistoryItem>,
    pub next_history_seq: u64,
}

impl SessionEntry {
    pub fn new(
        session: Session,
        output_budget: Arc<OutputBudget>,
        config: SessionRuntimeConfig,
    ) -> Self {
        Self::build(
            session,
            output_budget,
            config,
            TerminalSize::DEFAULT,
            OutputLog::new(),
            HistoryLog::new(),
            false,
        )
    }

    pub fn restore(
        session: Session,
        output_budget: Arc<OutputBudget>,
        size: TerminalSize,
        logs: RestoredLogs,
        repo_root: Option<String>,
        config: SessionRuntimeConfig,
    ) -> Self {
        let mut session = session;
        if session.status == SessionState::Active {
            session.work_state = Some(WorkState::Paused);
            session.waiting_for = None;
            session.preview = RESTART_PREVIEW.to_owned();
        }
        if session.status == SessionState::Closed {
            session.work_state = None;
            session.waiting_for = None;
        }
        // A demo server belongs to the agent's process. After a restart none
        // of them run, so every saved record becomes a stopped record.
        for server in &mut session.demo_servers {
            server.state = DemoServerState::Stopped;
        }
        session.pending_permission = None;

        let log = OutputLog::restored(logs.output, logs.next_seq);
        let bytes = log.bytes;
        let history = HistoryLog::restored(logs.history, logs.next_history_seq);
        let entry = Self::build(
            session,
            Arc::clone(&output_budget),
            config,
            size,
            log,
            history,
            true,
        );
        *lock(&entry.repo_root) = repo_root;
        output_budget.account(bytes as isize);
        entry
    }

    fn build(
        session: Session,
        output_budget: Arc<OutputBudget>,
        config: SessionRuntimeConfig,
        size: TerminalSize,
        output: OutputLog,
        history: HistoryLog,
        restored: bool,
    ) -> Self {
        let (output_tx, _) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
        let (close_tx, _) = watch::channel(restored);
        Self {
            data: Mutex::new(SessionData {
                session,
                output,
                history,
                runtime: None,
                process_exited: restored,
                closing: false,
                preview_tail: String::new(),
                last_preview_at: None,
                pending_hook: None,
            }),
            writer: Mutex::new(None),
            master: Mutex::new(None),
            command: Mutex::new(None),
            size: Mutex::new(size),
            repo_root: Mutex::new(None),
            write_gate: AsyncMutex::new(()),
            lifecycle: AsyncMutex::new(()),
            output_tx,
            events: config.events,
            aux: config.aux,
            close_tx,
            reader_stopped: AtomicBool::new(restored),
            output_budget,
            preview_throttle: config.preview_throttle,
            claude: Mutex::new(None),
            home: Mutex::new(None),
            mcp_config: Mutex::new(None),
            skill_dir: Mutex::new(None),
            opencode: Mutex::new(None),
            codex: Mutex::new(None),
            auto_approve_children: Mutex::new(false),
            redactions: Mutex::new(Vec::new()),
        }
    }

    pub fn set_auto_approve_children(&self, allow: bool) {
        *lock(&self.auto_approve_children) = allow;
    }

    pub fn auto_approves_children(&self) -> bool {
        *lock(&self.auto_approve_children)
    }

    pub fn set_codex_runtime(&self, runtime: CodexRuntime) {
        *lock(&self.codex) = Some(runtime);
    }

    pub fn codex_sender(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<crate::codex::Decision>> {
        lock(&self.codex)
            .as_ref()
            .map(|runtime| runtime.sender.clone())
    }

    pub fn set_opencode_runtime(&self, runtime: OpenCodeRuntime) {
        *lock(&self.opencode) = Some(runtime);
    }

    pub fn opencode_runtime(&self) -> Option<OpenCodeRuntime> {
        lock(&self.opencode).clone()
    }

    pub fn set_mcp_config(&self, path: PathBuf) {
        *lock(&self.mcp_config) = Some(path);
    }

    pub fn set_skill_dir(&self, path: PathBuf) {
        *lock(&self.skill_dir) = Some(path);
    }

    pub fn set_redactions(&self, secrets: Vec<String>) {
        *lock(&self.redactions) = secrets;
    }

    pub fn redactions(&self) -> Vec<String> {
        lock(&self.redactions).clone()
    }

    pub fn persistence_output(&self) -> (Vec<(u64, String)>, u64) {
        let data = lock(&self.data);
        (
            data.output
                .chunks
                .iter()
                .map(|chunk| (chunk.seq, chunk.data.clone()))
                .collect(),
            data.output.next_seq,
        )
    }

    pub fn persistence_history(&self) -> (Vec<HistoryItem>, u64) {
        let data = lock(&self.data);
        (data.history.items(), data.history.next_seq())
    }

    pub fn history_snapshot(&self, after: u64) -> Result<HistorySnapshot, SnapshotFailure> {
        let data = lock(&self.data);
        data.history.snapshot(after)
    }

    /// Returns true when this session has a structured conversation source.
    pub fn history_supported(&self) -> bool {
        let state = lock(&self.data);
        state.session.kind == SessionKind::Agent
            && state
                .session
                .client
                .is_some_and(crate::history::client_supports_history)
    }

    pub fn clear_history(&self) {
        let mut state = lock(&self.data);
        state.history = HistoryLog::new();
    }

    /// Applies one parsed change to the bounded history and sends it to the
    /// live streams. Kept text already has session secrets removed.
    pub fn record_history(&self, change: HistoryChange) -> Option<HistoryItem> {
        let change = {
            let secrets = lock(&self.redactions);
            change.redact(&secrets)
        };
        let (item, session_id) = {
            let mut state = lock(&self.data);
            let session_id = state.session.id.clone();
            let item = state.history.apply(change)?;
            (item, session_id)
        };
        let _ = self.aux.send(AuxEvent::History {
            session_id,
            item: item.clone(),
        });
        Some(item)
    }

    pub fn id(&self) -> String {
        lock(&self.data).session.id.clone()
    }

    pub fn session(&self) -> Session {
        lock(&self.data).session.clone()
    }

    pub fn note_agent_session_id(&self, id: &str) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.agent_session_id.as_deref() == Some(id) {
            return None;
        }
        state.session.agent_session_id = Some(id.to_owned());
        Some(state.session.clone())
    }

    pub fn note_context_left(&self, context_left: Option<u8>) -> Option<Session> {
        let mut state = lock(&self.data);
        let context_left = context_left.map(|value| value.min(100));
        if state.session.context_left == context_left {
            return None;
        }
        state.session.context_left = context_left;
        Some(state.session.clone())
    }

    pub fn short(&self) -> String {
        lock(&self.data).session.short.clone()
    }

    pub fn set_short(&self, short: String) {
        lock(&self.data).session.short = short;
    }

    /// Copies an AI config onto the session so a running session keeps its
    /// launch settings even when the stored config changes.
    pub fn set_launch_config(
        &self,
        config: &crate::state::AiConfig,
        launch_config: BTreeMap<String, String>,
    ) -> Session {
        let mut state = lock(&self.data);
        state.session.config_id = Some(config.id.clone());
        state.session.provider_id = Some(config.provider_id.clone());
        state.session.client = Some(config.client);
        state.session.model = config.model.clone();
        state.session.effort = config.effort.clone();
        state.session.args = config.args.clone();
        state.session.config = launch_config;
        state.session.clone()
    }

    pub fn set_agent_worktree(&self, folder: String, branch: String, repo_root: String) -> Session {
        let mut state = lock(&self.data);
        state.session.folder = folder;
        state.session.branch = Some(branch);
        let session = state.session.clone();
        *lock(&self.repo_root) = Some(repo_root);
        session
    }

    pub fn repo_root(&self) -> Option<String> {
        lock(&self.repo_root).clone()
    }

    pub fn set_claude_settings(&self, token: String, settings_path: PathBuf) {
        let mut claude = lock(&self.claude);
        if let Some(previous) = claude.take()
            && previous.settings_path != settings_path
        {
            claude::remove(&previous.settings_path);
        }
        *claude = Some(ClaudeRuntime {
            token,
            settings_path,
        });
    }

    pub fn claude_token(&self) -> Option<String> {
        lock(&self.claude)
            .as_ref()
            .map(|claude| claude.token.clone())
    }

    pub fn remove_claude_settings(&self) {
        if let Some(claude) = lock(&self.claude).take() {
            claude::remove(&claude.settings_path);
        }
    }

    pub fn set_home(&self, home: PathBuf) {
        *lock(&self.home) = Some(home);
    }

    pub fn home(&self) -> Option<PathBuf> {
        lock(&self.home).clone()
    }

    pub fn remove_home(&self) {
        if let Some(home) = lock(&self.home).take() {
            let _ = std::fs::remove_dir_all(home);
        }
    }

    pub fn remove_session_files(&self) {
        self.remove_claude_settings();
        self.remove_home();
        if let Some(path) = lock(&self.mcp_config).take() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = lock(&self.skill_dir).take() {
            let _ = std::fs::remove_dir_all(path);
        }
        let _ = lock(&self.codex).take();
    }

    pub fn try_lifecycle(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        self.lifecycle.try_lock().ok()
    }

    /// Waits for the lifecycle lock. A pause holds it while it asks the agent
    /// to finish and saves the worktree.
    pub async fn lock_lifecycle(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.lifecycle.lock().await
    }

    pub fn is_closing(&self) -> bool {
        lock(&self.data).closing
    }

    pub fn process_exited(&self) -> bool {
        lock(&self.data).process_exited
    }

    pub fn begin_close(&self) -> Result<bool, CloseFailure> {
        let (runtime, writer, master, should_wait) = {
            let mut state = lock(&self.data);
            if state.session.status == SessionState::Closed {
                return Err(CloseFailure::AlreadyClosed);
            }
            if state.closing {
                return Err(CloseFailure::Closing);
            }
            state.closing = true;
            resolve_pending(&mut state, "The session was closed.");
            let should_wait = state.runtime.is_some() && !state.process_exited;
            if state.runtime.is_none() {
                state.process_exited = true;
            }
            let runtime = state.runtime.take();
            drop(state);

            let writer = {
                let mut writer = lock(&self.writer);
                writer.take()
            };
            let master = {
                let mut master = lock(&self.master);
                master.take()
            };
            (runtime, writer, master, should_wait)
        };

        drop(writer);
        drop(master);
        drop(runtime);
        Ok(should_wait)
    }

    pub fn finish_close(&self, preview: Option<&str>) -> Session {
        let session = {
            let mut state = lock(&self.data);
            resolve_pending(&mut state, "The session was closed.");
            state.session.status = SessionState::Closed;
            state.session.work_state = None;
            state.session.waiting_for = None;
            mark_demo_servers_stopped(&mut state.session);
            state.closing = false;
            state.preview_tail.clear();
            state.last_preview_at = None;
            if let Some(preview) = preview {
                state.session.preview = preview.to_owned();
            }
            state.session.clone()
        };

        self.remove_session_files();
        self.stop_reader();
        let _ = self.close_tx.send(true);
        session
    }

    pub fn set_preview(&self, preview: String) -> Session {
        let mut state = lock(&self.data);
        state.session.preview = preview;
        state.session.clone()
    }

    pub fn finish_switch(
        &self,
        config_id: String,
        provider_id: String,
        client: Client,
        shortcuts: Vec<Shortcut>,
        agent_session_id: Option<String>,
    ) -> Session {
        let mut state = lock(&self.data);
        let previous_client = state.session.client;
        state.closing = false;
        state.process_exited = false;
        state.session.status = SessionState::Active;
        state.session.work_state = Some(WorkState::Working);
        state.session.waiting_for = None;
        state.session.pending_permission = None;
        state.session.config_id = Some(config_id);
        state.session.provider_id = Some(provider_id);
        state.session.client = Some(client);
        // A switch to a different client starts a new conversation, so the
        // earlier client's items no longer describe what the session shows.
        if previous_client.is_some() && previous_client != Some(client) {
            state.history = HistoryLog::new();
        }
        state.session.agent_session_id = agent_session_id;
        state.session.shortcuts = shortcuts;
        state.session.context_left = None;
        state.session.preview = format!("Switched to {client}.");
        state.preview_tail.clear();
        state.last_preview_at = None;
        state.session.clone()
    }

    pub fn mark_switch_failed(&self, reason: &str) -> Session {
        let mut state = lock(&self.data);
        state.closing = false;
        state.process_exited = true;
        state.session.work_state = Some(WorkState::Input);
        state.session.waiting_for = None;
        state.session.pending_permission = None;
        state.session.preview = reason.to_owned();
        state.session.clone()
    }

    pub fn finish_resume(&self) -> Session {
        let mut state = lock(&self.data);
        state.closing = false;
        state.process_exited = false;
        state.session.status = SessionState::Active;
        state.session.work_state = Some(WorkState::Working);
        state.session.waiting_for = None;
        state.session.pending_permission = None;
        state.session.context_left = None;
        state.session.preview = "Resumed.".to_owned();
        state.preview_tail.clear();
        state.last_preview_at = None;
        state.session.clone()
    }

    pub fn begin_recovery(&self, preview: &str) -> Session {
        let session = {
            let mut state = lock(&self.data);
            resolve_pending(&mut state, "The app restarted.");
            state.closing = false;
            state.process_exited = false;
            state.session.status = SessionState::Active;
            state.session.work_state = Some(WorkState::Working);
            state.session.waiting_for = None;
            state.session.pending_permission = None;
            state.session.context_left = None;
            state.session.preview = preview.to_owned();
            state.preview_tail.clear();
            state.last_preview_at = None;
            state.session.clone()
        };

        self.reader_stopped.store(false, Ordering::SeqCst);
        let _ = self.close_tx.send(false);
        session
    }

    pub fn pause(&self, reason: &str) -> Session {
        let mut state = lock(&self.data);
        state.closing = false;
        state.process_exited = true;
        state.session.status = SessionState::Active;
        state.session.work_state = Some(WorkState::Paused);
        state.session.waiting_for = None;
        state.session.pending_permission = None;
        mark_demo_servers_stopped(&mut state.session);
        state.session.preview = reason.to_owned();
        state.preview_tail.clear();
        state.last_preview_at = None;
        state.session.clone()
    }

    pub fn mark_question(&self, text: &str) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active {
            return None;
        }
        state.session.waiting_for = Some(WaitingFor::Question);
        state.session.work_state = Some(WorkState::Waiting);
        state.session.preview = format!("Question: {text}");
        Some(state.session.clone())
    }

    pub fn clear_question(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.waiting_for != Some(WaitingFor::Question) {
            return None;
        }
        state.session.waiting_for = None;
        state.session.work_state = Some(WorkState::Working);
        Some(state.session.clone())
    }

    /// Marks the session as idle and waiting for the user's next prompt.
    ///
    /// A pending permission prompt or question keeps its own waiting state, so
    /// an idle notice cannot hide a card that still needs an answer. The
    /// failed-switch error state is separate: it marks the process as gone, and
    /// this live signal is ignored there.
    pub fn mark_input(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        if matches!(
            state.session.waiting_for,
            Some(WaitingFor::Permission) | Some(WaitingFor::Question)
        ) {
            return None;
        }
        if state.session.waiting_for == Some(WaitingFor::Input)
            && state.session.work_state == Some(WorkState::Waiting)
        {
            return None;
        }
        state.session.waiting_for = Some(WaitingFor::Input);
        state.session.work_state = Some(WorkState::Waiting);
        Some(state.session.clone())
    }

    pub fn report_done(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active {
            return None;
        }
        state.session.work_state = Some(WorkState::Done);
        state.session.waiting_for = None;
        Some(state.session.clone())
    }

    /// Registers a demo server for this session.
    ///
    /// A repeated port in the same session updates its name and makes it active
    /// again. A closed session refuses the registration.
    pub fn register_demo_server(
        &self,
        name: String,
        port: u16,
        tailscale_address: Option<String>,
    ) -> Result<Session, DemoFailure> {
        let name = crate::demo::clean_name(&name).ok_or(DemoFailure::InvalidName)?;
        if !crate::demo::port_is_allowed(port) {
            return Err(DemoFailure::InvalidPort);
        }
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active {
            return Err(DemoFailure::Closed);
        }
        if let Some(existing) = state
            .session
            .demo_servers
            .iter_mut()
            .find(|server| server.port == port)
        {
            existing.name = name;
            existing.tailscale_address = tailscale_address;
            existing.state = DemoServerState::Active;
            return Ok(state.session.clone());
        }
        let active = state
            .session
            .demo_servers
            .iter()
            .filter(|server| server.is_active())
            .count();
        if active >= MAX_DEMO_SERVERS {
            return Err(DemoFailure::TooMany);
        }
        // Keep the whole list bounded. A stopped record is a stale process, so
        // the oldest one makes room for a new registration.
        if state.session.demo_servers.len() >= MAX_DEMO_SERVERS {
            let Some(index) = state
                .session
                .demo_servers
                .iter()
                .position(|server| !server.is_active())
            else {
                return Err(DemoFailure::TooMany);
            };
            state.session.demo_servers.remove(index);
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let session_id = state.session.id.clone();
        state.session.demo_servers.push(crate::demo::new_server(
            &session_id,
            name,
            port,
            tailscale_address,
            &now,
        ));
        Ok(state.session.clone())
    }

    /// Sets or clears the public address of one demo server.
    pub fn set_funnel(
        &self,
        server_id: &str,
        funnel_address: Option<String>,
    ) -> Result<Session, DemoFailure> {
        let mut state = lock(&self.data);
        let Some(server) = state
            .session
            .demo_servers
            .iter_mut()
            .find(|server| server.id == server_id)
        else {
            return Err(DemoFailure::NotFound);
        };
        server.funnel_address = funnel_address;
        Ok(state.session.clone())
    }

    /// Stops one demo server in this session.
    pub fn stop_demo_server(&self, server_id: &str) -> Result<Session, DemoFailure> {
        let mut state = lock(&self.data);
        let Some(server) = state
            .session
            .demo_servers
            .iter_mut()
            .find(|server| server.id == server_id)
        else {
            return Err(DemoFailure::NotFound);
        };
        server.state = DemoServerState::Stopped;
        server.funnel_address = None;
        Ok(state.session.clone())
    }

    pub fn rename(&self, name: String) -> Session {
        let mut state = lock(&self.data);
        state.session.name = name;
        state.session.clone()
    }

    /// Changes the project that groups the session. The folder, branch,
    /// worktree, process and log stay as they are.
    pub fn move_to_project(&self, project_id: String) -> Session {
        let mut state = lock(&self.data);
        state.session.project_id = project_id;
        state.session.clone()
    }

    pub fn set_allowed_configs(&self, config_ids: Vec<String>) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.session.kind != SessionKind::Agent
        {
            return None;
        }
        state.session.allowed_config_ids = config_ids;
        Some(state.session.clone())
    }

    /// Drops a removed config from a session. A closed session that ran the
    /// config keeps its log and says the config is gone.
    pub fn forget_config(&self, config_id: &str) -> bool {
        let mut state = lock(&self.data);
        let mut changed = false;
        let before = state.session.allowed_config_ids.len();
        state
            .session
            .allowed_config_ids
            .retain(|id| id != config_id);
        changed |= state.session.allowed_config_ids.len() != before;
        if state.session.config_id.as_deref() == Some(config_id) {
            state.session.config_id = None;
            state.session.provider_id = None;
            if state.session.status == SessionState::Closed {
                state.session.preview =
                    "The AI config was removed. Choose another config.".to_owned();
            }
            changed = true;
        }
        changed
    }

    pub fn set_handoff_note(&self, note: String) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active {
            return None;
        }
        state.session.handoff_note = Some(note);
        Some(state.session.clone())
    }

    /// Builds the starting context for an agent that takes over the session.
    ///
    /// The context holds the saved handoff note and the end of the output log.
    /// It returns `None` when the session has neither.
    pub fn handoff_context(&self) -> Option<String> {
        let state = lock(&self.data);
        let note = state
            .session
            .handoff_note
            .as_deref()
            .map(|note| collapse_handoff_text(note, HANDOFF_CONTEXT_MAX_CHARS))
            .filter(|note| !note.is_empty());
        let log = state.output.tail_text(HANDOFF_LOG_TAIL_CHARS);
        if note.is_none() && log.is_empty() {
            return None;
        }

        let mut context = String::from("Handoff from the previous agent. ");
        if let Some(note) = note {
            context.push_str("Note: ");
            context.push_str(&note);
            context.push_str(". ");
        }
        if !log.is_empty() {
            context.push_str("End of the previous log: ");
            context.push_str(&log);
            context.push_str(". ");
        }
        context.push_str("Continue the work. Do not repeat steps that are already done.");
        context.truncate(
            context
                .char_indices()
                .nth(HANDOFF_CONTEXT_MAX_CHARS)
                .map_or(context.len(), |(index, _)| index),
        );
        Some(context)
    }

    pub fn stored_command(&self) -> Option<CommandBuilder> {
        lock(&self.command).clone()
    }

    pub fn stored_size(&self) -> TerminalSize {
        *lock(&self.size)
    }

    pub fn begin_reopen(&self) -> Result<(), ReopenFailure> {
        {
            let mut state = lock(&self.data);
            if state.closing {
                return Err(ReopenFailure::Closing);
            }
            if state.session.status != SessionState::Closed {
                return Err(ReopenFailure::Active);
            }
            state.session.status = SessionState::Active;
            state.session.work_state = Some(WorkState::Working);
            state.session.waiting_for = None;
            state.session.pending_permission = None;
            state.pending_hook = None;
            state.process_exited = false;
            state.closing = false;
            state.preview_tail.clear();
            state.last_preview_at = None;
        }

        self.reader_stopped.store(false, Ordering::SeqCst);
        let _ = self.close_tx.send(false);
        Ok(())
    }

    pub fn rollback_reopen(&self) {
        let runtime = {
            let mut state = lock(&self.data);
            resolve_pending(&mut state, "The session could not start.");
            state.session.status = SessionState::Closed;
            state.session.work_state = None;
            state.session.waiting_for = None;
            state.process_exited = false;
            state.closing = false;
            state.runtime.take()
        };
        self.remove_session_files();
        let writer = {
            let mut writer = lock(&self.writer);
            writer.take()
        };
        let master = {
            let mut master = lock(&self.master);
            master.take()
        };

        self.reader_stopped.store(true, Ordering::SeqCst);
        let _ = self.close_tx.send(true);
        drop(writer);
        drop(master);
        drop(runtime);
    }

    pub fn is_active(&self) -> bool {
        lock(&self.data).session.status == SessionState::Active
    }

    pub fn is_closed(&self) -> bool {
        lock(&self.data).session.status == SessionState::Closed
    }

    pub fn mark_process_exited(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active {
            return None;
        }
        state.process_exited = true;
        resolve_pending(&mut state, "The agent exited.");
        if state.closing {
            return None;
        }
        state.session.work_state = Some(WorkState::Done);
        state.session.waiting_for = None;
        Some(state.session.clone())
    }

    pub fn mark_restarted(&self) {
        let mut state = lock(&self.data);
        if state.session.status == SessionState::Closed {
            state.session.status = SessionState::Active;
            state.session.work_state = Some(WorkState::Paused);
            state.session.preview = RESTART_PREVIEW.to_owned();
        }
    }

    pub fn begin_permission(&self, permission: PendingPermission) -> BeginPermission {
        let (receiver, event) = {
            let mut state = lock(&self.data);
            if state.session.status != SessionState::Active || state.closing || state.process_exited
            {
                return BeginPermission::Unavailable;
            }
            if state
                .pending_hook
                .as_ref()
                .is_some_and(|pending| pending.sender.is_some())
            {
                return BeginPermission::Busy;
            }
            state.pending_hook = None;
            let (sender, receiver) = oneshot::channel();
            state.pending_hook = Some(PendingHook {
                permission: permission.clone(),
                sender: Some(sender),
            });
            state.session.pending_permission = Some(permission);
            state.session.waiting_for = Some(WaitingFor::Permission);
            (receiver, state.session.clone())
        };
        self.publish(event);
        BeginPermission::Held(receiver)
    }

    pub fn answer_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), PermissionAnswerFailure> {
        let mut state = lock(&self.data);
        if state.session.status == SessionState::Closed || state.closing {
            return Err(PermissionAnswerFailure::Closed);
        }
        let Some(pending) = state.pending_hook.as_mut() else {
            return Err(PermissionAnswerFailure::NoPending);
        };
        if pending.permission.request_id != request_id {
            return Err(PermissionAnswerFailure::Mismatch);
        }
        let Some(sender) = pending.sender.take() else {
            return Err(PermissionAnswerFailure::AlreadyAnswered);
        };
        let reason = match decision {
            PermissionDecision::Allow => "The user allowed this action.",
            PermissionDecision::Deny => "The user denied this action.",
        };
        if sender
            .send(PermissionReply {
                decision,
                reason: reason.to_owned(),
            })
            .is_err()
        {
            return Err(PermissionAnswerFailure::NoPending);
        }
        Ok(())
    }

    pub fn clear_unanswered_permission(&self, request_id: &str) -> Option<Session> {
        let mut state = lock(&self.data);
        let pending = state.pending_hook.as_ref()?;
        if pending.permission.request_id != request_id || pending.sender.is_none() {
            return None;
        }
        let _ = state.pending_hook.take();
        clear_permission_state(&mut state);
        Some(state.session.clone())
    }

    pub fn mark_external_permission(&self, permission: PendingPermission) -> Option<Session> {
        let event = {
            let mut state = lock(&self.data);
            if state.session.status != SessionState::Active || state.closing || state.process_exited
            {
                return None;
            }
            if let Some(pending) = state.pending_hook.as_ref()
                && (pending.sender.is_some()
                    || pending.permission.request_id == permission.request_id)
            {
                return None;
            }
            state.pending_hook = Some(PendingHook {
                permission: permission.clone(),
                sender: None,
            });
            state.session.pending_permission = Some(permission);
            state.session.waiting_for = Some(WaitingFor::Permission);
            state.session.clone()
        };
        self.publish(event);
        Some(self.session())
    }

    pub fn answer_external_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), PermissionAnswerFailure> {
        let mut state = lock(&self.data);
        if state.session.status == SessionState::Closed || state.closing {
            return Err(PermissionAnswerFailure::Closed);
        }
        let Some(pending) = state.pending_hook.as_ref() else {
            return Err(PermissionAnswerFailure::NoPending);
        };
        if pending.permission.request_id != request_id {
            return Err(PermissionAnswerFailure::Mismatch);
        }
        if pending.sender.is_some() {
            return Err(PermissionAnswerFailure::AlreadyAnswered);
        }
        let _ = state.pending_hook.take();
        clear_permission_state(&mut state);
        let session = state.session.clone();
        drop(state);
        self.publish(session);
        let _ = decision;
        Ok(())
    }

    pub fn clear_external_permission(&self, request_id: &str) -> Option<Session> {
        let mut state = lock(&self.data);
        let pending = state.pending_hook.as_ref()?;
        if pending.permission.request_id != request_id || pending.sender.is_some() {
            return None;
        }
        let _ = state.pending_hook.take();
        clear_permission_state(&mut state);
        Some(state.session.clone())
    }

    pub fn mark_working(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        let had_permission = state.session.pending_permission.is_some()
            || state.session.waiting_for == Some(WaitingFor::Permission);
        resolve_pending(&mut state, "The agent received a new prompt.");
        let was_waiting = state.session.waiting_for.is_some();
        state.session.waiting_for = None;
        if !had_permission && !was_waiting && state.session.work_state == Some(WorkState::Working) {
            return None;
        }
        state.session.work_state = Some(WorkState::Working);
        Some(state.session.clone())
    }

    pub fn finish_tool(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        let had_permission = state.session.pending_permission.is_some()
            || state.session.waiting_for == Some(WaitingFor::Permission);
        resolve_pending(&mut state, "The agent continued without an answer.");
        let changed = had_permission
            || state.session.waiting_for.is_some()
            || state.session.work_state != Some(WorkState::Working);
        state.session.waiting_for = None;
        state.session.work_state = Some(WorkState::Working);
        changed.then(|| state.session.clone())
    }

    pub fn mark_done(&self, preview: Option<String>) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        let had_permission = state.session.pending_permission.is_some()
            || state.session.waiting_for == Some(WaitingFor::Permission);
        resolve_pending(&mut state, "The agent stopped.");
        let was_waiting = state.session.waiting_for.is_some();
        let previous_preview = state.session.preview.clone();
        if let Some(preview) = preview.as_deref().and_then(preview_from_output) {
            state.session.preview = preview;
        }
        let changed = had_permission
            || was_waiting
            || state.session.work_state != Some(WorkState::Done)
            || state.session.preview != previous_preview;
        state.session.waiting_for = None;
        state.session.work_state = Some(WorkState::Done);
        changed.then(|| state.session.clone())
    }

    pub fn mark_permission_note(&self) -> Option<Session> {
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        if state.session.waiting_for == Some(WaitingFor::Permission) {
            return None;
        }
        state.session.waiting_for = Some(WaitingFor::Permission);
        Some(state.session.clone())
    }

    pub fn hook_preview(&self, text: &str) -> Option<Session> {
        let preview = preview_from_output(text)?;
        let mut state = lock(&self.data);
        if state.session.status != SessionState::Active || state.closing || state.process_exited {
            return None;
        }
        if state.session.preview == preview {
            return None;
        }
        state.session.preview = preview;
        state.last_preview_at = Some(Instant::now());
        Some(state.session.clone())
    }

    fn publish(&self, session: Session) {
        let _ = self.events.send(SessionEvent::updated(session));
    }

    pub fn resize(&self, size: TerminalSize) -> Result<(), ResizeFailure> {
        {
            let state = lock(&self.data);
            if state.session.status == SessionState::Closed {
                return Err(ResizeFailure::Closed);
            }
            if state.closing {
                return Err(ResizeFailure::Closing);
            }
            if state.runtime.is_none() || state.process_exited {
                return Err(ResizeFailure::NotRunning);
            }
        }

        let master = lock(&self.master);
        let Some(master) = master.as_ref() else {
            return Err(ResizeFailure::NotRunning);
        };
        master
            .resize(size.pty_size())
            .map_err(|_| ResizeFailure::NotRunning)?;
        *lock(&self.size) = size;
        Ok(())
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<OutputChunk> {
        self.output_tx.subscribe()
    }

    pub fn subscribe_close(&self) -> watch::Receiver<bool> {
        self.close_tx.subscribe()
    }

    pub fn output_snapshot(&self, after: u64) -> Result<OutputSnapshot, SnapshotFailure> {
        let data = lock(&self.data);
        data.output.snapshot(after)
    }

    pub fn append_output(&self, data: String) {
        let data = {
            let secrets = lock(&self.redactions);
            redact_secrets(data, &secrets)
        };
        let (chunk, delta, preview_event) = {
            let mut state = lock(&self.data);
            if !data.is_empty() {
                append_preview_tail(&mut state.preview_tail, &data);
            }
            let before = state.output.bytes;
            let chunk = state.output.push(data);
            let after = state.output.bytes;

            let mut preview_event = None;
            if chunk.is_some() && state.session.status == SessionState::Active && !state.closing {
                let due = state
                    .last_preview_at
                    .is_none_or(|updated| updated.elapsed() >= self.preview_throttle);
                if due
                    && let Some(preview) = preview_from_output(&state.preview_tail)
                    && preview != state.session.preview
                {
                    state.session.preview = preview;
                    state.last_preview_at = Some(Instant::now());
                    preview_event = Some(state.session.clone());
                }
            }

            (chunk, after as isize - before as isize, preview_event)
        };

        if let Some(chunk) = chunk {
            let _ = self.output_tx.send(chunk);
        }
        self.output_budget.account(delta);
        self.output_budget.enforce();
        if let Some(session) = preview_event {
            let _ = self.events.send(SessionEvent::updated(session));
        }
    }

    pub async fn write_input(&self, bytes: Vec<u8>) -> Result<(), InputFailure> {
        let (writer, session_id) = {
            let state = lock(&self.data);
            if state.session.status == SessionState::Closed {
                return Err(InputFailure::Closed);
            }
            if state.closing {
                return Err(InputFailure::Closing);
            }
            if state.runtime.is_none() || state.process_exited {
                return Err(InputFailure::NotRunning);
            }
            (lock(&self.writer).clone(), state.session.id.clone())
        };
        let writer = writer.ok_or(InputFailure::NotRunning)?;

        let _gate = self.write_gate.lock().await;
        {
            let state = lock(&self.data);
            if state.session.status == SessionState::Closed {
                return Err(InputFailure::Closed);
            }
            if state.closing {
                return Err(InputFailure::Closing);
            }
            if state.runtime.is_none() || state.process_exited {
                return Err(InputFailure::NotRunning);
            }
        }

        let result = tokio::task::spawn_blocking(move || {
            let mut writer = lock(writer.as_ref());
            writer.write_all(&bytes).and_then(|()| writer.flush())
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                tracing::warn!(session = %session_id, %error, "session input write failed");
                Err(InputFailure::NotRunning)
            }
            Err(error) => {
                tracing::warn!(session = %session_id, %error, "session input write failed");
                Err(InputFailure::NotRunning)
            }
        }
    }

    fn clear_output(&self) -> usize {
        let mut state = lock(&self.data);
        let bytes = state.output.bytes;
        state.output.chunks.clear();
        state.output.bytes = 0;
        bytes
    }

    fn stop_reader(&self) {
        self.reader_stopped.store(true, Ordering::SeqCst);
    }

    fn reader_stopped(&self) -> bool {
        self.reader_stopped.load(Ordering::SeqCst)
    }
}

impl OutputLog {
    fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
        }
    }

    fn restored(chunks: Vec<(u64, String)>, next_seq: u64) -> Self {
        let mut log = Self::new();
        let mut last_seq = 0_u64;

        for (seq, data) in chunks {
            if seq <= last_seq {
                continue;
            }
            log.bytes = log.bytes.saturating_add(data.len());
            log.chunks.push_back(OutputChunk {
                seq,
                data,
                gap: None,
            });
            last_seq = seq;
        }

        while log.bytes > OUTPUT_LOG_LIMIT {
            let Some(removed) = log.chunks.pop_front() else {
                break;
            };
            log.bytes = log.bytes.saturating_sub(removed.data.len());
        }

        log.next_seq = next_seq.max(last_seq.saturating_add(1)).max(1);
        log
    }

    fn push(&mut self, data: String) -> Option<OutputChunk> {
        if data.is_empty() {
            return None;
        }

        let chunk = OutputChunk {
            seq: self.next_seq,
            data,
            gap: None,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.bytes = self.bytes.saturating_add(chunk.data.len());
        self.chunks.push_back(chunk.clone());

        while self.bytes > OUTPUT_LOG_LIMIT {
            let Some(removed) = self.chunks.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.data.len());
        }

        Some(chunk)
    }

    fn snapshot(&self, after: u64) -> Result<OutputSnapshot, SnapshotFailure> {
        let newest_seq = self.next_seq.saturating_sub(1);
        if after > newest_seq {
            return Err(SnapshotFailure::AfterAhead { newest: newest_seq });
        }

        let mut chunks = self
            .chunks
            .iter()
            .filter(|chunk| chunk.seq > after)
            .cloned()
            .collect::<Vec<_>>();

        if chunks.is_empty() {
            if after < newest_seq {
                return Err(SnapshotFailure::LogUnavailable);
            }
            return Ok(OutputSnapshot { chunks, newest_seq });
        }

        let oldest_seq = self.chunks.front().map(|chunk| chunk.seq);
        if let Some(oldest_seq) = oldest_seq.filter(|oldest| after.saturating_add(1) < *oldest)
            && let Some(first) = chunks.first_mut()
        {
            first.gap = Some(OutputGap {
                after,
                oldest_seq,
                newest_seq,
            });
        }

        Ok(OutputSnapshot { chunks, newest_seq })
    }

    /// Returns the end of the log as one line of plain text.
    fn tail_text(&self, limit_chars: usize) -> String {
        let mut parts = Vec::new();
        let mut bytes = 0;
        for chunk in self.chunks.iter().rev() {
            if bytes >= limit_chars {
                break;
            }
            bytes = bytes.saturating_add(chunk.data.len());
            parts.push(chunk.data.as_str());
        }
        parts.reverse();
        let text = strip_terminal_sequences(&parts.concat());
        let count = text.chars().count();
        let tail = if count > limit_chars {
            text.chars().skip(count - limit_chars).collect()
        } else {
            text
        };
        collapse_handoff_text(&tail, limit_chars)
    }
}

impl OutputBudget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: AtomicUsize::new(0),
            entries: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, entry: &Arc<SessionEntry>) {
        let mut entries = lock(&self.entries);
        entries.retain(|entry| entry.strong_count() > 0);
        entries.push(Arc::downgrade(entry));
    }

    pub fn enforce(&self) {
        let mut remaining = self.used.load(Ordering::Acquire);
        if remaining <= self.limit {
            return;
        }

        let entries = {
            let mut entries = lock(&self.entries);
            entries.retain(|entry| entry.strong_count() > 0);
            entries.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
        };

        for closed_only in [true, false] {
            for entry in &entries {
                if remaining <= self.limit {
                    return;
                }
                if entry.is_closed() != closed_only {
                    continue;
                }
                let freed = entry.clear_output();
                if freed > 0 {
                    remaining = remaining.saturating_sub(freed);
                    self.subtract(freed);
                }
            }
        }
    }

    fn account(&self, delta: isize) {
        if delta > 0 {
            self.used.fetch_add(delta as usize, Ordering::AcqRel);
        } else if delta < 0 {
            self.subtract(delta.unsigned_abs());
        }
    }

    fn subtract(&self, bytes: usize) {
        let mut current = self.used.load(Ordering::Acquire);
        while current > 0 {
            match self.used.compare_exchange(
                current,
                current.saturating_sub(bytes),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }
}

impl Drop for PtyRuntime {
    fn drop(&mut self) {
        if let Some(release) = &self.stalled_release {
            release.store(true, Ordering::Release);
        }

        #[cfg(unix)]
        {
            let descendants = self.leader_pid.map(collect_descendants).unwrap_or_default();

            if let Some(group) = self.process_group
                && group > 0
            {
                signal_group(group, libc::SIGHUP);
                for pid in &descendants {
                    signal_pid(*pid, libc::SIGHUP);
                }
                thread::spawn(move || {
                    thread::sleep(GROUP_KILL_GRACE);
                    signal_group(group, libc::SIGKILL);
                    for pid in descendants {
                        signal_pid(pid, libc::SIGKILL);
                    }
                });
                return;
            }

            if let Some(pid) = self.leader_pid {
                signal_pid(pid, libc::SIGHUP);
                for child in &descendants {
                    signal_pid(*child, libc::SIGHUP);
                }
                thread::spawn(move || {
                    thread::sleep(GROUP_KILL_GRACE);
                    signal_pid(pid, libc::SIGKILL);
                    for child in descendants {
                        signal_pid(child, libc::SIGKILL);
                    }
                });
                return;
            }
        }

        let _ = self.killer.kill();
    }
}

pub fn spawn(
    entry: &Arc<SessionEntry>,
    mut command: CommandBuilder,
    folder: &Path,
    size: TerminalSize,
    stall_writer: bool,
    events: broadcast::Sender<SessionEvent>,
    environment: &[(String, String)],
) -> Result<(), String> {
    {
        let mut stored = lock(&entry.command);
        *stored = Some(command.clone());
        let mut stored_size = lock(&entry.size);
        *stored_size = size;
    }

    command.cwd(folder);
    configure_environment(&mut command);
    for (name, value) in environment {
        command.env(name, value);
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size.pty_size())
        .map_err(|error| format!("Could not open a terminal: {error}"))?;

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("Could not start the session: {error}"))?;
    let killer = child.clone_killer();
    let leader_pid = child.process_id();

    #[cfg(unix)]
    let process_group = pair.master.process_group_leader().map(|pid| pid as u32);
    #[cfg(not(unix))]
    let process_group = None;

    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            return Err(format!("Could not read the terminal: {error}"));
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            let _ = child.kill();
            return Err(format!("Could not write to the terminal: {error}"));
        }
    };
    let (writer, stalled_release) = if stall_writer {
        let release = Arc::new(AtomicBool::new(false));
        let writer: Box<dyn Write + Send> = Box::new(StalledWriter {
            _inner: writer,
            release: Arc::clone(&release),
        });
        (writer, Some(release))
    } else {
        (writer, None)
    };
    drop(pair.slave);

    let session_id = entry.id();
    let exit_entry = Arc::clone(entry);
    let waiter = thread::Builder::new()
        .name(format!("pty-wait-{session_id}"))
        .spawn(move || {
            let _ = child.wait();
            exit_entry.remove_session_files();
            if let Some(session) = exit_entry.mark_process_exited() {
                let _ = events.send(SessionEvent::updated(session));
            }
        });
    if let Err(error) = waiter {
        #[cfg(unix)]
        if let Some(group) = process_group
            && group > 0
        {
            signal_group(group, libc::SIGKILL);
        }
        let mut killer = killer;
        let _ = killer.kill();
        return Err(format!("Could not supervise the session: {error}"));
    }

    let writer = Arc::new(Mutex::new(writer));
    {
        let mut state = lock(&entry.data);
        state.runtime = Some(PtyRuntime {
            killer,
            process_group,
            leader_pid,
            stalled_release,
        });
    }
    {
        let mut handle = lock(&entry.writer);
        *handle = Some(Arc::clone(&writer));
    }

    let reader_entry = Arc::clone(entry);
    let reader_thread = thread::Builder::new()
        .name(format!("pty-read-{session_id}"))
        .spawn(move || read_output(reader, reader_entry));
    if let Err(error) = reader_thread {
        let runtime = {
            let mut state = lock(&entry.data);
            state.runtime.take()
        };
        let writer = {
            let mut handle = lock(&entry.writer);
            handle.take()
        };
        drop(writer);
        drop(runtime);
        return Err(format!("Could not read the terminal: {error}"));
    }

    {
        let mut master = lock(&entry.master);
        *master = Some(pair.master);
    }

    Ok(())
}

pub fn close_entry(
    entry: &SessionEntry,
    events: &broadcast::Sender<SessionEvent>,
) -> Result<Session, CloseFailure> {
    let (session, runtime) = {
        let mut state = lock(&entry.data);
        if state.session.status == SessionState::Closed {
            return Err(CloseFailure::AlreadyClosed);
        }
        resolve_pending(&mut state, "The session was closed.");
        state.session.status = SessionState::Closed;
        state.session.work_state = None;
        state.session.waiting_for = None;
        mark_demo_servers_stopped(&mut state.session);
        (state.session.clone(), state.runtime.take())
    };

    entry.remove_session_files();
    entry.stop_reader();
    let _ = entry.close_tx.send(true);
    let writer = {
        let mut writer = lock(&entry.writer);
        writer.take()
    };
    let master = {
        let mut master = lock(&entry.master);
        master.take()
    };
    drop(writer);
    drop(master);
    drop(runtime);
    let _ = events.send(SessionEvent::updated(session.clone()));
    Ok(session)
}

fn configure_environment(command: &mut CommandBuilder) {
    command.env_clear();
    for name in ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL"] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("TERM", "xterm-256color");
}

fn mark_demo_servers_stopped(session: &mut Session) {
    for server in &mut session.demo_servers {
        server.state = DemoServerState::Stopped;
        // A public address belongs to a running server. Stop clears it with
        // the server, and the caller turns the Funnel mapping off first.
        server.funnel_address = None;
    }
}

fn resolve_pending(state: &mut SessionData, reason: &str) {
    if let Some(pending) = state.pending_hook.take()
        && let Some(sender) = pending.sender
    {
        let _ = sender.send(PermissionReply {
            decision: PermissionDecision::Deny,
            reason: reason.to_owned(),
        });
    }
    clear_permission_state(state);
}

fn clear_permission_state(state: &mut SessionData) {
    state.session.pending_permission = None;
    if state.session.waiting_for == Some(WaitingFor::Permission) {
        state.session.waiting_for = None;
    }
}

const PREVIEW_TAIL_LIMIT: usize = 8192;
const PREVIEW_MAX_CHARS: usize = 80;

fn redact_secrets(data: String, secrets: &[String]) -> String {
    crate::text::redact_secrets(data, secrets)
}

fn append_preview_tail(tail: &mut String, data: &str) {
    tail.push_str(data);
    if tail.len() > PREVIEW_TAIL_LIMIT {
        let mut cut = tail.len() - PREVIEW_TAIL_LIMIT;
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail.drain(..cut);
    }
}

/// Collapses text to one line so a terminal client receives it as a single prompt.
fn collapse_handoff_text(text: &str, limit_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(limit_chars).collect()
}

fn preview_from_output(text: &str) -> Option<String> {
    let clean = strip_terminal_sequences(text);
    for line in clean.lines().rev() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        return Some(collapsed.chars().take(PREVIEW_MAX_CHARS).collect());
    }
    None
}

fn strip_terminal_sequences(text: &str) -> String {
    let mut output = String::new();
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            match characters.next() {
                Some('[') => {
                    for next in characters.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(next) = characters.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && characters.peek() == Some(&'\\') {
                            characters.next();
                            break;
                        }
                    }
                }
                Some(_) | None => {}
            }
        } else if character == '\n' || character == '\r' {
            output.push('\n');
        } else if character == '\t' {
            output.push(' ');
        } else if !character.is_control() {
            output.push(character);
        }
    }

    output
}

#[cfg(unix)]
fn signal_group(group: u32, signal: i32) {
    if let Ok(group) = i32::try_from(group) {
        unsafe {
            libc::kill(-group, signal);
        }
    }
}

#[cfg(unix)]
fn signal_pid(pid: u32, signal: i32) {
    if let Ok(pid) = i32::try_from(pid)
        && pid > 0
    {
        unsafe {
            libc::kill(pid, signal);
        }
    }
}

#[cfg(unix)]
fn collect_descendants(root: u32) -> Vec<u32> {
    let mut descendants = Vec::new();
    let mut seen = HashSet::new();
    collect_children(root, &mut descendants, &mut seen, 0);
    descendants
}

#[cfg(unix)]
fn collect_children(
    parent: u32,
    descendants: &mut Vec<u32>,
    seen: &mut HashSet<u32>,
    depth: usize,
) {
    if depth >= 32 {
        return;
    }
    for child in child_pids(parent) {
        if !seen.insert(child) {
            continue;
        }
        descendants.push(child);
        collect_children(child, descendants, seen, depth + 1);
    }
}

#[cfg(target_os = "macos")]
fn child_pids(parent: u32) -> Vec<u32> {
    let mut buffer = [0_i32; 512];
    let capacity = std::mem::size_of_val(&buffer);
    let Ok(parent) = i32::try_from(parent) else {
        return Vec::new();
    };
    let bytes =
        unsafe { libc::proc_listchildpids(parent, buffer.as_mut_ptr().cast(), capacity as i32) };
    if bytes <= 0 {
        return Vec::new();
    }
    let count = (bytes as usize).min(buffer.len());
    buffer[..count]
        .iter()
        .filter_map(|pid| u32::try_from(*pid).ok())
        .filter(|pid| *pid > 0)
        .collect()
}

#[cfg(target_os = "linux")]
fn child_pids(parent: u32) -> Vec<u32> {
    let path = format!("/proc/{parent}/task/{parent}/children");
    let Ok(children) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    children
        .split_whitespace()
        .filter_map(|pid| pid.parse::<u32>().ok())
        .collect()
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn child_pids(_parent: u32) -> Vec<u32> {
    Vec::new()
}

fn read_output(mut reader: Box<dyn Read + Send>, entry: Arc<SessionEntry>) {
    let mut buffer = [0_u8; 8192];
    let mut pending = Vec::new();

    while !entry.reader_stopped() {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if entry.reader_stopped() {
                    break;
                }
                pending.extend_from_slice(&buffer[..read]);
                let text = drain_complete_utf8(&mut pending);
                if !text.is_empty() {
                    entry.append_output(text);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }

    if !entry.reader_stopped() && !pending.is_empty() {
        entry.append_output(String::from_utf8_lossy(&pending).into_owned());
    }
}

fn drain_complete_utf8(pending: &mut Vec<u8>) -> String {
    let mut output = String::new();

    loop {
        let (valid, invalid) = match std::str::from_utf8(pending) {
            Ok(text) => {
                output.push_str(text);
                pending.clear();
                break;
            }
            Err(error) => (error.valid_up_to(), error.error_len()),
        };

        if valid > 0 {
            if let Ok(text) = std::str::from_utf8(&pending[..valid]) {
                output.push_str(text);
            }
            pending.drain(..valid);
        }

        match invalid {
            Some(length) => {
                output.push('\u{FFFD}');
                pending.drain(..length.min(pending.len()));
            }
            None => break,
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_session_does_not_update_its_preview() {
        let session = Session {
            id: "closed".to_owned(),
            name: "Closed".to_owned(),
            short: "closed".to_owned(),
            project_id: "web".to_owned(),
            kind: SessionKind::Terminal,
            status: SessionState::Closed,
            work_state: None,
            client: None,
            config_id: None,
            allowed_config_ids: Vec::new(),
            provider_id: None,
            legacy_account_id: None,
            legacy_allowed_account_ids: Vec::new(),
            legacy_preset_id: None,
            branch: None,
            folder: "/tmp".to_owned(),
            source_repository: None,
            preview: "keep me".to_owned(),
            waiting_for: None,
            pending_permission: None,
            first_prompt: None,
            parent_session_id: None,
            parent_session_name: None,
            env_overrides: Vec::new(),
            model: None,
            effort: None,
            args: Vec::new(),
            config: BTreeMap::new(),
            agent_session_id: None,
            handoff_note: None,
            context_left: None,
            demo_servers: Vec::new(),
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            shortcuts: Vec::new(),
        };
        let (events, _) = broadcast::channel(8);
        let mut receiver = events.subscribe();
        let (aux, _) = broadcast::channel(8);
        let entry = SessionEntry::new(
            session,
            Arc::new(OutputBudget::new(1024 * 1024)),
            SessionRuntimeConfig::new(events, aux, Duration::from_secs(2)),
        );

        entry.append_output("a newer line\n".to_owned());

        assert_eq!(entry.session().preview, "keep me");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn history_redacts_injected_secrets() {
        let session = Session {
            id: "history-redact".to_owned(),
            name: "History redact".to_owned(),
            short: "history-redact".to_owned(),
            project_id: "web".to_owned(),
            kind: SessionKind::Terminal,
            status: SessionState::Active,
            work_state: Some(WorkState::Working),
            client: None,
            config_id: None,
            allowed_config_ids: Vec::new(),
            provider_id: None,
            legacy_account_id: None,
            legacy_allowed_account_ids: Vec::new(),
            legacy_preset_id: None,
            branch: None,
            folder: "/tmp".to_owned(),
            source_repository: None,
            preview: String::new(),
            waiting_for: None,
            pending_permission: None,
            first_prompt: None,
            parent_session_id: None,
            parent_session_name: None,
            env_overrides: Vec::new(),
            model: None,
            effort: None,
            args: Vec::new(),
            config: BTreeMap::new(),
            agent_session_id: None,
            handoff_note: None,
            context_left: None,
            demo_servers: Vec::new(),
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            shortcuts: Vec::new(),
        };
        let (events, _) = broadcast::channel(8);
        let (aux, mut aux_receiver) = broadcast::channel(8);
        let entry = SessionEntry::new(
            session,
            Arc::new(OutputBudget::new(1024 * 1024)),
            SessionRuntimeConfig::new(events, aux, Duration::from_secs(2)),
        );
        entry.set_redactions(vec!["sk-secret-value".to_owned()]);

        entry
            .record_history(HistoryChange::Prompt {
                id: "prompt-1".to_owned(),
                text: "the key is sk-secret-value".to_owned(),
            })
            .expect("history item");

        let (history, _) = entry.persistence_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "the key is [redacted]");
        assert!(!history[0].text.contains("sk-secret-value"));
        assert!(matches!(
            aux_receiver.try_recv(),
            Ok(AuxEvent::History { .. })
        ));
    }

    #[test]
    fn history_replays_from_a_sequence() {
        let session = Session {
            id: "history-replay".to_owned(),
            name: "History replay".to_owned(),
            short: "history-replay".to_owned(),
            project_id: "web".to_owned(),
            kind: SessionKind::Terminal,
            status: SessionState::Active,
            work_state: Some(WorkState::Working),
            client: None,
            config_id: None,
            allowed_config_ids: Vec::new(),
            provider_id: None,
            legacy_account_id: None,
            legacy_allowed_account_ids: Vec::new(),
            legacy_preset_id: None,
            branch: None,
            folder: "/tmp".to_owned(),
            source_repository: None,
            preview: String::new(),
            waiting_for: None,
            pending_permission: None,
            first_prompt: None,
            parent_session_id: None,
            parent_session_name: None,
            env_overrides: Vec::new(),
            model: None,
            effort: None,
            args: Vec::new(),
            config: BTreeMap::new(),
            agent_session_id: None,
            handoff_note: None,
            context_left: None,
            demo_servers: Vec::new(),
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            shortcuts: Vec::new(),
        };
        let (events, _) = broadcast::channel(8);
        let (aux, _) = broadcast::channel(8);
        let entry = SessionEntry::new(
            session,
            Arc::new(OutputBudget::new(1024 * 1024)),
            SessionRuntimeConfig::new(events, aux, Duration::from_secs(2)),
        );
        entry.record_history(HistoryChange::Prompt {
            id: "prompt-1".to_owned(),
            text: "first".to_owned(),
        });
        entry.record_history(HistoryChange::Tool {
            id: "tool-1".to_owned(),
            tool: "Bash".to_owned(),
            text: "cargo test".to_owned(),
            state: crate::history::HistoryState::Done,
        });

        let replay = entry.history_snapshot(1).expect("replay");
        assert_eq!(replay.items.len(), 1);
        assert!(replay.items[0].id.starts_with('h'));
        assert_eq!(replay.newest_seq, 2);
        assert!(entry.history_snapshot(3).is_err());
    }

    #[test]
    fn output_redacts_injected_secrets() {
        let session = Session {
            id: "redact".to_owned(),
            name: "Redact".to_owned(),
            short: "redact".to_owned(),
            project_id: "web".to_owned(),
            kind: SessionKind::Terminal,
            status: SessionState::Active,
            work_state: Some(WorkState::Working),
            client: None,
            config_id: None,
            allowed_config_ids: Vec::new(),
            provider_id: None,
            legacy_account_id: None,
            legacy_allowed_account_ids: Vec::new(),
            legacy_preset_id: None,
            branch: None,
            folder: "/tmp".to_owned(),
            source_repository: None,
            preview: String::new(),
            waiting_for: None,
            pending_permission: None,
            first_prompt: None,
            parent_session_id: None,
            parent_session_name: None,
            env_overrides: Vec::new(),
            model: None,
            effort: None,
            args: Vec::new(),
            config: BTreeMap::new(),
            agent_session_id: None,
            handoff_note: None,
            context_left: None,
            demo_servers: Vec::new(),
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            shortcuts: Vec::new(),
        };
        let (events, _) = broadcast::channel(8);
        let (aux, _) = broadcast::channel(8);
        let entry = SessionEntry::new(
            session,
            Arc::new(OutputBudget::new(1024 * 1024)),
            SessionRuntimeConfig::new(events, aux, Duration::from_secs(2)),
        );
        entry.set_redactions(vec!["sk-secret-value".to_owned()]);

        entry.append_output("key is sk-secret-value now\n".to_owned());

        let (chunks, _) = entry.persistence_output();
        let text = chunks.into_iter().map(|(_, data)| data).collect::<String>();
        assert!(text.contains("[redacted]"), "{text}");
        assert!(!text.contains("sk-secret-value"), "{text}");
    }

    #[test]
    fn clients_get_their_own_prompt_and_resume_arguments() {
        assert_eq!(
            prompt_arguments(Client::ClaudeCode, "fix it"),
            vec!["fix it".to_owned()]
        );
        assert_eq!(
            prompt_arguments(Client::Codex, "fix it"),
            vec!["fix it".to_owned()]
        );
        assert_eq!(
            prompt_arguments(Client::Antigravity, "fix it"),
            vec!["--prompt-interactive".to_owned(), "fix it".to_owned()]
        );
        assert_eq!(
            prompt_arguments(Client::OpenCode2, "fix it"),
            vec!["--prompt".to_owned(), "fix it".to_owned()]
        );
        assert!(!client_needs_prompt_input(Client::OpenCode2));

        assert_eq!(resume_arguments(Client::ClaudeCode, None), ["--continue"]);
        assert_eq!(
            resume_arguments(Client::ClaudeCode, Some("conversation-1")),
            ["--resume", "conversation-1"]
        );
        assert_eq!(resume_arguments(Client::Antigravity, None), ["--continue"]);
        assert_eq!(
            resume_arguments(Client::Antigravity, Some("conversation-1")),
            ["--continue"]
        );
        assert_eq!(resume_arguments(Client::Codex, None), ["resume", "--last"]);
        assert_eq!(
            resume_arguments(Client::Codex, Some("conversation-1")),
            ["resume", "conversation-1"]
        );
        assert_eq!(resume_arguments(Client::OpenCode2, None), ["--continue"]);
        assert_eq!(
            resume_arguments(Client::OpenCode2, Some("conversation-1")),
            ["--session", "conversation-1"]
        );
        assert_eq!(
            resume_arguments(Client::ClaudeCode, Some("  ")),
            ["--continue"]
        );
    }
}
