use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, broadcast};

use crate::clients;
use crate::persist::{LoadResult, PersistedOutput, PersistedSession, PersistedState, StateStore};
use crate::secrets::{KeyringStore, MemorySecretStore, SecretStore};
use crate::seed;
use crate::sessions::{self, CloseFailure, OutputBudget, SessionEntry, TerminalSize};
use crate::slug::{slugify, with_suffix};

const EVENT_CHANNEL_CAPACITY: usize = 256;
const HEALTH_CACHE_TTL: Duration = Duration::from_secs(10);
const MAX_SSE_CLIENTS: usize = 32;
const OUTPUT_MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_PUSH_TIMEOUT: Duration = Duration::from_secs(30);
const PREVIEW_THROTTLE: Duration = Duration::from_secs(2);
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_PORT: u16 = 4319;
pub const MAX_ACTIVE_SESSIONS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Active,
    Closed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkState {
    Working,
    Waiting,
    Input,
    Paused,
    Done,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WaitingFor {
    Permission,
    Input,
    Start,
    Question,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Agent,
    Terminal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Client {
    #[serde(rename = "Claude Code")]
    ClaudeCode,
    #[serde(rename = "Codex")]
    Codex,
    #[serde(rename = "OpenCode 2")]
    OpenCode2,
    #[serde(rename = "Antigravity")]
    Antigravity,
}

impl Client {
    pub const ALL: [Client; 4] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::OpenCode2,
        Client::Antigravity,
    ];

    pub fn binary(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude",
            Client::Codex => "codex",
            Client::OpenCode2 => "opencode",
            Client::Antigravity => "agy",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::OpenCode2 => "OpenCode 2",
            Client::Antigravity => "Antigravity",
        }
    }
}

impl std::fmt::Display for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.display_name())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Subscription,
    Key,
    Local,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Shortcut {
    pub key: String,
    pub label: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub request_id: String,
    pub tool: String,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvOverride {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub short: String,
    pub project_id: String,
    pub kind: SessionKind,
    pub status: SessionState,
    pub work_state: Option<WorkState>,
    pub client: Option<Client>,
    pub account_id: Option<String>,
    pub branch: Option<String>,
    pub folder: String,
    pub preview: String,
    pub waiting_for: Option<WaitingFor>,
    #[serde(default)]
    pub pending_permission: Option<PendingPermission>,
    #[serde(default)]
    pub first_prompt: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub parent_session_name: Option<String>,
    #[serde(default)]
    pub env_overrides: Vec<EnvOverride>,
    pub started_at: String,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub text: String,
    pub options: Vec<String>,
    pub status: QuestionStatus,
    pub answer: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Pending,
    Answered,
    Withdrawn,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRequest {
    pub id: String,
    pub parent_session_id: String,
    pub parent_session_name: String,
    pub project_id: String,
    pub project_name: String,
    pub name: Option<String>,
    pub folder: String,
    pub source_branch: Option<String>,
    pub first_prompt: Option<String>,
    pub account_id: String,
    pub client: Client,
    pub status: RequestStatus,
    pub child_session_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum AuxEvent {
    #[serde(rename = "question.updated")]
    Question { question: Question },
    #[serde(rename = "request.updated")]
    Request { request: SessionRequest },
    #[serde(rename = "notification")]
    Notification { notification: Notification },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder: String,
    #[serde(default)]
    pub default_account_id: Option<String>,
    #[serde(default)]
    pub default_source_branch: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsageRow {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub client: Client,
    pub provider: String,
    pub auth: Auth,
    #[serde(default)]
    pub sample: bool,
    pub rows: Vec<UsageRow>,
    pub foot: String,
    pub server: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub api_style: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentCheck {
    pub name: Client,
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
    pub agents: Vec<AgentCheck>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub projects: Vec<Project>,
    pub accounts: Vec<Account>,
    pub sessions: Vec<Session>,
    pub questions: Vec<Question>,
    pub session_requests: Vec<SessionRequest>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub session: Session,
}

impl SessionEvent {
    pub fn updated(session: Session) -> Self {
        Self {
            event_type: "session.updated",
            session,
        }
    }
}

#[derive(Debug)]
pub struct SessionLimitReached;

#[derive(Debug)]
pub struct DuplicateProjectFolder;

#[derive(Debug)]
pub enum DeleteSessionFailure {
    NotFound,
    Active,
}

#[derive(Debug)]
pub enum QuestionFailure {
    NotFound,
    NotPending,
    Closed,
}

struct CachedHealth {
    checked_at: Instant,
    agents: Vec<AgentCheck>,
}

#[derive(Clone, Default)]
struct SseCounter {
    active: Arc<AtomicUsize>,
}

pub struct SsePermit {
    counter: SseCounter,
}

impl SseCounter {
    fn acquire(&self) -> Option<SsePermit> {
        loop {
            let active = self.active.load(Ordering::Acquire);
            if active >= MAX_SSE_CLIENTS {
                return None;
            }
            if self
                .active
                .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(SsePermit {
                    counter: self.clone(),
                });
            }
        }
    }
}

impl Drop for SsePermit {
    fn drop(&mut self) {
        self.counter.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct SessionStore {
    entries: HashMap<String, Arc<SessionEntry>>,
    order: Vec<String>,
    active: usize,
}

impl SessionStore {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            active: 0,
        }
    }
}

struct Inner {
    projects: Mutex<Vec<Project>>,
    accounts: Mutex<Vec<Account>>,
    secrets: Arc<dyn SecretStore>,
    sessions: Mutex<SessionStore>,
    events: broadcast::Sender<SessionEvent>,
    questions: Mutex<Vec<Question>>,
    session_requests: Mutex<Vec<SessionRequest>>,
    aux_events: broadcast::Sender<AuxEvent>,
    command_override: Option<Vec<String>>,
    agent_bin: Option<PathBuf>,
    session_cap: usize,
    stall_writer: bool,
    output_budget: Arc<OutputBudget>,
    output_sse: SseCounter,
    events_sse: SseCounter,
    health: AsyncMutex<Option<CachedHealth>>,
    git_timeout: Duration,
    git_push_timeout: Duration,
    preview_throttle: Duration,
    permission_timeout: Duration,
    port: u16,
    agent_identity: AsyncMutex<()>,
    store: Option<Arc<StateStore>>,
    persist_lock: Mutex<()>,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl AppState {
    pub fn new() -> Self {
        Self::new_with_port(DEFAULT_PORT)
    }

    pub fn new_with_port(port: u16) -> Self {
        let secrets: Arc<dyn SecretStore> = if env::var_os("ELESDEE_MEMORY_SECRETS").is_some() {
            Arc::new(MemorySecretStore::default())
        } else {
            Arc::new(KeyringStore::new())
        };
        let mut state = Self::from_store(
            StateStore::from_environment().map(Arc::new),
            secrets,
            None,
            MAX_ACTIVE_SESSIONS,
            false,
        );
        if let Some(inner) = Arc::get_mut(&mut state.inner) {
            inner.port = port;
            inner.agent_bin = env::var_os("ELESDEE_AGENT_BIN")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
        }
        state
    }

    #[doc(hidden)]
    pub fn ephemeral() -> Self {
        Self::from_parts(
            Vec::new(),
            seed::accounts(),
            Arc::new(MemorySecretStore::default()),
            None,
            MAX_ACTIVE_SESSIONS,
            false,
            None,
        )
    }

    #[doc(hidden)]
    pub fn with_data_dir(directory: impl Into<PathBuf>) -> Self {
        let store = match StateStore::new(directory.into()) {
            Ok(store) => Some(Arc::new(store)),
            Err(error) => {
                tracing::error!(%error, "could not prepare the test data directory");
                None
            }
        };
        Self::from_store(
            store,
            Arc::new(MemorySecretStore::default()),
            None,
            MAX_ACTIVE_SESSIONS,
            false,
        )
    }

    #[doc(hidden)]
    pub fn with_test_command(argv: Vec<String>) -> Self {
        Self::from_parts(
            Vec::new(),
            seed::accounts(),
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            MAX_ACTIVE_SESSIONS,
            false,
            None,
        )
    }

    #[doc(hidden)]
    pub fn with_test_command_and_data_dir(
        argv: Vec<String>,
        directory: impl Into<PathBuf>,
    ) -> Self {
        let store = match StateStore::new(directory.into()) {
            Ok(store) => Some(Arc::new(store)),
            Err(error) => {
                tracing::error!(%error, "could not prepare the test data directory");
                None
            }
        };
        Self::from_store(
            store,
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            MAX_ACTIVE_SESSIONS,
            false,
        )
    }

    #[doc(hidden)]
    pub fn with_test_command_and_session_cap(argv: Vec<String>, session_cap: usize) -> Self {
        Self::from_parts(
            Vec::new(),
            seed::accounts(),
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            session_cap,
            false,
            None,
        )
    }

    #[doc(hidden)]
    pub fn with_test_stalled_writer(argv: Vec<String>) -> Self {
        Self::from_parts(
            Vec::new(),
            seed::accounts(),
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            MAX_ACTIVE_SESSIONS,
            true,
            None,
        )
    }

    fn from_store(
        store: Option<Arc<StateStore>>,
        secrets: Arc<dyn SecretStore>,
        command_override: Option<Vec<String>>,
        session_cap: usize,
        stall_writer: bool,
    ) -> Self {
        let (projects, accounts, sessions, questions, session_requests) =
            match store.as_ref().map(|store| store.load()) {
                Some(LoadResult::Loaded(state)) => (
                    state.projects,
                    state.accounts.unwrap_or_else(seed::accounts),
                    state.sessions,
                    state.questions.unwrap_or_default(),
                    state.session_requests.unwrap_or_default(),
                ),
                Some(LoadResult::Missing | LoadResult::Corrupt) | None => (
                    Vec::new(),
                    seed::accounts(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            };
        let mut accounts = accounts;
        let seed_ids = seed::accounts()
            .into_iter()
            .map(|account| account.id)
            .collect::<HashSet<_>>();
        for account in &mut accounts {
            if seed_ids.contains(&account.id) {
                account.sample = true;
            }
        }

        let state = Self::from_parts(
            projects,
            accounts,
            secrets,
            command_override,
            session_cap,
            stall_writer,
            store,
        );
        for session in sessions {
            state.restore_session(session);
        }
        let pending = questions
            .into_iter()
            .filter(|question| {
                question.status == QuestionStatus::Pending
                    && state
                        .session(&question.session_id)
                        .is_some_and(|entry| entry.session().status == SessionState::Active)
            })
            .collect();
        *lock(&state.inner.questions) = pending;
        let pending_requests = session_requests
            .into_iter()
            .filter(|request| {
                request.status == RequestStatus::Pending
                    && state
                        .session(&request.parent_session_id)
                        .is_some_and(|entry| entry.session().status == SessionState::Active)
            })
            .collect();
        *lock(&state.inner.session_requests) = pending_requests;
        if let Some(directory) = state.data_dir() {
            crate::claude::sweep(directory);
        }
        state.inner.output_budget.enforce();
        state
    }

    fn from_parts(
        projects: Vec<Project>,
        accounts: Vec<Account>,
        secrets: Arc<dyn SecretStore>,
        command_override: Option<Vec<String>>,
        session_cap: usize,
        stall_writer: bool,
        store: Option<Arc<StateStore>>,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (aux_events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                projects: Mutex::new(projects),
                accounts: Mutex::new(accounts),
                secrets,
                sessions: Mutex::new(SessionStore::new()),
                events,
                questions: Mutex::new(Vec::new()),
                session_requests: Mutex::new(Vec::new()),
                aux_events,
                command_override,
                agent_bin: None,
                session_cap,
                stall_writer,
                output_budget: Arc::new(OutputBudget::new(OUTPUT_MEMORY_LIMIT)),
                output_sse: SseCounter::default(),
                events_sse: SseCounter::default(),
                health: AsyncMutex::new(None),
                git_timeout: GIT_TIMEOUT,
                git_push_timeout: GIT_PUSH_TIMEOUT,
                preview_throttle: PREVIEW_THROTTLE,
                permission_timeout: PERMISSION_TIMEOUT,
                port: DEFAULT_PORT,
                agent_identity: AsyncMutex::new(()),
                store,
                persist_lock: Mutex::new(()),
            }),
        }
    }

    #[doc(hidden)]
    pub fn with_test_git_timeout(mut self, local: Duration, push: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.git_timeout = local;
            inner.git_push_timeout = push;
        }
        self
    }

    pub fn git_timeout(&self) -> Duration {
        self.inner.git_timeout
    }

    pub fn git_push_timeout(&self) -> Duration {
        self.inner.git_push_timeout
    }

    #[doc(hidden)]
    pub fn with_test_preview_throttle(mut self, throttle: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.preview_throttle = throttle;
        }
        self
    }

    pub fn preview_throttle(&self) -> Duration {
        self.inner.preview_throttle
    }

    #[doc(hidden)]
    pub fn with_test_permission_timeout(mut self, timeout: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.permission_timeout = timeout;
        }
        self
    }

    pub fn permission_timeout(&self) -> Duration {
        self.inner.permission_timeout
    }

    pub fn port(&self) -> u16 {
        self.inner.port
    }

    #[doc(hidden)]
    pub fn with_test_secret_store(mut self, store: Arc<dyn SecretStore>) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.secrets = store;
        }
        self
    }

    pub(crate) fn secret_store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.inner.secrets)
    }

    pub async fn lock_agent_identity(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.agent_identity.lock().await
    }

    pub fn try_reserve_short(&self, entry: &SessionEntry, project_id: &str, short: &str) -> bool {
        let store = lock(&self.inner.sessions);
        let in_use = store.entries.values().any(|other| {
            let session = other.session();
            session.project_id == project_id && session.short == short && session.id != entry.id()
        });
        if in_use {
            return false;
        }
        entry.set_short(short.to_owned());
        true
    }

    pub fn projects(&self) -> Vec<Project> {
        lock(&self.inner.projects).clone()
    }

    pub fn accounts(&self) -> Vec<Account> {
        lock(&self.inner.accounts).clone()
    }

    pub fn add_account(&self, mut account: Account) -> Account {
        let mut accounts = lock(&self.inner.accounts);
        let base = slugify(&account.name, "account");
        let id = with_suffix(&base, |candidate| {
            accounts.iter().any(|existing| existing.id == candidate)
        });
        account.id = id;
        accounts.push(account.clone());
        account
    }

    pub fn remove_account(&self, id: &str) {
        lock(&self.inner.accounts).retain(|account| account.id != id);
    }

    pub fn update_account_rows(&self, id: &str, rows: Vec<UsageRow>) -> Option<Account> {
        let mut accounts = lock(&self.inner.accounts);
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.rows = rows;
        Some(account.clone())
    }

    pub fn pending_questions(&self) -> Vec<Question> {
        lock(&self.inner.questions)
            .iter()
            .filter(|question| question.status == QuestionStatus::Pending)
            .cloned()
            .collect()
    }

    pub fn question(&self, id: &str) -> Option<Question> {
        lock(&self.inner.questions)
            .iter()
            .find(|question| question.id == id)
            .cloned()
    }

    pub fn add_question(
        &self,
        session_id: &str,
        text: String,
        options: Vec<String>,
    ) -> Result<Question, QuestionFailure> {
        let entry = self.session(session_id).ok_or(QuestionFailure::NotFound)?;
        let session = entry.session();
        if session.status != SessionState::Closed {
            let question = Question {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session.id.clone(),
                session_name: session.name.clone(),
                text,
                options,
                status: QuestionStatus::Pending,
                answer: None,
                created_at: timestamp(),
            };
            lock(&self.inner.questions).push(question.clone());
            if let Some(updated) = entry.mark_question(&question.text) {
                self.emit(SessionEvent::updated(updated));
            }
            self.emit_aux(AuxEvent::Question {
                question: question.clone(),
            });
            self.save();
            Ok(question)
        } else {
            Err(QuestionFailure::Closed)
        }
    }

    pub fn answer_question(&self, id: &str, answer: String) -> Result<Question, QuestionFailure> {
        let question = {
            let mut questions = lock(&self.inner.questions);
            let question = questions
                .iter_mut()
                .find(|question| question.id == id)
                .ok_or(QuestionFailure::NotFound)?;
            if question.status != QuestionStatus::Pending {
                return Err(QuestionFailure::NotPending);
            }
            question.status = QuestionStatus::Answered;
            question.answer = Some(answer);
            question.clone()
        };
        self.finish_question(&question);
        Ok(question)
    }

    pub fn withdraw_question(&self, id: &str) -> Result<Question, QuestionFailure> {
        let question = {
            let mut questions = lock(&self.inner.questions);
            let question = questions
                .iter_mut()
                .find(|question| question.id == id)
                .ok_or(QuestionFailure::NotFound)?;
            if question.status != QuestionStatus::Pending {
                return Err(QuestionFailure::NotPending);
            }
            question.status = QuestionStatus::Withdrawn;
            question.clone()
        };
        self.finish_question(&question);
        Ok(question)
    }

    fn finish_question(&self, question: &Question) {
        let still_pending = lock(&self.inner.questions).iter().any(|other| {
            other.session_id == question.session_id && other.status == QuestionStatus::Pending
        });
        if !still_pending
            && let Some(entry) = self.session(&question.session_id)
            && let Some(updated) = entry.clear_question()
        {
            self.emit(SessionEvent::updated(updated));
        }
        self.emit_aux(AuxEvent::Question {
            question: question.clone(),
        });
        self.save();
    }

    pub fn withdraw_questions_for_session(&self, session_id: &str) {
        let withdrawn = {
            let mut questions = lock(&self.inner.questions);
            questions
                .iter_mut()
                .filter(|question| {
                    question.session_id == session_id && question.status == QuestionStatus::Pending
                })
                .map(|question| {
                    question.status = QuestionStatus::Withdrawn;
                    question.clone()
                })
                .collect::<Vec<_>>()
        };
        for question in withdrawn {
            self.emit_aux(AuxEvent::Question { question });
        }
    }

    pub fn has_pending_question(&self, session_id: &str) -> bool {
        lock(&self.inner.questions).iter().any(|question| {
            question.session_id == session_id && question.status == QuestionStatus::Pending
        })
    }

    pub fn withdraw_requests_for_session(&self, session_id: &str) {
        let finished = {
            let mut requests = lock(&self.inner.session_requests);
            requests
                .iter_mut()
                .filter(|request| {
                    request.parent_session_id == session_id
                        && request.status == RequestStatus::Pending
                })
                .map(|request| {
                    request.status = RequestStatus::Denied;
                    request.clone()
                })
                .collect::<Vec<_>>()
        };
        for request in finished {
            self.emit_aux(AuxEvent::Request { request });
        }
    }

    pub fn notify(&self, session_id: &str, message: String) -> Option<Notification> {
        let entry = self.session(session_id)?;
        let session = entry.session();
        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            session_name: session.name.clone(),
            message,
            created_at: timestamp(),
        };
        self.emit_aux(AuxEvent::Notification {
            notification: notification.clone(),
        });
        Some(notification)
    }

    pub fn subscribe_aux(&self) -> broadcast::Receiver<AuxEvent> {
        self.inner.aux_events.subscribe()
    }

    pub fn pending_session_requests(&self) -> Vec<SessionRequest> {
        lock(&self.inner.session_requests)
            .iter()
            .filter(|request| request.status == RequestStatus::Pending)
            .cloned()
            .collect()
    }

    pub fn session_request(&self, id: &str) -> Option<SessionRequest> {
        lock(&self.inner.session_requests)
            .iter()
            .find(|request| request.id == id)
            .cloned()
    }

    pub fn add_session_request(&self, request: SessionRequest) -> SessionRequest {
        lock(&self.inner.session_requests).push(request.clone());
        self.emit_aux(AuxEvent::Request {
            request: request.clone(),
        });
        self.save();
        request
    }

    pub fn finish_session_request(
        &self,
        id: &str,
        status: RequestStatus,
        child_session_id: Option<String>,
    ) -> Option<SessionRequest> {
        let request = {
            let mut requests = lock(&self.inner.session_requests);
            let request = requests.iter_mut().find(|request| request.id == id)?;
            if request.status != RequestStatus::Pending {
                return None;
            }
            request.status = status;
            request.child_session_id = child_session_id;
            request.clone()
        };
        self.emit_aux(AuxEvent::Request {
            request: request.clone(),
        });
        self.save();
        Some(request)
    }

    fn emit_aux(&self, event: AuxEvent) {
        let _ = self.inner.aux_events.send(event);
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        lock(&self.inner.projects)
            .iter()
            .find(|project| project.id == id)
            .cloned()
    }

    pub fn add_project(
        &self,
        name: String,
        folder: String,
    ) -> Result<Project, DuplicateProjectFolder> {
        let mut projects = lock(&self.inner.projects);
        if projects.iter().any(|project| project.folder == folder) {
            return Err(DuplicateProjectFolder);
        }

        let base = slugify(&name, "project");
        let id = with_suffix(&base, |candidate| {
            projects.iter().any(|project| project.id == candidate)
        });
        let project = Project {
            id,
            name,
            folder,
            default_account_id: None,
            default_source_branch: None,
        };
        projects.push(project.clone());
        Ok(project)
    }

    pub fn update_project_defaults(
        &self,
        id: &str,
        default_account_id: Option<Option<String>>,
        default_source_branch: Option<Option<String>>,
    ) -> Option<Project> {
        let mut projects = lock(&self.inner.projects);
        let project = projects.iter_mut().find(|project| project.id == id)?;
        if let Some(value) = default_account_id {
            project.default_account_id = value;
        }
        if let Some(value) = default_source_branch {
            project.default_source_branch = value;
        }
        Some(project.clone())
    }

    pub fn account(&self, id: &str) -> Option<Account> {
        lock(&self.inner.accounts)
            .iter()
            .find(|account| account.id == id)
            .cloned()
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.inner.store.as_ref().map(|store| store.directory())
    }

    pub fn session(&self, id: &str) -> Option<Arc<SessionEntry>> {
        let store = lock(&self.inner.sessions);
        store.entries.get(id).cloned()
    }

    pub fn sessions(&self) -> Vec<Arc<SessionEntry>> {
        self.entries_in_order()
    }

    pub fn bootstrap(&self) -> Bootstrap {
        let entries = self.entries_in_order();
        let sessions = entries
            .iter()
            .map(|entry| entry.session())
            .collect::<Vec<_>>();
        Bootstrap {
            projects: lock(&self.inner.projects).clone(),
            accounts: self.accounts(),
            sessions,
            questions: self.pending_questions(),
            session_requests: self.pending_session_requests(),
        }
    }

    pub fn insert_session(&self, entry: Arc<SessionEntry>) -> Result<(), SessionLimitReached> {
        let id = entry.id();
        let mut store = lock(&self.inner.sessions);
        let is_active = entry.is_active();
        if is_active && store.active >= self.inner.session_cap {
            return Err(SessionLimitReached);
        }
        let base = entry.short();
        let unique = unique_short(&store.entries, &entry.session().project_id, &base);
        if unique != base {
            entry.set_short(unique);
        }
        if !store.entries.contains_key(&id) {
            store.order.push(id.clone());
        }
        store.entries.insert(id, entry);
        if is_active {
            store.active += 1;
        }
        Ok(())
    }

    pub fn remove_session(&self, id: &str) {
        let mut store = lock(&self.inner.sessions);
        if let Some(entry) = store.entries.remove(id) {
            if entry.is_active() {
                store.active = store.active.saturating_sub(1);
            }
        }
        store.order.retain(|existing| existing != id);
    }

    fn restore_session(&self, persisted: PersistedSession) {
        let size =
            TerminalSize::new(persisted.cols, persisted.rows).unwrap_or(TerminalSize::DEFAULT);
        let output = persisted
            .output
            .into_iter()
            .map(|chunk| (chunk.seq, chunk.data))
            .collect();
        let budget = self.output_budget();
        let entry = Arc::new(SessionEntry::restore(
            persisted.session,
            Arc::clone(&budget),
            size,
            output,
            persisted.next_seq,
            persisted.repo_root,
            sessions::SessionRuntimeConfig::new(
                self.inner.events.clone(),
                self.inner.preview_throttle,
            ),
        ));
        budget.register(&entry);

        let id = entry.id();
        let mut store = lock(&self.inner.sessions);
        store.order.push(id.clone());
        store.entries.insert(id, entry);
    }

    pub fn save(&self) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let _guard = lock(&self.inner.persist_lock);
        let projects = lock(&self.inner.projects).clone();
        let sessions = self
            .entries_in_order()
            .iter()
            .map(|entry| {
                let (output, next_seq) = entry.persistence_output();
                let size = entry.stored_size();
                let mut session = entry.session();
                session.pending_permission = None;
                if session.waiting_for == Some(WaitingFor::Permission) {
                    session.waiting_for = None;
                }
                PersistedSession {
                    session,
                    cols: size.cols,
                    rows: size.rows,
                    output: output
                        .into_iter()
                        .map(|(seq, data)| PersistedOutput { seq, data })
                        .collect(),
                    next_seq,
                    repo_root: entry.repo_root(),
                }
            })
            .collect();
        let state = PersistedState::new(
            projects,
            Some(self.accounts()),
            Some(self.pending_questions()),
            Some(self.pending_session_requests()),
            sessions,
        );
        if let Err(error) = store.save(&state) {
            tracing::error!(%error, "could not save state");
        }
    }

    pub fn close_session(&self, entry: &SessionEntry) -> Result<Session, CloseFailure> {
        let result = sessions::close_entry(entry, &self.inner.events);
        if result.is_ok() {
            let mut store = lock(&self.inner.sessions);
            store.active = store.active.saturating_sub(1);
            drop(store);
            self.inner.output_budget.enforce();
        }
        result
    }

    pub fn reserve_activation(&self) -> Result<(), SessionLimitReached> {
        let mut store = lock(&self.inner.sessions);
        if store.active >= self.inner.session_cap {
            return Err(SessionLimitReached);
        }
        store.active += 1;
        Ok(())
    }

    pub fn release_activation(&self) {
        let mut store = lock(&self.inner.sessions);
        store.active = store.active.saturating_sub(1);
    }

    pub fn delete_session(&self, id: &str) -> Result<(), DeleteSessionFailure> {
        let mut store = lock(&self.inner.sessions);
        let Some(entry) = store.entries.get(id).cloned() else {
            return Err(DeleteSessionFailure::NotFound);
        };
        if entry.is_active() {
            return Err(DeleteSessionFailure::Active);
        }
        store.entries.remove(id);
        store.order.retain(|existing| existing != id);
        Ok(())
    }

    pub fn output_budget(&self) -> Arc<OutputBudget> {
        Arc::clone(&self.inner.output_budget)
    }

    pub fn try_acquire_output_sse(&self) -> Option<SsePermit> {
        self.inner.output_sse.acquire()
    }

    pub fn try_acquire_events_sse(&self) -> Option<SsePermit> {
        self.inner.events_sse.acquire()
    }

    pub async fn health_checks(&self) -> Vec<AgentCheck> {
        let mut cache = self.inner.health.lock().await;
        if let Some(cached) = cache.as_ref()
            && cached.checked_at.elapsed() < HEALTH_CACHE_TTL
        {
            return cached.agents.clone();
        }

        let agents = tokio::task::spawn_blocking(clients::check_all)
            .await
            .unwrap_or_else(|_| clients::missing_checks());
        *cache = Some(CachedHealth {
            checked_at: Instant::now(),
            agents: agents.clone(),
        });
        agents
    }

    pub fn event_sender(&self) -> broadcast::Sender<SessionEvent> {
        self.inner.events.clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SessionEvent> {
        self.inner.events.subscribe()
    }

    pub fn emit(&self, event: SessionEvent) {
        let _ = self.inner.events.send(event);
    }

    pub fn command_override(&self) -> Option<&[String]> {
        self.inner.command_override.as_deref()
    }

    pub fn agent_bin(&self) -> Option<&Path> {
        self.inner.agent_bin.as_deref()
    }

    pub fn stall_writer_for_tests(&self) -> bool {
        self.inner.stall_writer
    }

    pub fn shutdown_all(&self) -> bool {
        const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

        let state = self.clone();
        let (finished, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut closed_any = false;
            for entry in state.entries_in_order() {
                let was_active = entry.is_active();
                if was_active && state.close_session(&entry).is_ok() {
                    entry.mark_restarted();
                    closed_any = true;
                }
            }
            if closed_any {
                std::thread::sleep(sessions::GROUP_KILL_GRACE + Duration::from_millis(50));
            }
            state.save();
            let _ = finished.send(());
        });
        receiver.recv_timeout(SHUTDOWN_TIMEOUT).is_ok()
    }

    fn entries_in_order(&self) -> Vec<Arc<SessionEntry>> {
        let store = lock(&self.inner.sessions);
        store
            .order
            .iter()
            .filter_map(|id| store.entries.get(id).cloned())
            .collect()
    }
}

fn unique_short(
    entries: &HashMap<String, Arc<SessionEntry>>,
    project_id: &str,
    base: &str,
) -> String {
    let existing = entries
        .values()
        .map(|entry| {
            let session = entry.session();
            (session.project_id, session.short)
        })
        .collect::<HashSet<_>>();

    with_suffix(base, |candidate| {
        existing.contains(&(project_id.to_owned(), candidate.to_owned()))
    })
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
