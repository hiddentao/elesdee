use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::private_fs;
use crate::state::{AppState, Project};

const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_LOGIN_BASE: &str = "https://github.com";
const TOKEN_ACCOUNT: &str = "github:oauth";
const USER_AGENT: &str = "elesdee";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const CLONE_TIMEOUT: Duration = Duration::from_secs(120);
const REPO_CACHE_TTL: Duration = Duration::from_secs(60);
const ACCOUNT_CACHE_TTL: Duration = Duration::from_secs(300);
const ADDED_CACHE_TTL: Duration = Duration::from_secs(10);
const DEVICE_RETENTION: Duration = Duration::from_secs(15 * 60);
const REPO_PAGE_SIZE: usize = 100;
const REPO_MAX_PAGES: usize = 3;
const RECENT_LIMIT: usize = 50;
const SEARCH_LIMIT: usize = 50;
const NETWORK_RETRIES: u32 = 3;
const GITHUB_API_VERSION: &str = "2022-11-28";

/// The per-process GitHub settings and caches.
pub(crate) struct GitHubInner {
    api_base: String,
    login_base: String,
    client_id: Option<String>,
    default_parent: Option<PathBuf>,
    allow_insecure_urls: bool,
    devices: Mutex<HashMap<String, DeviceEntry>>,
    repos: Mutex<Option<ReposCache>>,
    account: Mutex<Option<CachedAccount>>,
    added: Mutex<Option<CachedAdded>>,
    clone_lock: AsyncMutex<()>,
}

struct DeviceEntry {
    view: GitHubDeviceView,
    device_code: String,
    cancel: bool,
    created: Instant,
}

struct ReposCache {
    read_at: Instant,
    repos: Vec<GitHubRepo>,
}

struct CachedAccount {
    read_at: Instant,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

struct CachedAdded {
    read_at: Instant,
    remotes: HashSet<String>,
}

impl GitHubInner {
    pub(crate) fn from_environment() -> Self {
        let api_base =
            configured("ELESDEE_GITHUB_API_BASE").unwrap_or_else(|| DEFAULT_API_BASE.to_owned());
        let login_base = configured("ELESDEE_GITHUB_LOGIN_BASE")
            .unwrap_or_else(|| DEFAULT_LOGIN_BASE.to_owned());
        let client_id = configured("ELESDEE_GITHUB_CLIENT_ID")
            .or_else(|| option_env!("ELESDEE_GITHUB_CLIENT_ID").map(str::to_owned))
            .filter(|value| !value.is_empty());
        let default_parent = configured("ELESDEE_GITHUB_DEFAULT_PARENT").map(PathBuf::from);
        let allow_insecure_urls = configured("ELESDEE_GITHUB_API_BASE").is_some();
        Self {
            api_base,
            login_base,
            client_id,
            default_parent,
            allow_insecure_urls,
            devices: Mutex::new(HashMap::new()),
            repos: Mutex::new(None),
            account: Mutex::new(None),
            added: Mutex::new(None),
            clone_lock: AsyncMutex::new(()),
        }
    }

    #[doc(hidden)]
    pub(crate) fn for_test(
        api_base: String,
        login_base: String,
        client_id: String,
        default_parent: PathBuf,
    ) -> Self {
        Self {
            api_base,
            login_base,
            client_id: Some(client_id),
            default_parent: Some(default_parent),
            allow_insecure_urls: true,
            devices: Mutex::new(HashMap::new()),
            repos: Mutex::new(None),
            account: Mutex::new(None),
            added: Mutex::new(None),
            clone_lock: AsyncMutex::new(()),
        }
    }

    pub(crate) fn api_base(&self) -> &str {
        self.api_base.trim_end_matches('/')
    }

    pub(crate) fn login_base(&self) -> &str {
        self.login_base.trim_end_matches('/')
    }

    pub(crate) fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub(crate) fn default_parent(&self) -> PathBuf {
        match &self.default_parent {
            Some(path) => path.clone(),
            None => default_projects_folder(),
        }
    }

    fn insert_device(&self, view: GitHubDeviceView, device_code: String) {
        let now = Instant::now();
        let mut devices = lock(&self.devices);
        devices.retain(|_, entry| now.duration_since(entry.created) < DEVICE_RETENTION);
        devices.insert(
            view.id.clone(),
            DeviceEntry {
                view,
                device_code,
                cancel: false,
                created: now,
            },
        );
    }

    pub(crate) fn device_view(&self, id: &str) -> Option<GitHubDeviceView> {
        lock(&self.devices).get(id).map(|entry| entry.view.clone())
    }

    fn update_device(&self, id: &str, update: impl FnOnce(&mut GitHubDeviceView)) {
        if let Some(entry) = lock(&self.devices).get_mut(id) {
            update(&mut entry.view);
        }
    }

    fn device_code(&self, id: &str) -> Option<String> {
        lock(&self.devices)
            .get(id)
            .map(|entry| entry.device_code.clone())
    }

    fn device_interval(&self, id: &str) -> u64 {
        lock(&self.devices)
            .get(id)
            .map(|entry| entry.view.interval.max(1))
            .unwrap_or(5)
    }

    fn is_cancelled(&self, id: &str) -> bool {
        lock(&self.devices)
            .get(id)
            .is_some_and(|entry| entry.cancel)
    }

    pub(crate) fn cancel_device(&self, id: &str) -> bool {
        let mut devices = lock(&self.devices);
        let Some(entry) = devices.get_mut(id) else {
            return false;
        };
        entry.cancel = true;
        if !matches!(
            entry.view.state,
            GitHubDeviceState::Success | GitHubDeviceState::Failed
        ) {
            entry.view.state = GitHubDeviceState::Cancelled;
        }
        true
    }

    fn cancel_all_devices(&self) {
        for entry in lock(&self.devices).values_mut() {
            entry.cancel = true;
            if !matches!(
                entry.view.state,
                GitHubDeviceState::Success | GitHubDeviceState::Failed
            ) {
                entry.view.state = GitHubDeviceState::Cancelled;
            }
        }
    }

    fn cached_repos(&self) -> Option<Vec<GitHubRepo>> {
        let cache = lock(&self.repos);
        let cache = cache.as_ref()?;
        (cache.read_at.elapsed() < REPO_CACHE_TTL).then(|| cache.repos.clone())
    }

    fn store_repos(&self, repos: Vec<GitHubRepo>) {
        *lock(&self.repos) = Some(ReposCache {
            read_at: Instant::now(),
            repos,
        });
    }

    fn clear_repos(&self) {
        *lock(&self.repos) = None;
    }

    fn cached_account(&self) -> Option<(String, Option<String>, Option<String>)> {
        let cache = lock(&self.account);
        let cache = cache.as_ref()?;
        (cache.read_at.elapsed() < ACCOUNT_CACHE_TTL).then(|| {
            (
                cache.login.clone(),
                cache.name.clone(),
                cache.avatar_url.clone(),
            )
        })
    }

    fn store_account(&self, login: String, name: Option<String>, avatar_url: Option<String>) {
        *lock(&self.account) = Some(CachedAccount {
            read_at: Instant::now(),
            login,
            name,
            avatar_url,
        });
    }

    fn clear_account(&self) {
        *lock(&self.account) = None;
    }

    fn cached_added(&self) -> Option<HashSet<String>> {
        let cache = lock(&self.added);
        let cache = cache.as_ref()?;
        (cache.read_at.elapsed() < ADDED_CACHE_TTL).then(|| cache.remotes.clone())
    }

    fn store_added(&self, remotes: HashSet<String>) {
        *lock(&self.added) = Some(CachedAdded {
            read_at: Instant::now(),
            remotes,
        });
    }

    fn clear_caches(&self) {
        self.clear_repos();
        self.clear_account();
        *lock(&self.added) = None;
    }

    async fn clone_gate(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.clone_lock.lock().await
    }
}

fn configured(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn default_projects_folder() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Projects")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GitHubDeviceState {
    Starting,
    Waiting,
    Success,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GitHubDeviceReason {
    NotConfigured,
    Network,
    Denied,
    Expired,
    Keychain,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDeviceView {
    pub id: String,
    pub state: GitHubDeviceState,
    pub user_code: Option<String>,
    pub verification_uri: Option<String>,
    pub interval: u64,
    pub expires_in: u64,
    pub login: Option<String>,
    pub reason: Option<GitHubDeviceReason>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubStatus {
    pub connected: bool,
    pub login: Option<String>,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub default_clone_folder: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepo {
    pub id: u64,
    pub full_name: String,
    pub name: String,
    pub owner: String,
    pub private: bool,
    pub clone_url: String,
    pub html_url: String,
    pub description: Option<String>,
    pub updated_at: Option<String>,
    pub added: bool,
}

#[derive(Debug)]
pub enum GitHubError {
    NotConfigured,
    NotConnected,
    Unauthorized,
    Upstream,
    NotFound,
    Conflict(String),
    Invalid(String),
    Failed,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    interval: u64,
}

#[derive(Debug, Default, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawUser {
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRepoOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct RawRepo {
    id: u64,
    full_name: String,
    name: String,
    owner: RawRepoOwner,
    #[serde(default)]
    private: bool,
    clone_url: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

fn http_client() -> Result<reqwest::Client, GitHubError> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|_| GitHubError::Upstream)
}

async fn read_token(state: &AppState) -> Result<Option<String>, GitHubError> {
    let store = state.secret_store();
    match tokio::task::spawn_blocking(move || store.get_key(TOKEN_ACCOUNT)).await {
        Ok(Ok(Some(token))) if !token.is_empty() => Ok(Some(token)),
        Ok(Ok(_)) => Ok(None),
        Ok(Err(_)) | Err(_) => Err(GitHubError::Failed),
    }
}

async fn write_token(state: &AppState, token: &str) -> Result<(), GitHubError> {
    let store = state.secret_store();
    let token = token.to_owned();
    match tokio::task::spawn_blocking(move || store.set_key(TOKEN_ACCOUNT, &token)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(GitHubError::Failed),
    }
}

async fn delete_token(state: &AppState) -> Result<(), GitHubError> {
    let store = state.secret_store();
    match tokio::task::spawn_blocking(move || store.delete_key(TOKEN_ACCOUNT)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(GitHubError::Failed),
    }
}

async fn required_token(state: &AppState) -> Result<String, GitHubError> {
    read_token(state).await?.ok_or(GitHubError::NotConnected)
}

/// Starts the GitHub device flow. The service fetches the one-time code and
/// polls GitHub in the background; the caller reads the view until it ends.
pub async fn start_device(state: &AppState) -> Result<GitHubDeviceView, GitHubError> {
    let client_id = state
        .github()
        .client_id()
        .map(str::to_owned)
        .ok_or(GitHubError::NotConfigured)?;
    let client = http_client()?;
    let url = format!("{}/login/device/code", state.github().login_base());
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str()), ("scope", "repo")])
        .send()
        .await
        .map_err(|_| GitHubError::Upstream)?;
    if !response.status().is_success() {
        return Err(GitHubError::Upstream);
    }
    let code = response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|_| GitHubError::Upstream)?;
    let id = Uuid::new_v4().to_string();
    let view = GitHubDeviceView {
        id: id.clone(),
        state: GitHubDeviceState::Waiting,
        user_code: Some(code.user_code),
        verification_uri: Some(code.verification_uri),
        interval: code.interval.max(1),
        expires_in: code.expires_in,
        login: None,
        reason: None,
    };
    state.github().insert_device(view.clone(), code.device_code);
    tokio::spawn(poll_device(state.clone(), id, client_id, code.expires_in));
    Ok(view)
}

async fn poll_device(state: AppState, id: String, client_id: String, expires_in: u64) {
    let Some(device_code) = state.github().device_code(&id) else {
        return;
    };
    let settings = state.github().api_base().to_owned();
    let login_base = state.github().login_base().to_owned();
    let client = match http_client() {
        Ok(client) => client,
        Err(_) => {
            fail_device(&state, &id, GitHubDeviceReason::Network);
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(expires_in.clamp(1, 3600));
    let mut interval = state.github().device_interval(&id);
    let mut network_failures = 0_u32;

    loop {
        if state.github().is_cancelled(&id) {
            return;
        }
        if Instant::now() >= deadline {
            fail_device(&state, &id, GitHubDeviceReason::Expired);
            return;
        }

        let url = format!("{login_base}/login/oauth/access_token");
        let result = client
            .post(url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await;

        match result {
            Ok(response) if response.status().is_success() => {
                let token = match response.json::<TokenResponse>().await {
                    Ok(token) => token,
                    Err(_) => {
                        network_failures += 1;
                        if network_failures >= NETWORK_RETRIES {
                            fail_device(&state, &id, GitHubDeviceReason::Network);
                            return;
                        }
                        sleep_seconds(interval).await;
                        continue;
                    }
                };
                network_failures = 0;
                if let Some(access) = token.access_token {
                    if state.github().is_cancelled(&id) {
                        return;
                    }
                    if write_token(&state, &access).await.is_err() {
                        fail_device(&state, &id, GitHubDeviceReason::Keychain);
                        return;
                    }
                    state.github().clear_caches();
                    let account = fetch_user(&client, &settings, &access).await.ok();
                    if let Some(account) = &account {
                        state.github().store_account(
                            account.login.clone(),
                            account.name.clone(),
                            account.avatar_url.clone(),
                        );
                    }
                    state.github().update_device(&id, |view| {
                        view.state = GitHubDeviceState::Success;
                        view.login = account.map(|account| account.login);
                    });
                    return;
                }

                match token.error.as_deref() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => {
                        interval = token.interval.unwrap_or(interval + 5).max(interval + 5);
                    }
                    Some("expired_token") => {
                        fail_device(&state, &id, GitHubDeviceReason::Expired);
                        return;
                    }
                    Some("access_denied") => {
                        fail_device(&state, &id, GitHubDeviceReason::Denied);
                        return;
                    }
                    _ => {
                        fail_device(&state, &id, GitHubDeviceReason::Error);
                        return;
                    }
                }
            }
            Ok(_) => {
                fail_device(&state, &id, GitHubDeviceReason::Error);
                return;
            }
            Err(_) => {
                network_failures += 1;
                if network_failures >= NETWORK_RETRIES {
                    fail_device(&state, &id, GitHubDeviceReason::Network);
                    return;
                }
            }
        }

        sleep_seconds(interval).await;
    }
}

fn fail_device(state: &AppState, id: &str, reason: GitHubDeviceReason) {
    state.github().update_device(id, |view| {
        view.state = GitHubDeviceState::Failed;
        view.reason = Some(reason);
    });
}

async fn sleep_seconds(seconds: u64) {
    tokio::time::sleep(Duration::from_secs(seconds.max(1))).await;
}

async fn fetch_user(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
) -> Result<RawUser, GitHubError> {
    let response = client
        .get(format!("{api_base}/user"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
        .send()
        .await
        .map_err(|_| GitHubError::Upstream)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GitHubError::Unauthorized);
    }
    if !response.status().is_success() {
        return Err(GitHubError::Upstream);
    }
    response
        .json::<RawUser>()
        .await
        .map_err(|_| GitHubError::Upstream)
}

/// Reports the connection state. A token that GitHub rejects is removed.
pub async fn status(state: &AppState) -> GitHubStatus {
    let default_clone_folder = state
        .github()
        .default_parent()
        .to_string_lossy()
        .into_owned();
    let disconnected = || GitHubStatus {
        connected: false,
        login: None,
        name: None,
        avatar_url: None,
        default_clone_folder: default_clone_folder.clone(),
    };
    let connected =
        |login: Option<String>, name: Option<String>, avatar: Option<String>| GitHubStatus {
            connected: true,
            login,
            name,
            avatar_url: avatar,
            default_clone_folder: default_clone_folder.clone(),
        };

    let token = match read_token(state).await {
        Ok(Some(token)) => token,
        Ok(None) | Err(_) => return disconnected(),
    };
    if let Some((login, name, avatar)) = state.github().cached_account() {
        return connected(Some(login), name, avatar);
    }

    let Ok(client) = http_client() else {
        return connected(None, None, None);
    };
    match fetch_user(&client, state.github().api_base(), &token).await {
        Ok(account) => {
            state.github().store_account(
                account.login.clone(),
                account.name.clone(),
                account.avatar_url.clone(),
            );
            connected(Some(account.login), account.name, account.avatar_url)
        }
        Err(GitHubError::Unauthorized) => {
            let _ = delete_token(state).await;
            state.github().clear_caches();
            disconnected()
        }
        Err(_) => connected(None, None, None),
    }
}

/// Ends the connection and removes the token from the system keychain.
pub async fn sign_out(state: &AppState) -> Result<(), GitHubError> {
    state.github().cancel_all_devices();
    delete_token(state).await?;
    state.github().clear_caches();
    Ok(())
}

/// Lists the repositories the token can reach. An empty query returns the
/// most recently updated repositories; a query filters by owner and name.
pub async fn list_repos(state: &AppState, query: &str) -> Result<Vec<GitHubRepo>, GitHubError> {
    let token = required_token(state).await?;
    let mut repos = load_repos(state, &token).await?;
    let query = query.trim().to_lowercase();
    let added = added_remotes(state).await;

    if query.is_empty() {
        repos.truncate(RECENT_LIMIT);
    } else {
        let terms: Vec<&str> = query.split_whitespace().collect();
        repos.retain(|repo| {
            let name = repo.full_name.to_lowercase();
            let description = repo.description.as_deref().unwrap_or("").to_lowercase();
            terms
                .iter()
                .all(|term| name.contains(term) || description.contains(term))
        });
        repos.truncate(SEARCH_LIMIT);
    }

    for repo in &mut repos {
        repo.added = added.contains(&normalize_remote(&repo.clone_url));
    }
    Ok(repos)
}

async fn load_repos(state: &AppState, token: &str) -> Result<Vec<GitHubRepo>, GitHubError> {
    if let Some(repos) = state.github().cached_repos() {
        return Ok(repos);
    }

    let client = http_client()?;
    let mut repos: Vec<GitHubRepo> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for page in 1..=REPO_MAX_PAGES {
        let url = format!(
            "{}/user/repos?sort=updated&per_page={REPO_PAGE_SIZE}&page={page}&affiliation=owner,collaborator,organization_member",
            state.github().api_base()
        );
        let response = client
            .get(url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .send()
            .await
            .map_err(|_| GitHubError::Upstream)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(GitHubError::Upstream);
        }
        let raw = response
            .json::<Vec<RawRepo>>()
            .await
            .map_err(|_| GitHubError::Upstream)?;
        let count = raw.len();
        for repo in raw {
            if seen.insert(repo.id) {
                repos.push(GitHubRepo {
                    id: repo.id,
                    full_name: repo.full_name,
                    name: repo.name,
                    owner: repo.owner.login,
                    private: repo.private,
                    clone_url: repo.clone_url,
                    html_url: repo.html_url,
                    description: repo.description,
                    updated_at: repo.updated_at,
                    added: false,
                });
            }
        }
        if count < REPO_PAGE_SIZE {
            break;
        }
    }
    state.github().store_repos(repos.clone());
    Ok(repos)
}

async fn added_remotes(state: &AppState) -> HashSet<String> {
    if let Some(remotes) = state.github().cached_added() {
        return remotes;
    }
    let folders: Vec<PathBuf> = state
        .projects()
        .into_iter()
        .map(|project| PathBuf::from(project.folder))
        .collect();
    let timeout = state.git_timeout();
    let remotes = futures::future::join_all(
        folders
            .into_iter()
            .map(|folder| crate::git::origin_remote(folder, timeout)),
    )
    .await;
    let remotes: HashSet<String> = remotes
        .into_iter()
        .flatten()
        .map(|remote| normalize_remote(&remote))
        .collect();
    state.github().store_added(remotes.clone());
    remotes
}

#[derive(Debug)]
pub struct CloneInput {
    pub full_name: String,
    pub name: Option<String>,
    pub parent_folder: Option<String>,
}

/// Clones a repository from the connected account into a new folder and adds
/// that folder as a project. The token reaches Git through a temporary
/// askpass helper, never through the remote address.
pub async fn clone_repo(state: &AppState, input: CloneInput) -> Result<Project, GitHubError> {
    let token = required_token(state).await?;
    let full_name = input.full_name.trim();
    if !valid_full_name(full_name) {
        return Err(GitHubError::Invalid(
            "Choose a valid GitHub repository.".to_owned(),
        ));
    }
    let repos = load_repos(state, &token).await?;
    let repo = repos
        .iter()
        .find(|repo| repo.full_name.eq_ignore_ascii_case(full_name))
        .cloned()
        .ok_or(GitHubError::NotFound)?;
    if !state.github().clone_url_allowed(&repo.clone_url) {
        return Err(GitHubError::Failed);
    }
    if !valid_folder_name(&repo.name) {
        return Err(GitHubError::Invalid(
            "The repository name cannot be used as a folder name.".to_owned(),
        ));
    }

    let project_name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| repo.name.clone());

    let parent = match input.parent_folder.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => {
            let folder = crate::seed::expand_tilde(value);
            fs::create_dir_all(&folder).map_err(|_| {
                GitHubError::Invalid("The clone folder could not be created.".to_owned())
            })?;
            let folder = folder
                .canonicalize()
                .map_err(|_| GitHubError::Invalid("The clone folder does not exist.".to_owned()))?;
            if !folder.is_dir() {
                return Err(GitHubError::Invalid(
                    "The clone folder is not a directory.".to_owned(),
                ));
            }
            folder
        }
        _ => {
            let folder = state.github().default_parent();
            fs::create_dir_all(&folder).map_err(|_| GitHubError::Failed)?;
            folder
        }
    };

    let _guard = state.github().clone_gate().await;
    let target = parent.join(&repo.name);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(GitHubError::Conflict(
            "A folder with this name already exists.".to_owned(),
        ));
    }

    let askpass = write_askpass(state).map_err(|_| GitHubError::Failed)?;
    let clone_url = repo.clone_url.clone();
    let clone_target = target.clone();
    let script = askpass.clone();
    let clone_token = token.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        run_clone(&clone_url, &clone_target, &script, &clone_token)
    })
    .await;
    let _ = fs::remove_file(&askpass);

    if !matches!(outcome, Ok(Ok(()))) {
        let _ = fs::remove_dir_all(&target);
        return Err(GitHubError::Failed);
    }

    let folder = match fs::canonicalize(&target) {
        Ok(folder) => folder,
        Err(_) => {
            let _ = fs::remove_dir_all(&target);
            return Err(GitHubError::Failed);
        }
    };
    let project = match state.add_project(project_name, folder.to_string_lossy().into_owned()) {
        Ok(project) => project,
        Err(_) => {
            let _ = fs::remove_dir_all(&target);
            return Err(GitHubError::Conflict(
                "A project with this folder already exists.".to_owned(),
            ));
        }
    };
    if state.try_save().is_err() {
        state.remove_project(&project.id);
        let _ = fs::remove_dir_all(&target);
        return Err(GitHubError::Failed);
    }
    state.github().clear_caches();
    Ok(project)
}

impl GitHubInner {
    fn clone_url_allowed(&self, url: &str) -> bool {
        if url.starts_with("https://") {
            return true;
        }
        self.allow_insecure_urls && (url.starts_with("http://") || url.starts_with("file://"))
    }
}

fn write_askpass(state: &AppState) -> std::io::Result<PathBuf> {
    #[cfg(not(unix))]
    {
        let _ = state;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the credential helper is not available on this system",
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let directory = state
            .data_dir()
            .map(|directory| directory.join("github"))
            .unwrap_or_else(|| env::temp_dir().join("elesdee-github"));
        private_fs::create_private_dir_all(&directory)?;
        let path = directory.join(format!("askpass-{}.sh", Uuid::new_v4()));
        private_fs::write_private_file(
            &path,
            b"#!/bin/sh\ncase \"$1\" in\n  *[Uu]sername*) printf '%s\\n' \"x-access-token\" ;;\n  *) printf '%s\\n' \"$ELESDEE_GITHUB_TOKEN\" ;;\nesac\n",
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(path)
    }
}

#[derive(Debug)]
enum CloneFailure {
    Failed,
    Timeout,
}

fn run_clone(url: &str, target: &Path, askpass: &Path, token: &str) -> Result<(), CloneFailure> {
    let spawned = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--")
        .arg(url)
        .arg(target)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", askpass)
        .env("ELESDEE_GITHUB_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = spawned.map_err(|_| CloneFailure::Failed)?;
    let deadline = Instant::now() + CLONE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(CloneFailure::Failed)
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CloneFailure::Timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CloneFailure::Failed);
            }
        }
    }
}

fn valid_full_name(value: &str) -> bool {
    let Some((owner, name)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !name.is_empty()
        && !name.contains('/')
        && valid_folder_name(owner)
        && valid_folder_name(name)
}

fn valid_folder_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.starts_with('-')
        && value.chars().count() <= 200
        && !value
            .chars()
            .any(|character| character == '/' || character == '\\' || character.is_control())
}

/// Reduces a Git remote address to `host/owner/name`, so https, ssh and scp
/// forms of the same repository compare equal.
pub fn normalize_remote(value: &str) -> String {
    let mut remote = value.trim().to_owned();
    if let Some(rest) = remote.strip_prefix("git@") {
        remote = rest.replacen(':', "/", 1);
    } else if let Some(rest) = remote.strip_prefix("ssh://") {
        let rest = rest.strip_prefix("git@").unwrap_or(rest);
        remote = rest.replacen(':', "/", 1);
    } else if let Some(rest) = remote.strip_prefix("https://") {
        remote = rest.to_owned();
    } else if let Some(rest) = remote.strip_prefix("http://") {
        remote = rest.to_owned();
    } else if let Some(rest) = remote.strip_prefix("file://") {
        remote = rest.to_owned();
    }
    if let Some((_, path)) = remote.split_once('/') {
        remote = path.to_owned();
    }
    remote = remote.trim_end_matches('/').to_owned();
    remote = remote.strip_suffix(".git").unwrap_or(&remote).to_owned();
    remote.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_forms_compare_equal() {
        let expected = normalize_remote("https://github.com/acme/web.git");
        assert_eq!(expected, "acme/web");
        assert_eq!(normalize_remote("git@github.com:acme/web.git"), expected);
        assert_eq!(normalize_remote("ssh://git@github.com/acme/web"), expected);
        assert_eq!(normalize_remote("https://GitHub.com/acme/web/"), expected);
        assert_eq!(normalize_remote("http://github.com/acme/web.git"), expected);
    }

    #[test]
    fn repository_names_are_safe() {
        assert!(valid_full_name("acme/web"));
        assert!(valid_full_name("acme/web.app"));
        assert!(!valid_full_name("acme"));
        assert!(!valid_full_name("acme/.."));
        assert!(!valid_full_name("acme/../evil"));
        assert!(!valid_full_name("acme/web\\evil"));
        assert!(!valid_folder_name("."));
        assert!(!valid_folder_name(".."));
        assert!(!valid_folder_name("--no-checkout"));
        assert!(!valid_full_name("acme/--mirror"));
    }
}
