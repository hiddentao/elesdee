use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, broadcast};

use crate::browser::BrowserManager;
use crate::clients;
use crate::demo::{DemoFailure, DemoServer, clean_name, port_is_allowed};
use crate::persist::{LoadResult, PersistedOutput, PersistedSession, PersistedState, StateStore};
use crate::secrets::{KeyringStore, MemorySecretStore, SecretStore};
use crate::seed;
use crate::sessions::{self, CloseFailure, OutputBudget, SessionEntry, TerminalSize};
use crate::shared::SharedFile;
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
    #[serde(default)]
    pub config_id: Option<String>,
    #[serde(default)]
    pub allowed_config_ids: Vec<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default, rename = "accountId", skip_serializing)]
    pub legacy_account_id: Option<String>,
    #[serde(default, rename = "allowedAccountIds", skip_serializing)]
    pub legacy_allowed_account_ids: Vec<String>,
    #[serde(default, rename = "presetId", skip_serializing)]
    pub legacy_preset_id: Option<String>,
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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub agent_session_id: Option<String>,
    #[serde(default)]
    pub handoff_note: Option<String>,
    /// Percentage of the agent's context window that is still free. It comes
    /// from the running client and stays `None` when that client reports no
    /// figure.
    #[serde(default)]
    pub context_left: Option<u8>,
    /// Demo servers that this session registered for the phone to open.
    #[serde(default)]
    pub demo_servers: Vec<DemoServer>,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationKind {
    Message,
    Done,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub message: String,
    pub kind: NotificationKind,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub endpoint_id: String,
    pub name: String,
    pub platform: String,
    pub paired_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
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
    pub config_id: String,
    pub provider_id: String,
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
    #[serde(rename = "sharedfile.created")]
    SharedFile { file: SharedFile },
    #[serde(rename = "provider.updated")]
    Provider { provider: ProviderView },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder: String,
    #[serde(default)]
    pub default_config_id: Option<String>,
    #[serde(default)]
    pub default_source_branch: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageStatus {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
}

/// A credential and the endpoint it connects to. The key or subscription token
/// lives in the system keychain under the provider id and never appears here.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub auth: Auth,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub api_style: Option<String>,
    #[serde(default)]
    pub sample: bool,
    pub plan: String,
    pub rows: Vec<UsageRow>,
    pub foot: String,
    #[serde(default)]
    pub usage_read_at: Option<String>,
    #[serde(default)]
    pub usage_checked_at: Option<String>,
    #[serde(default)]
    pub usage_status: Option<UsageStatus>,
    #[serde(default)]
    pub usage_source: Option<String>,
}

/// A provider with the clients it can run. The client list is computed from the
/// vendor and the API style, so it is returned with the record but never stored.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: Provider,
    pub supported_clients: Vec<Client>,
}

impl ProviderView {
    pub fn new(provider: Provider) -> Self {
        let supported_clients = crate::providers::supported_clients(&provider);
        Self {
            provider,
            supported_clients,
        }
    }
}

/// What a session runs and what the user switches between. Several configs may
/// share one provider.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub client: Client,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<EnvOverride>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The account record from a state file written before providers and configs.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAccount {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub client: Client,
    pub provider: String,
    pub auth: Auth,
    #[serde(default)]
    pub sample: bool,
    #[serde(default)]
    pub rows: Vec<UsageRow>,
    #[serde(default)]
    pub foot: String,
    #[serde(default)]
    pub usage_read_at: Option<String>,
    #[serde(default)]
    pub usage_checked_at: Option<String>,
    #[serde(default)]
    pub usage_status: Option<UsageStatus>,
    #[serde(default)]
    pub usage_source: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_style: Option<String>,
}

/// The preset record from a state file written before AI configs.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPreset {
    pub id: String,
    pub name: String,
    pub account_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub env: Vec<EnvOverride>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

pub struct LegacyMigration {
    pub providers: Vec<Provider>,
    pub configs: Vec<AiConfig>,
    pub account_configs: HashMap<String, String>,
    pub preset_configs: HashMap<String, String>,
}

/// Turns the old accounts and presets into providers and AI configs.
///
/// Each account becomes a provider and one config named after it and its
/// client. Each preset becomes a config on its account's provider. Ids stay
/// stable when they can, so a session can map its old account and preset.
pub fn migrate_legacy(accounts: Vec<LegacyAccount>, presets: Vec<LegacyPreset>) -> LegacyMigration {
    let mut providers = Vec::with_capacity(accounts.len());
    let mut configs = Vec::with_capacity(accounts.len() + presets.len());
    let mut account_clients = HashMap::new();
    let mut account_configs = HashMap::new();
    let mut used = HashSet::new();

    for account in accounts {
        account_clients.insert(account.id.clone(), account.client);
        providers.push(Provider {
            id: account.id.clone(),
            name: account.name.clone(),
            vendor: account.provider,
            auth: account.auth,
            server: account.server,
            api_style: account.api_style,
            sample: account.sample,
            plan: account.plan,
            rows: account.rows,
            foot: account.foot,
            usage_read_at: account.usage_read_at,
            usage_checked_at: account.usage_checked_at,
            usage_status: account.usage_status,
            usage_source: account.usage_source,
        });
        let id = with_suffix(&account.id, |candidate| used.contains(candidate));
        used.insert(id.clone());
        account_configs.insert(account.id.clone(), id.clone());
        let now = timestamp();
        configs.push(AiConfig {
            id,
            name: format!("{} ({})", account.name, account.client),
            provider_id: account.id,
            client: account.client,
            model: account.model,
            effort: None,
            args: Vec::new(),
            env: Vec::new(),
            config: BTreeMap::new(),
            created_at: now.clone(),
            updated_at: now,
        });
    }

    let mut preset_configs = HashMap::new();
    for preset in presets {
        let Some(client) = account_clients.get(&preset.account_id).copied() else {
            continue;
        };
        let id = with_suffix(&preset.id, |candidate| used.contains(candidate));
        used.insert(id.clone());
        preset_configs.insert(preset.id.clone(), id.clone());
        let now = timestamp();
        configs.push(AiConfig {
            id,
            name: preset.name,
            provider_id: preset.account_id,
            client,
            model: preset.model,
            effort: None,
            args: Vec::new(),
            env: preset.env,
            config: preset.config,
            created_at: if preset.created_at.is_empty() {
                now.clone()
            } else {
                preset.created_at
            },
            updated_at: if preset.updated_at.is_empty() {
                now
            } else {
                preset.updated_at
            },
        });
    }

    LegacyMigration {
        providers,
        configs,
        account_configs,
        preset_configs,
    }
}

pub struct LegacySessionMapping {
    pub account_configs: HashMap<String, String>,
    pub preset_configs: HashMap<String, String>,
}

/// Maps a session written before AI configs onto the migrated records.
///
/// A session keeps its place when its preset or account maps to a config. When
/// neither does, the session is kept and its preview says why.
pub fn migrate_session_config(
    session: &mut Session,
    mapping: Option<&LegacySessionMapping>,
    configs: &[AiConfig],
) {
    if session.config_id.is_some() {
        return;
    }
    let Some(mapping) = mapping else {
        return;
    };
    let mapped = session
        .legacy_preset_id
        .as_ref()
        .and_then(|id| mapping.preset_configs.get(id))
        .or_else(|| {
            session
                .legacy_account_id
                .as_ref()
                .and_then(|id| mapping.account_configs.get(id))
        })
        .cloned();
    let had_legacy = session.legacy_preset_id.is_some() || session.legacy_account_id.is_some();

    if !session.legacy_allowed_account_ids.is_empty() {
        let mut allowed = Vec::new();
        for account_id in &session.legacy_allowed_account_ids {
            if let Some(config_id) = mapping.account_configs.get(account_id)
                && !allowed.iter().any(|id| id == config_id)
            {
                allowed.push(config_id.clone());
            }
        }
        session.allowed_config_ids = allowed;
    }

    match mapped {
        Some(config_id) => {
            let Some(config) = configs.iter().find(|config| config.id == config_id) else {
                session.preview =
                    "Migration: the saved config is missing. Choose an AI config.".to_owned();
                return;
            };
            session.config_id = Some(config.id.clone());
            session.provider_id = Some(config.provider_id.clone());
            session.client = Some(config.client);
            if session.model.is_none() {
                session.model = config.model.clone();
            }
            if session.effort.is_none() {
                session.effort = config.effort.clone();
            }
            if session.args.is_empty() {
                session.args = config.args.clone();
            }
            if session.config.is_empty() {
                session.config = config.config.clone();
            }
            if !session.allowed_config_ids.iter().any(|id| id == &config.id) {
                session.allowed_config_ids.push(config.id.clone());
            }
        }
        None if had_legacy => {
            session.preview =
                "Migration: the saved account or preset is missing. Choose an AI config."
                    .to_owned();
        }
        None => {}
    }
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
    pub providers: Vec<ProviderView>,
    pub configs: Vec<AiConfig>,
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
pub struct PairedDeviceLimit;

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

struct InitialRecords {
    projects: Vec<Project>,
    providers: Vec<Provider>,
    configs: Vec<AiConfig>,
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
    providers: Mutex<Vec<Provider>>,
    configs: Mutex<Vec<AiConfig>>,
    secrets: Arc<dyn SecretStore>,
    sessions: Mutex<SessionStore>,
    events: broadcast::Sender<SessionEvent>,
    questions: Mutex<Vec<Question>>,
    session_requests: Mutex<Vec<SessionRequest>>,
    paired_devices: Mutex<Vec<PairedDevice>>,
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
    demo_lock: Mutex<()>,
    usage_gates: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    usage_last_attempt: Mutex<HashMap<String, Instant>>,
    usage_last_save: Mutex<HashMap<String, Instant>>,
    usage_base_override: Option<String>,
    model_cache: Mutex<HashMap<String, CachedModels>>,
    model_gates: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    link: crate::link::LinkHub,
    browser: BrowserManager,
}

#[derive(Clone, Debug)]
pub struct CachedModels {
    pub read_at: Instant,
    pub models: Vec<crate::model_list::ModelChoice>,
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
            false,
        );
        if let Some(inner) = Arc::get_mut(&mut state.inner) {
            inner.port = port;
            inner.agent_bin = env::var_os("ELESDEE_AGENT_BIN")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
        }
        state.recover_after_load()
    }

    #[doc(hidden)]
    pub fn ephemeral() -> Self {
        Self::from_parts(
            InitialRecords {
                projects: Vec::new(),
                providers: seed::providers(),
                configs: seed::configs(),
            },
            Arc::new(MemorySecretStore::default()),
            None,
            MAX_ACTIVE_SESSIONS,
            false,
            None,
            true,
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
            true,
        )
        .recover_after_load()
    }

    #[doc(hidden)]
    pub fn with_test_command(argv: Vec<String>) -> Self {
        Self::from_parts(
            InitialRecords {
                projects: Vec::new(),
                providers: seed::providers(),
                configs: seed::configs(),
            },
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            MAX_ACTIVE_SESSIONS,
            false,
            None,
            true,
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
            true,
        )
        .recover_after_load()
    }

    #[doc(hidden)]
    pub fn with_test_agent_bin_and_data_dir(
        binary: PathBuf,
        directory: impl Into<PathBuf>,
    ) -> Self {
        let store = match StateStore::new(directory.into()) {
            Ok(store) => Some(Arc::new(store)),
            Err(error) => {
                tracing::error!(%error, "could not prepare the test data directory");
                None
            }
        };
        let mut state = Self::from_store(
            store,
            Arc::new(MemorySecretStore::default()),
            None,
            MAX_ACTIVE_SESSIONS,
            false,
            true,
        );
        if let Some(inner) = Arc::get_mut(&mut state.inner) {
            inner.agent_bin = Some(binary);
        }
        state.recover_after_load()
    }

    #[doc(hidden)]
    pub fn with_test_command_and_session_cap(argv: Vec<String>, session_cap: usize) -> Self {
        Self::from_parts(
            InitialRecords {
                projects: Vec::new(),
                providers: seed::providers(),
                configs: seed::configs(),
            },
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            session_cap,
            false,
            None,
            true,
        )
    }

    #[doc(hidden)]
    pub fn with_test_stalled_writer(argv: Vec<String>) -> Self {
        Self::from_parts(
            InitialRecords {
                projects: Vec::new(),
                providers: seed::providers(),
                configs: seed::configs(),
            },
            Arc::new(MemorySecretStore::default()),
            Some(argv),
            MAX_ACTIVE_SESSIONS,
            true,
            None,
            true,
        )
    }

    fn recover_after_load(self) -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            crate::recovery::recover(&self);
        }
        self
    }

    fn from_store(
        store: Option<Arc<StateStore>>,
        secrets: Arc<dyn SecretStore>,
        command_override: Option<Vec<String>>,
        session_cap: usize,
        stall_writer: bool,
        fake_browser: bool,
    ) -> Self {
        let (
            projects,
            providers,
            configs,
            migration,
            sessions,
            questions,
            session_requests,
            paired_devices,
        ) = match store.as_ref().map(|store| store.load()) {
            Some(LoadResult::Loaded(state)) => {
                let (providers, configs, migration) = match (state.providers, state.configs) {
                    (Some(providers), Some(configs)) => (providers, configs, None),
                    _ => {
                        let legacy = migrate_legacy(
                            state.accounts.unwrap_or_default(),
                            state.presets.unwrap_or_default(),
                        );
                        let mapping = LegacySessionMapping {
                            account_configs: legacy.account_configs.clone(),
                            preset_configs: legacy.preset_configs.clone(),
                        };
                        (legacy.providers, legacy.configs, Some(mapping))
                    }
                };
                (
                    state.projects,
                    providers,
                    configs,
                    migration,
                    state.sessions,
                    state.questions.unwrap_or_default(),
                    state.session_requests.unwrap_or_default(),
                    state.paired_devices,
                )
            }
            Some(LoadResult::Missing | LoadResult::Corrupt) | None => (
                Vec::new(),
                seed::providers(),
                seed::configs(),
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        };
        let state = Self::from_parts(
            InitialRecords {
                projects,
                providers,
                configs,
            },
            secrets,
            command_override,
            session_cap,
            stall_writer,
            store,
            fake_browser,
        );
        let migrated = migration.is_some();
        for mut session in sessions {
            if session.session.config_id.is_none() {
                migrate_session_config(&mut session.session, migration.as_ref(), &state.configs());
            }
            if session.session.allowed_config_ids.is_empty() {
                session.session.allowed_config_ids = crate::providers::default_allowed_configs(
                    &state.configs(),
                    &state.providers(),
                    session.session.client,
                    session.session.config_id.as_deref(),
                );
            }
            session.session.legacy_account_id = None;
            session.session.legacy_allowed_account_ids.clear();
            session.session.legacy_preset_id = None;
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
        *lock(&state.inner.paired_devices) = paired_devices;
        if let Some(directory) = state.data_dir() {
            crate::claude::sweep(directory);
            crate::mcp::sweep(directory);
        }
        state.inner.output_budget.enforce();
        if migrated {
            state.save();
        }
        state
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        records: InitialRecords,
        secrets: Arc<dyn SecretStore>,
        command_override: Option<Vec<String>>,
        session_cap: usize,
        stall_writer: bool,
        store: Option<Arc<StateStore>>,
        fake_browser: bool,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (aux_events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let browser_profiles = store
            .as_ref()
            .map(|store| store.directory().join("browsers"));
        Self {
            inner: Arc::new(Inner {
                projects: Mutex::new(records.projects),
                providers: Mutex::new(records.providers),
                configs: Mutex::new(records.configs),
                secrets,
                sessions: Mutex::new(SessionStore::new()),
                events,
                questions: Mutex::new(Vec::new()),
                session_requests: Mutex::new(Vec::new()),
                paired_devices: Mutex::new(Vec::new()),
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
                demo_lock: Mutex::new(()),
                usage_gates: Mutex::new(HashMap::new()),
                usage_last_attempt: Mutex::new(HashMap::new()),
                usage_last_save: Mutex::new(HashMap::new()),
                usage_base_override: None,
                model_cache: Mutex::new(HashMap::new()),
                model_gates: Mutex::new(HashMap::new()),
                link: crate::link::LinkHub::new(),
                browser: BrowserManager::new(browser_profiles, fake_browser),
            }),
        }
    }

    /// Returns the shared remote-link state.
    pub fn link(&self) -> &crate::link::LinkHub {
        &self.inner.link
    }

    /// Returns the per-session browser manager.
    pub fn browser(&self) -> &BrowserManager {
        &self.inner.browser
    }

    /// Stops one session's browser and closes its tabs. The profile stays.
    pub async fn stop_browser(&self, session_id: &str) {
        self.inner.browser.stop(session_id).await;
    }

    /// Stops every session browser at shutdown.
    pub async fn stop_all_browsers(&self) {
        self.inner.browser.stop_all().await;
    }

    pub fn paired_devices(&self) -> Vec<PairedDevice> {
        lock(&self.inner.paired_devices).clone()
    }

    pub fn paired_device_count(&self) -> usize {
        lock(&self.inner.paired_devices).len()
    }

    pub fn is_paired_device(&self, endpoint_id: &str) -> bool {
        lock(&self.inner.paired_devices)
            .iter()
            .any(|device| device.endpoint_id == endpoint_id)
    }

    /// Adds a paired phone, or updates the record for a phone that pairs again.
    ///
    /// # Errors
    ///
    /// Returns [`PairedDeviceLimit`] when the list is full and the phone is new.
    pub fn add_paired_device(
        &self,
        device: PairedDevice,
    ) -> Result<PairedDevice, PairedDeviceLimit> {
        let mut devices = lock(&self.inner.paired_devices);
        if let Some(existing) = devices
            .iter_mut()
            .find(|existing| existing.endpoint_id == device.endpoint_id)
        {
            *existing = device.clone();
            return Ok(device);
        }
        if devices.len() >= elesdee_link::MAX_PAIRED_PHONES {
            return Err(PairedDeviceLimit);
        }
        devices.push(device.clone());
        Ok(device)
    }

    pub fn remove_paired_device(&self, endpoint_id: &str) -> bool {
        let mut devices = lock(&self.inner.paired_devices);
        let before = devices.len();
        devices.retain(|device| device.endpoint_id != endpoint_id);
        before != devices.len()
    }

    /// Records that a paired phone connected, at most once a minute.
    pub fn touch_paired_device(&self, endpoint_id: &str) {
        let now = Utc::now();
        let mut devices = lock(&self.inner.paired_devices);
        let Some(device) = devices
            .iter_mut()
            .find(|device| device.endpoint_id == endpoint_id)
        else {
            return;
        };
        let stale = device
            .last_seen
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|seen| (now - seen.with_timezone(&Utc)).num_seconds() >= 60);
        if stale {
            device.last_seen = Some(timestamp());
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

    pub fn providers(&self) -> Vec<Provider> {
        lock(&self.inner.providers).clone()
    }

    pub fn provider_views(&self) -> Vec<ProviderView> {
        self.providers()
            .into_iter()
            .map(ProviderView::new)
            .collect()
    }

    pub fn provider(&self, id: &str) -> Option<Provider> {
        lock(&self.inner.providers)
            .iter()
            .find(|provider| provider.id == id)
            .cloned()
    }

    pub fn add_provider(&self, mut provider: Provider) -> Provider {
        let mut providers = lock(&self.inner.providers);
        let base = slugify(&provider.name, "provider");
        let id = with_suffix(&base, |candidate| {
            providers.iter().any(|existing| existing.id == candidate)
        });
        provider.id = id;
        providers.push(provider.clone());
        provider
    }

    /// Replaces the provider with the same id and returns the stored record.
    pub fn replace_provider(&self, provider: Provider) -> Option<Provider> {
        let mut providers = lock(&self.inner.providers);
        let slot = providers
            .iter_mut()
            .find(|existing| existing.id == provider.id)?;
        *slot = provider.clone();
        Some(provider)
    }

    pub fn remove_provider(&self, id: &str) -> Option<Provider> {
        let removed = {
            let mut providers = lock(&self.inner.providers);
            let position = providers.iter().position(|provider| provider.id == id)?;
            providers.remove(position)
        };
        lock(&self.inner.usage_gates).remove(&removed.id);
        lock(&self.inner.usage_last_attempt).remove(&removed.id);
        lock(&self.inner.usage_last_save).remove(&removed.id);
        Some(removed)
    }

    pub fn config_count_for_provider(&self, provider_id: &str) -> usize {
        lock(&self.inner.configs)
            .iter()
            .filter(|config| config.provider_id == provider_id)
            .count()
    }

    pub fn set_provider_usage(
        &self,
        id: &str,
        rows: Vec<UsageRow>,
        source: &str,
        read_at: &str,
        checked_at: &str,
    ) -> Option<Provider> {
        let mut providers = lock(&self.inner.providers);
        let provider = providers.iter_mut().find(|provider| provider.id == id)?;
        provider.rows = rows;
        provider.usage_source = Some(source.to_owned());
        provider.usage_status = Some(UsageStatus::Fresh);
        provider.usage_read_at = Some(read_at.to_owned());
        provider.usage_checked_at = Some(checked_at.to_owned());
        provider.foot.clear();
        Some(provider.clone())
    }

    pub fn mark_provider_usage_failed(
        &self,
        id: &str,
        source: &str,
        checked_at: &str,
    ) -> Option<Provider> {
        let mut providers = lock(&self.inner.providers);
        let provider = providers.iter_mut().find(|provider| provider.id == id)?;
        provider.usage_source = Some(source.to_owned());
        provider.usage_status = Some(if provider.rows.is_empty() {
            UsageStatus::Unknown
        } else {
            UsageStatus::Stale
        });
        provider.usage_checked_at = Some(checked_at.to_owned());
        provider.foot.clear();
        Some(provider.clone())
    }

    /// Returns the gate that serializes reads for one provider.
    pub fn usage_gate(&self, account_id: &str) -> Arc<AsyncMutex<()>> {
        lock(&self.inner.usage_gates)
            .entry(account_id.to_owned())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    pub fn usage_read_due(&self, account_id: &str, minimum: Duration) -> bool {
        lock(&self.inner.usage_last_attempt)
            .get(account_id)
            .is_none_or(|attempt| attempt.elapsed() >= minimum)
    }

    pub fn note_usage_attempt(&self, account_id: &str) {
        lock(&self.inner.usage_last_attempt).insert(account_id.to_owned(), Instant::now());
    }

    pub fn usage_save_due(&self, account_id: &str, minimum: Duration) -> bool {
        lock(&self.inner.usage_last_save)
            .get(account_id)
            .is_none_or(|saved| saved.elapsed() >= minimum)
    }

    pub fn note_usage_saved(&self, account_id: &str) {
        lock(&self.inner.usage_last_save).insert(account_id.to_owned(), Instant::now());
    }

    pub fn publish_provider(&self, provider: Provider) {
        self.emit_aux(AuxEvent::Provider {
            provider: ProviderView::new(provider),
        });
    }

    #[doc(hidden)]
    pub fn with_test_usage_base(mut self, base: String) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.usage_base_override = Some(base);
        }
        self
    }

    pub fn usage_base_override(&self) -> Option<&str> {
        self.inner.usage_base_override.as_deref()
    }

    pub fn cached_models(
        &self,
        key: &str,
        minimum: Duration,
    ) -> Option<Vec<crate::model_list::ModelChoice>> {
        let cache = lock(&self.inner.model_cache);
        let entry = cache.get(key)?;
        (entry.read_at.elapsed() < minimum).then(|| entry.models.clone())
    }

    pub fn cache_models(&self, key: &str, models: Vec<crate::model_list::ModelChoice>) {
        lock(&self.inner.model_cache).insert(
            key.to_owned(),
            CachedModels {
                read_at: Instant::now(),
                models,
            },
        );
    }

    /// Returns the gate that coalesces model reads for one provider and client.
    pub fn model_gate(&self, key: &str) -> Arc<AsyncMutex<()>> {
        lock(&self.inner.model_gates)
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    pub fn configs(&self) -> Vec<AiConfig> {
        lock(&self.inner.configs).clone()
    }

    pub fn config(&self, id: &str) -> Option<AiConfig> {
        lock(&self.inner.configs)
            .iter()
            .find(|config| config.id == id)
            .cloned()
    }

    /// A config plus the provider it runs on, or `None` when either is missing.
    pub fn config_with_provider(&self, id: &str) -> Option<(AiConfig, Provider)> {
        let config = self.config(id)?;
        let provider = self.provider(&config.provider_id)?;
        Some((config, provider))
    }

    pub fn add_config(&self, mut config: AiConfig) -> AiConfig {
        let now = timestamp();
        let mut configs = lock(&self.inner.configs);
        let base = slugify(&config.name, "config");
        let id = with_suffix(&base, |candidate| {
            configs.iter().any(|existing| existing.id == candidate)
        });
        config.id = id;
        config.created_at = now.clone();
        config.updated_at = now;
        configs.push(config.clone());
        config
    }

    /// Replaces the config with the same id and returns the stored record.
    pub fn replace_config(&self, config: AiConfig) -> Option<AiConfig> {
        let mut configs = lock(&self.inner.configs);
        let slot = configs
            .iter_mut()
            .find(|existing| existing.id == config.id)?;
        let mut updated = config;
        updated.created_at = slot.created_at.clone();
        updated.updated_at = timestamp();
        *slot = updated.clone();
        Some(updated)
    }

    /// Removes a config and clears every project default and session reference.
    ///
    /// The caller refuses the removal while an active session runs the config.
    pub fn remove_config(&self, id: &str) -> Option<AiConfig> {
        let removed = {
            let mut configs = lock(&self.inner.configs);
            let position = configs.iter().position(|config| config.id == id)?;
            configs.remove(position)
        };
        {
            let mut projects = lock(&self.inner.projects);
            for project in projects.iter_mut() {
                if project.default_config_id.as_deref() == Some(id) {
                    project.default_config_id = None;
                }
            }
        }
        for entry in self.sessions() {
            if entry.forget_config(id) {
                self.emit(SessionEvent::updated(entry.session()));
            }
        }
        Some(removed)
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
        self.emit_notification(session_id, message, NotificationKind::Message)
    }

    pub fn notify_done(&self, session_id: &str) -> Option<Notification> {
        self.emit_notification(
            session_id,
            "Reported done.".to_owned(),
            NotificationKind::Done,
        )
    }

    fn emit_notification(
        &self,
        session_id: &str,
        message: String,
        kind: NotificationKind,
    ) -> Option<Notification> {
        let entry = self.session(session_id)?;
        let session = entry.session();
        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            session_name: session.name.clone(),
            message,
            kind,
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

    pub fn emit_shared_file(&self, file: SharedFile) {
        self.emit_aux(AuxEvent::SharedFile { file });
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
            default_config_id: None,
            default_source_branch: None,
        };
        projects.push(project.clone());
        Ok(project)
    }

    pub fn remove_project(&self, id: &str) {
        lock(&self.inner.projects).retain(|project| project.id != id);
    }

    pub fn update_project_defaults(
        &self,
        id: &str,
        default_config_id: Option<Option<String>>,
        default_source_branch: Option<Option<String>>,
    ) -> Option<Project> {
        let mut projects = lock(&self.inner.projects);
        let project = projects.iter_mut().find(|project| project.id == id)?;
        if let Some(value) = default_config_id {
            project.default_config_id = value;
        }
        if let Some(value) = default_source_branch {
            project.default_source_branch = value;
        }
        Some(project.clone())
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.inner.store.as_ref().map(|store| store.directory())
    }

    pub fn session(&self, id: &str) -> Option<Arc<SessionEntry>> {
        let store = lock(&self.inner.sessions);
        store.entries.get(id).cloned()
    }

    /// Registers a demo server for one session.
    ///
    /// A port belongs to at most one active session at a time. The service's
    /// own port and privileged ports are refused.
    pub fn register_demo_server(
        &self,
        session_id: &str,
        name: &str,
        port: u16,
    ) -> Result<(Session, DemoServer), DemoFailure> {
        let _guard = lock(&self.inner.demo_lock);
        let entry = self
            .session(session_id)
            .ok_or(DemoFailure::UnknownSession)?;
        if entry.session().status != SessionState::Active {
            return Err(DemoFailure::Closed);
        }
        let name = clean_name(name).ok_or(DemoFailure::InvalidName)?;
        if !port_is_allowed(port) {
            return Err(DemoFailure::InvalidPort);
        }
        if port == self.inner.port {
            return Err(DemoFailure::ReservedPort);
        }
        let in_use = self.sessions().iter().any(|other| {
            if other.id() == session_id {
                return false;
            }
            let session = other.session();
            session.status == SessionState::Active
                && session
                    .demo_servers
                    .iter()
                    .any(|server| server.port == port && server.is_active())
        });
        if in_use {
            return Err(DemoFailure::PortInUse);
        }
        let session = entry.register_demo_server(name, port)?;
        let server = session
            .demo_servers
            .iter()
            .find(|server| server.port == port && server.is_active())
            .cloned()
            .ok_or(DemoFailure::NotFound)?;
        Ok((session, server))
    }

    /// Stops one demo server in a session and returns its port.
    pub fn stop_demo_server(
        &self,
        session_id: &str,
        server_id: &str,
    ) -> Result<(Session, u16), DemoFailure> {
        let entry = self
            .session(session_id)
            .ok_or(DemoFailure::UnknownSession)?;
        let port = entry
            .session()
            .demo_servers
            .iter()
            .find(|server| server.id == server_id)
            .map(|server| server.port)
            .ok_or(DemoFailure::NotFound)?;
        let session = entry.stop_demo_server(server_id)?;
        Ok((session, port))
    }

    /// Returns the active demo server on a port, from any session.
    pub fn demo_server_by_port(&self, port: u16) -> Option<DemoServer> {
        self.sessions().iter().find_map(|entry| {
            let session = entry.session();
            if session.status != SessionState::Active {
                return None;
            }
            session
                .demo_servers
                .iter()
                .find(|server| server.port == port && server.is_active())
                .cloned()
        })
    }

    /// Returns every active demo server with its session name.
    pub fn active_demo_servers(&self) -> Vec<(String, DemoServer)> {
        self.sessions()
            .iter()
            .flat_map(|entry| {
                let session = entry.session();
                if session.status != SessionState::Active {
                    return Vec::new();
                }
                session
                    .demo_servers
                    .iter()
                    .filter(|server| server.is_active())
                    .map(|server| (session.name.clone(), server.clone()))
                    .collect()
            })
            .collect()
    }

    /// Closes the open tunnels for a session's demo servers.
    pub fn close_demo_tunnels(&self, session: &Session) {
        for server in &session.demo_servers {
            self.inner.link.close_forward(server.port);
        }
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
            providers: self.provider_views(),
            configs: self.configs(),
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
        if let Some(entry) = store.entries.remove(id)
            && entry.is_active()
        {
            store.active = store.active.saturating_sub(1);
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
        let active = entry.is_active();
        let mut store = lock(&self.inner.sessions);
        store.order.push(id.clone());
        store.entries.insert(id, entry);
        if active {
            store.active += 1;
        }
    }

    pub fn save(&self) {
        if let Err(error) = self.try_save() {
            tracing::error!(%error, "could not save state");
        }
    }

    pub fn try_save(&self) -> io::Result<()> {
        let Some(store) = &self.inner.store else {
            return Ok(());
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
            Some(self.providers()),
            Some(self.configs()),
            Some(self.pending_questions()),
            Some(self.pending_session_requests()),
            self.paired_devices(),
            sessions,
        );
        store.save(&state)
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
            state.link().close_all_forwards();
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
