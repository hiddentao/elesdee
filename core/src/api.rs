use std::convert::Infallible;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN, VARY,
};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{SecondsFormat, Utc};
use futures::Stream;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time::{MissedTickBehavior, interval_at};

use crate::antigravity;
use crate::claude;
use crate::clients;
use crate::git;
use crate::seed;
use crate::sessions::{
    self, BeginPermission, CloseFailure, InputFailure, PermissionAnswerFailure, ResizeFailure,
    SessionEntry, TerminalSize,
};
use crate::shortcuts;
use crate::slug::slugify;
use crate::state::{
    Account, AppState, Auth, Bootstrap, Client, DeleteSessionFailure, HealthResponse,
    PendingPermission, PermissionDecision, Project, Question, QuestionFailure, Session,
    SessionEvent, SessionKind, SessionState, WorkState,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SSE_HEARTBEAT: Duration = Duration::from_secs(15);
const ALLOWED_ORIGINS: [&str; 5] = [
    "http://localhost:5173",
    "http://127.0.0.1:5173",
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];
const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn unsupported_media_type(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn too_many_requests(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
        }
    }

    fn request_timeout(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::REQUEST_TIMEOUT,
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }

    fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionRequest {
    project_id: String,
    kind: SessionKind,
    account_id: Option<String>,
    name: Option<String>,
    folder: Option<String>,
    source_branch: Option<String>,
    first_prompt: Option<String>,
    parent_session_id: Option<String>,
    parent_session_name: Option<String>,
    env: Option<Vec<crate::state::EnvOverride>>,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct ResizeRequest {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    folder: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAccountRequest {
    name: String,
    provider_id: String,
    client: Option<Client>,
    auth: Auth,
    key: Option<String>,
    server: Option<String>,
    model: Option<String>,
    api_style: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BranchQuery {
    folder: String,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InputRequest {
    data: String,
}

#[derive(Debug, Deserialize)]
struct CloseRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchAccountRequest {
    account_id: String,
}

#[derive(Debug, Deserialize)]
struct UsageRequest {
    rows: Vec<crate::state::UsageRow>,
}

#[derive(Debug, Deserialize)]
struct AnswerRequest {
    answer: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApproveRequest {
    #[serde(default)]
    allow_further: bool,
}

#[derive(Serialize)]
struct ApproveResponse {
    request: crate::state::SessionRequest,
    session: Option<Session>,
}

#[derive(Debug, Deserialize)]
struct RenameRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ReopenRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionRequest {
    request_id: String,
    decision: PermissionDecision,
}

#[derive(Debug, Deserialize)]
struct HookQuery {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Option<serde_json::Value>,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    notification_type: Option<String>,
    #[serde(default)]
    message: Option<serde_json::Value>,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    text: Option<serde_json::Value>,
    #[serde(default)]
    last_assistant_message: Option<String>,
}

struct PreparedSession {
    session: Session,
    folder: PathBuf,
    command: CommandBuilder,
    repo_root: Option<PathBuf>,
    antigravity_account: Option<String>,
    environment: Vec<(String, String)>,
    startup_input: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OutputQuery {
    after: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let streaming = Router::new()
        .route("/api/sessions/{id}/output", get(session_output))
        .route("/api/events", get(events))
        .route("/api/questions/events", get(question_events))
        .route("/internal/hooks/claude/{id}", post(claude_hook));

    let regular = Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/branches", get(list_branches))
        .route("/api/providers", get(list_providers))
        .route("/api/accounts", get(list_accounts).post(create_account))
        .route("/api/accounts/{id}/usage", post(update_account_usage))
        .route("/api/questions", get(list_questions))
        .route("/api/questions/{id}/answer", post(answer_question))
        .route("/api/questions/{id}/withdraw", post(withdraw_question))
        .route(
            "/api/session-requests/{id}/approve",
            post(approve_session_request),
        )
        .route(
            "/api/session-requests/{id}/deny",
            post(deny_session_request),
        )
        .route("/api/sessions", post(create_session))
        .route(
            "/api/sessions/{id}",
            patch(rename_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/close", post(close_session))
        .route("/api/sessions/{id}/input", post(session_input))
        .route("/api/sessions/{id}/resize", post(resize_session))
        .route("/api/sessions/{id}/reopen", post(reopen_session))
        .route("/api/sessions/{id}/account", post(switch_session_account))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route("/api/sessions/{id}/permission", post(answer_permission))
        .route("/internal/mcp/{id}", post(mcp_request))
        .layer(middleware::from_fn(request_timeout));

    regular
        .merge(streaming)
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(not_found)
        .layer(middleware::from_fn(request_guard))
        .with_state(state)
}

async fn request_guard(request: Request, next: Next) -> Response {
    let origin = match validate_host_and_origin(&request) {
        Ok(origin) => origin,
        Err(error) => return security_headers(error.into_response()),
    };

    if request.method() == Method::POST || request.method() == Method::PATCH {
        if let Err(error) = validate_content_type(&request) {
            let mut response = error.into_response();
            apply_cors(&mut response, origin.as_deref(), false);
            return security_headers(response);
        }
    }

    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_cors(&mut response, origin.as_deref(), true);
        return security_headers(response);
    }

    let mut response = next.run(request).await;
    apply_cors(&mut response, origin.as_deref(), false);
    security_headers(response)
}

async fn request_timeout(request: Request, next: Next) -> Response {
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::request_timeout("Request timed out.").into_response(),
    }
}

fn validate_host_and_origin(request: &Request) -> Result<Option<String>, ApiError> {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("Missing or invalid Host header."))?;
    if !allowed_host(host) {
        return Err(ApiError::forbidden("Host is not allowed."));
    }

    match request.headers().get(ORIGIN) {
        Some(origin) => {
            let origin = origin
                .to_str()
                .map_err(|_| ApiError::forbidden("Origin is not allowed."))?;
            if !ALLOWED_ORIGINS.contains(&origin) {
                return Err(ApiError::forbidden("Origin is not allowed."));
            }
            Ok(Some(origin.to_owned()))
        }
        None => Ok(None),
    }
}

fn validate_content_type(request: &Request) -> Result<(), ApiError> {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unsupported_media_type("Content-Type must be application/json.")
        })?;
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case("application/json") {
        return Err(ApiError::unsupported_media_type(
            "Content-Type must be application/json.",
        ));
    }
    Ok(())
}

fn apply_cors(response: &mut Response, origin: Option<&str>, preflight: bool) {
    if let Some(origin) = origin {
        if let Ok(value) = HeaderValue::from_str(origin) {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
        }
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("Origin"));
    }

    if preflight {
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PATCH, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
}

fn allowed_host(value: &str) -> bool {
    let value = value.trim();

    if let Some(rest) = value.strip_prefix('[') {
        let Some((inside, tail)) = rest.split_once(']') else {
            return false;
        };
        if inside != "::1" {
            return false;
        }
        return match tail {
            "" => true,
            _ => tail.strip_prefix(':').is_some_and(valid_port),
        };
    }

    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) => {
            if host.contains(':') {
                return false;
            }
            (host, Some(port))
        }
        None => (value, None),
    };

    let host_allowed = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    let port_allowed = port.is_none_or(valid_port);
    host_allowed && port_allowed
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn security_headers(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(NOSNIFF, HeaderValue::from_static("nosniff"));
    response
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let agents = state.health_checks().await;
    Json(HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        agents,
    })
}

async fn bootstrap(State(state): State<AppState>) -> Json<Bootstrap> {
    Json(state.bootstrap())
}

async fn list_projects(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<Project>>, ApiError> {
    let filter = list_filter(query)?;
    let mut projects = state.projects();
    if let Some(filter) = filter.as_deref() {
        projects.retain(|project| {
            matches_filter(&project.name, filter) || matches_filter(&project.folder, filter)
        });
    }
    projects.sort_by_cached_key(|project| (project.name.to_lowercase(), project.id.clone()));
    Ok(Json(projects))
}

async fn list_branches(
    State(state): State<AppState>,
    query: Result<Query<BranchQuery>, QueryRejection>,
) -> Result<Json<Vec<git::BranchInfo>>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid branch query."))?;
    let folder = canonical_directory(seed::expand_tilde(query.folder.trim()))?;
    let repo_root = git::repo_root(folder, state.git_timeout())
        .await
        .ok_or_else(|| ApiError::unprocessable("Folder is not a Git repository."))?;
    let mut branches = git::list_branches(repo_root, state.git_timeout()).await;
    if let Some(filter) = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|filter| !filter.is_empty())
        .map(str::to_lowercase)
    {
        branches.retain(|branch| branch.name.to_lowercase().contains(&filter));
    }
    Ok(Json(branches))
}

async fn list_providers() -> Json<Vec<ProviderInfo>> {
    Json(
        crate::providers::PROVIDERS
            .iter()
            .map(|provider| ProviderInfo {
                id: provider.id.to_owned(),
                label: provider.label.to_owned(),
                client: provider.client,
                auth: provider.auth.to_vec(),
                default_model: provider.default_model.map(str::to_owned),
                custom: provider.custom,
            })
            .collect(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInfo {
    id: String,
    label: String,
    client: Client,
    auth: Vec<Auth>,
    default_model: Option<String>,
    custom: bool,
}

async fn list_accounts(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<Account>>, ApiError> {    let filter = list_filter(query)?;
    let mut accounts = state.accounts();
    if let Some(filter) = filter.as_deref() {
        accounts.retain(|account| {
            matches_filter(&account.name, filter)
                || matches_filter(account.client.display_name(), filter)
                || matches_filter(&account.provider, filter)
                || matches_filter(&account.plan, filter)
        });
    }
    accounts.sort_by_cached_key(|account| (account.name.to_lowercase(), account.id.clone()));
    Ok(Json(accounts))
}

fn list_filter(
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Option<String>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid list query."))?;
    Ok(query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|filter| !filter.is_empty())
        .map(str::to_lowercase))
}

fn matches_filter(value: &str, filter: &str) -> bool {
    value.to_lowercase().contains(filter)
}

async fn create_project(
    State(state): State<AppState>,
    body: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> Result<Json<Project>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("Project name is required."));
    }

    let folder = canonical_directory(seed::expand_tilde(request.folder.trim()))?;
    let project = state
        .add_project(name.to_owned(), path_string(&folder))
        .map_err(|_| ApiError::conflict("A project with this folder already exists."))?;
    state.save();

    Ok(Json(project))
}

async fn create_account(
    State(state): State<AppState>,
    body: Result<Json<CreateAccountRequest>, JsonRejection>,
) -> Result<Json<Account>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request(
            "Account name must be between 1 and 80 characters.",
        ));
    }

    let provider_id = request.provider_id.trim().to_ascii_lowercase();
    let provider = crate::providers::find(&provider_id)
        .ok_or_else(|| ApiError::unprocessable("This provider is not supported."))?;
    let api_style = request
        .api_style
        .as_deref()
        .map(str::trim)
        .map(str::to_lowercase)
        .filter(|style| !style.is_empty());

    let client = if provider.custom {
        let style = api_style
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("Choose an API style."))?;
        let allowed = crate::providers::clients_for_style(style);
        let client = request.client.unwrap_or(provider.client);
        if !allowed.contains(&client) {
            return Err(ApiError::unprocessable(
                "This client does not support that API style.",
            ));
        }
        client
    } else {
        if let Some(client) = request.client
            && client != provider.client
        {
            return Err(ApiError::unprocessable(
                "This provider and client pair is not supported.",
            ));
        }
        provider.client
    };

    if !provider.auth.contains(&request.auth) {
        return Err(ApiError::unprocessable(
            "This account does not support that authentication method.",
        ));
    }

    let key = request
        .key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());
    if request.auth == Auth::Key && key.is_none() {
        return Err(ApiError::bad_request("API key is required."));
    }

    let needs_server = provider.custom
        || request.auth == Auth::Local
        || matches!(provider.id, "ollama" | "lmstudio");
    let server = request
        .server
        .as_deref()
        .map(str::trim)
        .filter(|server| !server.is_empty())
        .map(str::to_owned);
    if needs_server {
        let Some(server) = server.as_deref() else {
            return Err(ApiError::bad_request("Server URL is required."));
        };
        if !safe_server_url(server) {
            return Err(ApiError::bad_request("The server URL is not valid."));
        }
    }

    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .or_else(|| provider.default_model.map(str::to_owned));
    if model
        .as_deref()
        .is_some_and(|model| model.chars().count() > 120)
    {
        return Err(ApiError::bad_request("Model name is too long."));
    }

    let plan = match request.auth {
        Auth::Subscription => "subscription",
        Auth::Local => "local",
        Auth::Key => "pay as you go",
    };
    let account = state.add_account(Account {
        id: String::new(),
        name: name.to_owned(),
        plan: plan.to_owned(),
        client,
        provider: provider_id,
        auth: request.auth,
        sample: false,
        rows: Vec::new(),
        foot: "No official usage source.".to_owned(),
        server: if provider.custom || request.auth == Auth::Local {
            server
        } else {
            None
        },
        model,
        api_style: if provider.custom { api_style } else { None },
    });
    if let Some(key) = key
        && let Err(error) = store_account_key(&state, &account.id, key).await
    {
        state.remove_account(&account.id);
        tracing::error!(account = %account.id, %error, "could not store account key");
        return Err(ApiError::internal("Could not store the account key."));
    }
    state.save();
    Ok(Json(account))
}

fn safe_server_url(value: &str) -> bool {
    (value.starts_with("https://") || value.starts_with("http://"))
        && value.chars().count() <= 500
        && !value.chars().any(|character| {
            character.is_control() || character.is_whitespace()
        })
}

async fn store_account_key(state: &AppState, account_id: &str, key: &str) -> Result<(), String> {
    let store = state.secret_store();
    let account_id = account_id.to_owned();
    let key = key.to_owned();
    match tokio::task::spawn_blocking(move || store.set_key(&account_id, &key)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

async fn account_key(state: &AppState, account_id: &str) -> Result<String, ApiError> {
    let store = state.secret_store();
    let id = account_id.to_owned();
    let result = tokio::task::spawn_blocking(move || store.get_key(&id)).await;
    match result {
        Ok(Ok(Some(key))) if !key.is_empty() => Ok(key),
        Ok(Ok(_)) => Err(ApiError::unprocessable("The account key is not available.")),
        Ok(Err(error)) => {
            tracing::error!(account = %account_id, %error, "could not read account key");
            Err(ApiError::internal("Could not read the account key."))
        }
        Err(error) => {
            tracing::error!(account = %account_id, %error, "could not read account key");
            Err(ApiError::internal("Could not read the account key."))
        }
    }
}

async fn account_environment(
    state: &AppState,
    account: &Account,
) -> Result<Vec<(String, String)>, ApiError> {
    let key = if account.auth == Auth::Key && account.provider != antigravity::PROVIDER_ID {
        Some(account_key(state, &account.id).await?)
    } else {
        None
    };
    Ok(crate::providers::environment(account, key.as_deref()))
}

fn secret_environment_values(environment: &[(String, String)]) -> Vec<String> {
    environment
        .iter()
        .filter(|(name, _)| {
            let name = name.to_ascii_uppercase();
            name.contains("KEY")
                || name.contains("TOKEN")
                || name.contains("SECRET")
                || name.contains("PASSWORD")
        })
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
        .collect()
}

async fn rollback_created_session(
    state: &AppState,
    entry: &SessionEntry,
    session_id: &str,
    worktree: Option<(PathBuf, PathBuf, String)>,
) {
    if let Some((repo_root, worktree, branch)) = worktree {
        git::rollback_worktree(repo_root, worktree, branch, state.git_timeout()).await;
    }
    entry.remove_session_files();
    state.remove_session(session_id);
}

async fn create_session(
    State(state): State<AppState>,
    body: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let size = requested_size(request.cols, request.rows)?;
    let env_overrides = validated_env(request.env.clone())?;
    let project = state
        .project(&request.project_id)
        .ok_or_else(|| ApiError::not_found("Unknown project."))?;

    let mut prepared = match request.kind {
        SessionKind::Terminal => prepare_terminal(&state, &request, &project)?,
        SessionKind::Agent => prepare_agent(&state, &request, &project).await?,
    };
    prepared.session.env_overrides = env_overrides.clone();
    let is_claude_agent = prepared.session.kind == SessionKind::Agent
        && prepared.session.client == Some(Client::ClaudeCode);
    let agent_client = prepared.session.client;
    let identity_guard = if prepared.repo_root.is_some() {
        Some(state.lock_agent_identity().await)
    } else {
        None
    };

    let output_budget = state.output_budget();
    let session_id = prepared.session.id.clone();
    let startup_input = prepared.startup_input.clone();
    let session_environment = prepared.environment.clone();
    let mut opencode_base = None;
    let entry = Arc::new(SessionEntry::new(
        prepared.session,
        Arc::clone(&output_budget),
        sessions::SessionRuntimeConfig::new(state.event_sender(), state.preview_throttle()),
    ));
    state.insert_session(Arc::clone(&entry)).map_err(|_| {
        ApiError::too_many_requests("Session limit reached. Close a session first.")
    })?;
    output_budget.register(&entry);

    let mut worktree_cleanup = None;
    let folder = if let Some(repo_root) = prepared.repo_root {
        let current = entry.session();
        let Some(data_dir) = state.data_dir().map(Path::to_path_buf) else {
            rollback_created_session(&state, &entry, &session_id, None).await;
            return Err(ApiError::unprocessable(
                "Agent sessions need a data directory.",
            ));
        };
        let source = match request
            .source_branch
            .as_deref()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
        {
            Some(branch) if !safe_branch_name(branch) => {
                rollback_created_session(&state, &entry, &session_id, None).await;
                return Err(ApiError::bad_request("The source branch name is not valid."));
            }
            Some(branch) => {
                if !git::ref_exists(
                    repo_root.clone(),
                    branch.to_owned(),
                    state.git_timeout(),
                )
                .await
                {
                    rollback_created_session(&state, &entry, &session_id, None).await;
                    return Err(ApiError::unprocessable("The source branch does not exist."));
                }
                branch.to_owned()
            }
            None => "HEAD".to_owned(),
        };
        let base = slugify(&current.name, "session");
        let project_part = slugify(&current.project_id, "project");
        let mut counter = 1_u32;
        let branch = loop {
            let candidate = if counter == 1 {
                base.clone()
            } else {
                format!("{base}-{counter}")
            };
            let branch = format!("elesdee/{project_part}/{candidate}");
            if !git::branch_exists(repo_root.clone(), branch.clone(), state.git_timeout()).await
                && state.try_reserve_short(&entry, &current.project_id, &candidate)
            {
                break branch;
            }
            counter = counter.saturating_add(1);
        };

        let worktrees_root = data_dir.join("worktrees");
        let worktree = worktrees_root.join(&current.id);
        if git::create_worktree(
            repo_root.clone(),
            worktrees_root,
            worktree.clone(),
            branch.clone(),
            source,
            state.git_timeout(),
        )
        .await
        .is_err()
        {
            rollback_created_session(&state, &entry, &session_id, None).await;
            return Err(ApiError::unprocessable(
                "Could not create the Git worktree.",
            ));
        }
        worktree_cleanup = Some((repo_root.clone(), worktree.clone(), branch.clone()));
        entry.set_agent_worktree(path_string(&worktree), branch, path_string(&repo_root));
        worktree
    } else {
        prepared.folder
    };
    drop(identity_guard);

    if is_claude_agent {
        match install_claude_settings(&state, &entry, &session_id) {
            Ok(settings_path) => {
                if state.command_override().is_none() {
                    claude::add_settings_argument(&mut prepared.command, &settings_path);
                }
            }
            Err(error) => {
                rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
                return Err(error);
            }
        }
    }

    let mut environment = session_environment;
    if let Some(account_id) = prepared.antigravity_account.as_deref() {
        let Some(data_dir) = state.data_dir().map(Path::to_path_buf) else {
            rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
            return Err(ApiError::unprocessable(
                "Agent sessions need a data directory.",
            ));
        };
        let key = match account_key(&state, account_id).await {
            Ok(key) => key,
            Err(error) => {
                rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
                return Err(error);
            }
        };
        match antigravity::prepare_home(&data_dir, &session_id) {
            Ok(home) => {
                entry.set_home(home.clone());
                environment = antigravity::environment(&home, &key);
            }
            Err(error) => {
                tracing::error!(session = %session_id, %error, "could not prepare the Antigravity home");
                rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
                return Err(ApiError::unprocessable(
                    "Could not prepare the Antigravity session home.",
                ));
            }
        }
    }

    for item in &env_overrides {
        environment.push((item.name.clone(), item.value.clone()));
    }

    if let Some(client) = agent_client {
        if let Err(error) = configure_session_tools(
            &state,
            &entry,
            &session_id,
            client,
            &mut prepared.command,
            &mut environment,
        ) {
            rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
            return Err(error);
        }
        if client == Client::Codex
            && let Err(error) = start_codex_runtime(&entry, &mut prepared.command)
        {
            rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
            return Err(error);
        }
        if client == Client::OpenCode2
            && let Some(port) = crate::opencode::free_port()
        {
            crate::opencode::launch_args(&mut prepared.command, port);
            let base_url = format!("http://127.0.0.1:{port}");
            entry.set_opencode_runtime(crate::sessions::OpenCodeRuntime {
                base_url: base_url.clone(),
                port,
            });
            opencode_base = Some(base_url);
        }
    }

    entry.set_redactions(secret_environment_values(&environment));

    if let Err(error) = sessions::spawn(
        &entry,
        prepared.command,
        &folder,
        size,
        state.stall_writer_for_tests(),
        state.event_sender(),
        &environment,
    ) {
        rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
        return Err(ApiError::unprocessable(error));
    }

    if let Some(base_url) = opencode_base {
        start_opencode_poller(state.clone(), Arc::clone(&entry), base_url);
    }

    if let Some(prompt) = startup_input {
        let entry = Arc::clone(&entry);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let mut bytes = prompt.into_bytes();
            bytes.push(b'\n');
            let _ = entry.write_input(bytes).await;
        });
    }

    let session = entry.session();
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

async fn close_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<CloseRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let _ = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("Session is closing."))?;
    if entry.is_closing() {
        return Err(ApiError::conflict("Session is closing."));
    }

    let closing = entry.session();
    let should_wait = entry.begin_close().map_err(|failure| match failure {
        CloseFailure::AlreadyClosed => ApiError::conflict("Session is already closed."),
        CloseFailure::Closing => ApiError::conflict("Session is closing."),
    })?;

    let agent_stopped = closing.kind != SessionKind::Agent
        || !should_wait
        || wait_for_process_exit(&entry, Duration::from_secs(2)).await;

    let preview = if closing.kind == SessionKind::Agent {
        if !agent_stopped {
            Some("Closed. The worktree was kept because the agent did not stop.".to_owned())
        } else {
            match (entry.repo_root(), closing.branch.as_deref()) {
                (Some(repo_root), Some(branch)) => {
                    let outcome = git::close_worktree(
                        PathBuf::from(repo_root),
                        PathBuf::from(&closing.folder),
                        branch.to_owned(),
                        closing.name.clone(),
                        git::GitTimeouts {
                            local: state.git_timeout(),
                            push: state.git_push_timeout(),
                        },
                    )
                    .await;
                    Some(outcome.preview().to_owned())
                }
                _ => Some("Closed. The worktree information is unavailable.".to_owned()),
            }
        }
    } else {
        None
    };

    let session = entry.finish_close(preview.as_deref());
    state.withdraw_questions_for_session(&id);
    state.withdraw_requests_for_session(&id);
    state.release_activation();
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

async fn wait_for_process_exit(entry: &SessionEntry, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if entry.process_exited() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn rename_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<RenameRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request(
            "Session name must be between 1 and 64 characters.",
        ));
    }

    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("Session is closing."))?;
    if entry.is_closing() {
        return Err(ApiError::conflict("Session is closing."));
    }
    let session = entry.rename(name.to_owned());
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

async fn reopen_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<ReopenRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let _ = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("Session is closing."))?;
    if entry.is_closing() {
        return Err(ApiError::conflict("Session is closing."));
    }
    let suspended = entry.session();
    if suspended.status != SessionState::Closed {
        return Err(ApiError::conflict("Session is already active."));
    }

    let folder_path = PathBuf::from(&suspended.folder);
    let mut environment = Vec::new();
    let mut opencode_base = None;
    let folder = if suspended.kind == SessionKind::Agent {
        let account_id = suspended
            .account_id
            .as_deref()
            .ok_or_else(|| ApiError::unprocessable("Account is no longer available."))?;
        let account = state
            .account(account_id)
            .ok_or_else(|| ApiError::unprocessable("Account is no longer available."))?;
        environment = account_environment(&state, &account).await?;
        if state.command_override().is_none()
            && state.agent_bin().is_none()
            && clients::find_client(account.client).is_none()
        {
            return Err(ApiError::unprocessable(format!(
                "{} is not installed.",
                account.client
            )));
        }

        let repo_root = entry.repo_root().ok_or_else(|| {
            ApiError::unprocessable("The session repository is no longer available.")
        })?;
        let branch = suspended
            .branch
            .as_deref()
            .ok_or_else(|| ApiError::unprocessable("The session branch is no longer available."))?;
        let data_dir = state
            .data_dir()
            .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
        let worktrees_root = data_dir.join("worktrees");
        match git::ensure_worktree(
            PathBuf::from(repo_root),
            worktrees_root,
            folder_path.clone(),
            branch.to_owned(),
            state.git_timeout(),
        )
        .await
        {
            Ok(()) => {}
            Err(git::EnsureFailure::BranchMissing) => {
                return Err(ApiError::unprocessable(
                    "The session branch no longer exists.",
                ));
            }
            Err(git::EnsureFailure::Failed) => {
                return Err(ApiError::unprocessable(
                    "Could not restore the Git worktree.",
                ));
            }
        }
        canonical_directory(folder_path)?
    } else {
        canonical_directory(folder_path)?
    };

    let mut command = session_command(&state, suspended.kind, suspended.client)?;
    if suspended.kind == SessionKind::Agent
        && let Some(client) = suspended.client
    {
        for argument in sessions::resume_arguments(client) {
            command.arg(*argument);
        }
    }
    let is_claude_agent =
        suspended.kind == SessionKind::Agent && suspended.client == Some(Client::ClaudeCode);
    if is_claude_agent {
        let settings_path = install_claude_settings(&state, &entry, &id)?;
        if state.command_override().is_none() {
            claude::add_settings_argument(&mut command, &settings_path);
        }
    }
    if suspended.kind == SessionKind::Agent && suspended.client == Some(Client::Antigravity) {
        let account_id = suspended
            .account_id
            .as_deref()
            .ok_or_else(|| ApiError::unprocessable("Account is no longer available."))?;
        let data_dir = state
            .data_dir()
            .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
        let key = account_key(&state, account_id).await?;
        let home = antigravity::prepare_home(data_dir, &id).map_err(|error| {
            tracing::error!(session = %id, %error, "could not prepare the Antigravity home");
            ApiError::unprocessable("Could not prepare the Antigravity session home.")
        })?;
        entry.set_home(home.clone());
        environment = antigravity::environment(&home, &key);
    }
    for item in &suspended.env_overrides {
        environment.push((item.name.clone(), item.value.clone()));
    }
    let size = entry.stored_size();
    if let Some(client) = suspended.client {
        configure_session_tools(&state, &entry, &id, client, &mut command, &mut environment)?;
        if client == Client::Codex {
            start_codex_runtime(&entry, &mut command)?;
        }
        if client == Client::OpenCode2
            && let Some(port) = crate::opencode::free_port()
        {
            crate::opencode::launch_args(&mut command, port);
            let base_url = format!("http://127.0.0.1:{port}");
            entry.set_opencode_runtime(crate::sessions::OpenCodeRuntime {
                base_url: base_url.clone(),
                port,
            });
            opencode_base = Some(base_url);
        }
    }

    state.reserve_activation().map_err(|_| {
        entry.remove_session_files();
        ApiError::too_many_requests("Session limit reached. Close a session first.")
    })?;
    match entry.begin_reopen() {
        Ok(()) => {}
        Err(crate::sessions::ReopenFailure::Closing) => {
            entry.remove_session_files();
            state.release_activation();
            return Err(ApiError::conflict("Session is closing."));
        }
        Err(crate::sessions::ReopenFailure::Active) => {
            entry.remove_session_files();
            state.release_activation();
            return Err(ApiError::conflict("Session is already active."));
        }
    }
    entry.set_redactions(secret_environment_values(&environment));
    if let Err(error) = sessions::spawn(
        &entry,
        command,
        &folder,
        size,
        state.stall_writer_for_tests(),
        state.event_sender(),
        &environment,
    ) {
        entry.rollback_reopen();
        state.release_activation();
        return Err(ApiError::unprocessable(error));
    }

    if let Some(base_url) = opencode_base {
        start_opencode_poller(state.clone(), Arc::clone(&entry), base_url);
    }

    let session = entry.session();
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

async fn switch_session_account(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<SwitchAccountRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    if session.kind != SessionKind::Agent {
        return Err(ApiError::unprocessable("Only agent sessions have accounts."));
    }
    if session.status != SessionState::Active {
        return Err(ApiError::conflict("Reopen the session before switching."));
    }
    let account_id = request.account_id.trim();
    if account_id.is_empty() {
        return Err(ApiError::bad_request("Choose an account."));
    }
    let account = state
        .account(account_id)
        .ok_or_else(|| ApiError::not_found("Unknown account."))?;
    let same_client = session.client == Some(account.client);
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("The session is busy."))?;
    relaunch_with_account(&state, &entry, &account, same_client).await
}

async fn resume_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<ReopenRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let _ = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    if session.status != SessionState::Active || session.work_state != Some(WorkState::Paused) {
        return Err(ApiError::conflict("The session is not paused."));
    }
    if session.kind == SessionKind::Terminal {
        let _guard = entry
            .try_lifecycle()
            .ok_or_else(|| ApiError::conflict("The session is busy."))?;
        return relaunch_terminal(&state, &entry).await;
    }
    let Some(client) = session.client else {
        return Err(ApiError::unprocessable("The session has no client."));
    };
    let account = state
        .accounts()
        .into_iter()
        .filter(|account| {
            !account.sample
                && account.client == client
                && crate::providers::usage_state(account) == crate::providers::UsageState::Room
        })
        .max_by_key(crate::providers::remaining_percent)
        .or_else(|| {
            session
                .account_id
                .as_deref()
                .and_then(|id| state.account(id))
                .filter(|account| account.client == client)
        })
        .ok_or_else(|| ApiError::unprocessable("No account has usage left."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("The session is busy."))?;
    relaunch_with_account(&state, &entry, &account, true).await
}

async fn relaunch_with_account(
    state: &AppState,
    entry: &Arc<SessionEntry>,
    account: &Account,
    resume: bool,
) -> Result<Json<Session>, ApiError> {
    let session = entry.session();
    if state.command_override().is_none()
        && state.agent_bin().is_none()
        && clients::find_client(account.client).is_none()
    {
        return Err(ApiError::unprocessable(format!(
            "{} is not installed.",
            account.client
        )));
    }

    let mut command = session_command(state, SessionKind::Agent, Some(account.client))?;
    if resume {
        for argument in sessions::resume_arguments(account.client) {
            command.arg(*argument);
        }
    }
    let mut environment = account_environment(state, account).await?;
    let folder = canonical_directory(PathBuf::from(&session.folder))?;
    let mut opencode_base = None;

    let should_wait = entry
        .begin_close()
        .map_err(|_| ApiError::conflict("The session is closing."))?;
    if should_wait {
        let _ = wait_for_process_exit(entry, Duration::from_secs(2)).await;
    }
    entry.remove_session_files();

    if account.client == Client::ClaudeCode {
        match install_claude_settings(state, entry, &session.id) {
            Ok(settings_path) => {
                if state.command_override().is_none() {
                    claude::add_settings_argument(&mut command, &settings_path);
                }
            }
            Err(error) => {
                let failed = entry.mark_switch_failed("The account switch failed.");
                state.emit(SessionEvent::updated(failed));
                state.save();
                return Err(error);
            }
        }
    }
    if account.client == Client::Antigravity {
        let key = account_key(state, &account.id).await?;
        let data_dir = state
            .data_dir()
            .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
        match antigravity::prepare_home(data_dir, &session.id) {
            Ok(home) => {
                entry.set_home(home.clone());
                environment.extend(antigravity::environment(&home, &key));
            }
            Err(error) => {
                tracing::error!(session = %session.id, %error, "could not prepare the Antigravity home");
                let failed = entry.mark_switch_failed("The account switch failed.");
                state.emit(SessionEvent::updated(failed));
                state.save();
                return Err(ApiError::unprocessable(
                    "Could not prepare the Antigravity session home.",
                ));
            }
        }
    }

    for item in &session.env_overrides {
        environment.push((item.name.clone(), item.value.clone()));
    }

    if let Err(error) = configure_session_tools(
        state,
        entry,
        &session.id,
        account.client,
        &mut command,
        &mut environment,
    ) {
        let failed = entry.mark_switch_failed("The account switch failed.");
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(error);
    }
    if account.client == Client::Codex
        && let Err(error) = start_codex_runtime(entry, &mut command)
    {
        let failed = entry.mark_switch_failed("The account switch failed.");
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(error);
    }
    if account.client == Client::OpenCode2
        && let Some(port) = crate::opencode::free_port()
    {
        crate::opencode::launch_args(&mut command, port);
        let base_url = format!("http://127.0.0.1:{port}");
        entry.set_opencode_runtime(crate::sessions::OpenCodeRuntime {
            base_url: base_url.clone(),
            port,
        });
        opencode_base = Some(base_url);
    }

    entry.set_redactions(secret_environment_values(&environment));
    let switched = entry.finish_switch(
        account.id.clone(),
        account.client,
        shortcuts::for_client(account.client),
    );
    let size = entry.stored_size();
    if let Err(error) = sessions::spawn(
        entry,
        command,
        &folder,
        size,
        state.stall_writer_for_tests(),
        state.event_sender(),
        &environment,
    ) {
        let failed = entry.mark_switch_failed(&format!("The account switch failed: {error}"));
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(ApiError::unprocessable("The account switch failed."));
    }

    if let Some(base_url) = opencode_base {
        start_opencode_poller(state.clone(), Arc::clone(entry), base_url);
    }

    state.emit(SessionEvent::updated(switched.clone()));
    state.save();
    Ok(Json(switched))
}

async fn relaunch_terminal(
    state: &AppState,
    entry: &Arc<SessionEntry>,
) -> Result<Json<Session>, ApiError> {
    let session = entry.session();
    let command = session_command(state, SessionKind::Terminal, None)?;
    let folder = canonical_directory(PathBuf::from(&session.folder))?;
    let should_wait = entry
        .begin_close()
        .map_err(|_| ApiError::conflict("The session is closing."))?;
    if should_wait {
        let _ = wait_for_process_exit(entry, Duration::from_secs(2)).await;
    }
    entry.remove_session_files();
    let resumed = entry.finish_resume();
    let size = entry.stored_size();
    if let Err(error) = sessions::spawn(
        entry,
        command,
        &folder,
        size,
        state.stall_writer_for_tests(),
        state.event_sender(),
        &[],
    ) {
        let failed = entry.mark_switch_failed(&format!("The session did not resume: {error}"));
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(ApiError::unprocessable("The session did not resume."));
    }
    state.emit(SessionEvent::updated(resumed.clone()));
    state.save();
    Ok(Json(resumed))
}

async fn update_account_usage(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<UsageRequest>, JsonRejection>,
) -> Result<Json<Account>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    if request.rows.len() > 16 {
        return Err(ApiError::bad_request("Too many usage rows."));
    }
    if request.rows.iter().any(|row| row.label.chars().count() > 80) {
        return Err(ApiError::bad_request("A usage label is too long."));
    }
    let account = state
        .update_account_rows(&id, request.rows)
        .ok_or_else(|| ApiError::not_found("Unknown account."))?;
    state.save();
    auto_switch_account(&state, &account).await;
    Ok(Json(account))
}

async fn auto_switch_account(state: &AppState, account: &Account) {
    let usage = crate::providers::usage_state(account);
    if usage == crate::providers::UsageState::Room {
        resume_paused_sessions(state, account).await;
        return;
    }
    if usage != crate::providers::UsageState::NearLimit {
        return;
    }

    let entries = state.sessions();
    for entry in entries {
        let session = entry.session();
        if session.status != SessionState::Active
            || session.kind != SessionKind::Agent
            || session.account_id.as_deref() != Some(account.id.as_str())
        {
            continue;
        }
        let Some(client) = session.client else {
            continue;
        };

        let target = state
            .accounts()
            .into_iter()
            .filter(|candidate| {
                candidate.id != account.id
                    && !candidate.sample
                    && candidate.client == client
                    && crate::providers::usage_state(candidate) == crate::providers::UsageState::Room
            })
            .max_by_key(crate::providers::remaining_percent);

        let Some(target) = target else {
            let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
                continue;
            };
            let _ = pause_session(state, &entry, "Paused. No account has usage left.").await;
            continue;
        };

        let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
            continue;
        };
        if relaunch_with_account(state, &entry, &target, true)
            .await
            .is_err()
        {
            let _ = pause_session(state, &entry, "Paused. The account switch failed.").await;
        }
    }
}

async fn resume_paused_sessions(state: &AppState, account: &Account) {
    for entry in state.sessions() {
        let session = entry.session();
        if session.status != SessionState::Active
            || session.kind != SessionKind::Agent
            || session.work_state != Some(WorkState::Paused)
            || session.client != Some(account.client)
        {
            continue;
        }
        let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
            continue;
        };
        if entry.session().work_state != Some(WorkState::Paused) {
            continue;
        }
        if relaunch_with_account(state, &entry, account, true)
            .await
            .is_err()
        {
            let _ = pause_session(state, &entry, "Paused. The account switch failed.").await;
        }
    }
}

async fn pause_session(
    state: &AppState,
    entry: &Arc<SessionEntry>,
    reason: &str,
) -> Result<Session, ()> {
    let should_wait = entry.begin_close().map_err(|_| ())?;
    if should_wait {
        let _ = wait_for_process_exit(entry, Duration::from_secs(2)).await;
    }
    entry.remove_session_files();
    let session = entry.pause(reason);
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(session)
}

async fn answer_permission(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<PermissionRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;

    let session = entry.session();
    let external_opencode =
        session.client == Some(Client::OpenCode2) && session.pending_permission.is_some();
    let external_codex =
        session.client == Some(Client::Codex) && session.pending_permission.is_some();
    if external_codex {
        let sender = entry
            .codex_sender()
            .ok_or_else(|| ApiError::conflict("The session has no Codex app server."))?;
        sender
            .send(crate::codex::Decision {
                request_id: request.request_id.clone(),
                allow: request.decision == PermissionDecision::Allow,
            })
            .map_err(|_| ApiError::bad_gateway("The agent did not accept the answer."))?;
        entry
            .answer_external_permission(&request.request_id, request.decision)
            .map_err(permission_error)?;
    } else if external_opencode {
        let runtime = entry
            .opencode_runtime()
            .ok_or_else(|| ApiError::conflict("The session has no OpenCode server."))?;
        crate::opencode::reply(
            &runtime.base_url,
            &request.request_id,
            request.decision == PermissionDecision::Allow,
        )
        .await
        .map_err(|error| {
            tracing::warn!(session = %id, %error, "could not answer the OpenCode permission");
            ApiError::bad_gateway("The agent did not accept the answer.")
        })?;
        entry
            .answer_external_permission(&request.request_id, request.decision)
            .map_err(permission_error)?;
    } else {
        entry
            .answer_permission(&request.request_id, request.decision)
            .map_err(permission_error)?;
    }

    Ok(StatusCode::NO_CONTENT)
}

fn permission_error(failure: PermissionAnswerFailure) -> ApiError {
    match failure {
        PermissionAnswerFailure::Closed => ApiError::conflict("Session is closed."),
        PermissionAnswerFailure::NoPending => {
            ApiError::conflict("There is no pending permission request.")
        }
        PermissionAnswerFailure::Mismatch => {
            ApiError::conflict("The permission request id does not match.")
        }
        PermissionAnswerFailure::AlreadyAnswered => {
            ApiError::conflict("This permission request was already answered.")
        }
    }
}

async fn delete_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("Session is closing."))?;
    if entry.is_closing() {
        return Err(ApiError::conflict("Session is closing."));
    }
    let session = entry.session();
    if session.status == SessionState::Active {
        return Err(ApiError::conflict("Close the session before deleting it."));
    }

    if session.kind == SessionKind::Agent
        && let Some(repo_root) = entry.repo_root()
    {
        let _ = git::remove_worktree(
            PathBuf::from(repo_root),
            PathBuf::from(&session.folder),
            state.git_timeout(),
        )
        .await;
    }
    entry.remove_session_files();
    if session.kind == SessionKind::Agent
        && let Some(data_dir) = state.data_dir()
    {
        if session.client == Some(Client::ClaudeCode) {
            claude::remove_session_settings(data_dir, &id);
        }
        if session.client == Some(Client::Antigravity) {
            antigravity::remove_home(data_dir, &id);
        }
    }

    state.delete_session(&id).map_err(|failure| match failure {
        DeleteSessionFailure::NotFound => ApiError::not_found("Unknown session."),
        DeleteSessionFailure::Active => ApiError::conflict("Close the session before deleting it."),
    })?;
    state.save();
    Ok(StatusCode::NO_CONTENT)
}

async fn session_input(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<InputRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;

    entry
        .write_input(request.data.into_bytes())
        .await
        .map_err(|failure| match failure {
            InputFailure::Closed => ApiError::conflict("Session is closed."),
            InputFailure::Closing => ApiError::conflict("Session is closing."),
            InputFailure::NotRunning => ApiError::conflict("Session is not running."),
        })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn resize_session(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<ResizeRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let size = TerminalSize::new(request.cols, request.rows)
        .ok_or_else(|| ApiError::bad_request(TerminalSize::range_error()))?;
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;

    entry.resize(size).map_err(|failure| match failure {
        ResizeFailure::Closed => ApiError::conflict("Session is closed."),
        ResizeFailure::Closing => ApiError::conflict("Session is closing."),
        ResizeFailure::NotRunning => ApiError::conflict("Session is not running."),
    })?;
    state.save();

    Ok(StatusCode::NO_CONTENT)
}

async fn session_output(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<OutputQuery>, QueryRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let Query(query) =
        query.map_err(|error| ApiError::bad_request(format!("Invalid output query: {error}")))?;

    let after = match query
        .after
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.parse::<u64>().map_err(|_| {
            ApiError::bad_request("Query parameter after must be a non-negative integer.")
        })?,
        None => 0,
    };

    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let permit = state
        .try_acquire_output_sse()
        .ok_or_else(|| ApiError::too_many_requests("Too many open output streams."))?;

    let mut receiver = entry.subscribe_output();
    let mut close_receiver = entry.subscribe_close();
    let snapshot = entry
        .output_snapshot(after)
        .map_err(|failure| match failure {
            sessions::SnapshotFailure::AfterAhead { newest } => ApiError::bad_request(format!(
                "Query parameter after is beyond the newest sequence ({newest})."
            )),
            sessions::SnapshotFailure::LogUnavailable => {
                ApiError::bad_request("Output at that position is no longer retained.")
            }
        })?;
    let last_seq = snapshot.newest_seq;
    let replay = snapshot.chunks;

    let stream = async_stream::stream! {
        let _permit = permit;
        yield Ok(Event::default().comment("connected"));

        for chunk in replay {
            if let Some(event) = json_event(&chunk) {
                yield Ok(event);
            }
        }

        if !*close_receiver.borrow_and_update() {
            let mut heartbeat = interval_at(
                tokio::time::Instant::now() + SSE_HEARTBEAT,
                SSE_HEARTBEAT,
            );
            heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    changed = close_receiver.changed() => {
                        let _ = changed;
                        break;
                    }
                    _ = heartbeat.tick() => {
                        yield Ok(Event::default().comment("heartbeat"));
                    }
                    received = receiver.recv() => {
                        match received {
                            Ok(chunk) if chunk.seq > last_seq => {
                                if let Some(event) = json_event(&chunk) {
                                    yield Ok(event);
                                }
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(_)) => break,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

async fn list_questions(State(state): State<AppState>) -> Json<Vec<Question>> {
    Json(state.pending_questions())
}

async fn answer_question(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<AnswerRequest>, JsonRejection>,
) -> Result<Json<Question>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let answer = request.answer.trim();
    if answer.is_empty() || answer.chars().count() > 2000 {
        return Err(ApiError::bad_request(
            "The answer must be between 1 and 2000 characters.",
        ));
    }
    state
        .answer_question(&id, answer.to_owned())
        .map(Json)
        .map_err(question_error)
}

async fn withdraw_question(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<CloseRequest>, JsonRejection>,
) -> Result<Json<Question>, ApiError> {
    let _ = body.map_err(invalid_json)?;
    state
        .withdraw_question(&id)
        .map(Json)
        .map_err(question_error)
}

fn question_error(failure: QuestionFailure) -> ApiError {
    match failure {
        QuestionFailure::NotFound => ApiError::not_found("Unknown question."),
        QuestionFailure::NotPending => {
            ApiError::conflict("This question is no longer pending.")
        }
        QuestionFailure::Closed => ApiError::conflict("The session is closed."),
    }
}

async fn approve_session_request(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<ApproveRequest>, JsonRejection>,
) -> Result<Json<ApproveResponse>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let pending = state
        .session_request(&id)
        .filter(|request| request.status == crate::state::RequestStatus::Pending)
        .ok_or_else(|| ApiError::not_found("Unknown session request."))?;

    let session = create_session_via_api(
        &state,
        serde_json::json!({
            "projectId": pending.project_id,
            "kind": "agent",
            "accountId": pending.account_id,
            "name": pending.name,
            "folder": pending.folder,
            "sourceBranch": pending.source_branch,
            "firstPrompt": pending.first_prompt,
            "parentSessionId": pending.parent_session_id,
            "parentSessionName": pending.parent_session_name
        }),
    )
    .await
    .map_err(|_| ApiError::unprocessable("The session did not start."))?;

    if request.allow_further
        && let Some(parent) = state.session(&pending.parent_session_id)
    {
        parent.set_auto_approve_children(true);
    }
    let request = state
        .finish_session_request(
            &id,
            crate::state::RequestStatus::Approved,
            Some(session.id.clone()),
        )
        .ok_or_else(|| ApiError::conflict("The request was already answered."))?;
    Ok(Json(ApproveResponse {
        request,
        session: Some(session),
    }))
}

async fn deny_session_request(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<CloseRequest>, JsonRejection>,
) -> Result<Json<crate::state::SessionRequest>, ApiError> {
    let _ = body.map_err(invalid_json)?;
    state
        .finish_session_request(&id, crate::state::RequestStatus::Denied, None)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Unknown session request."))
}

pub(crate) async fn create_session_via_api(
    state: &AppState,
    body: serde_json::Value,
) -> Result<Session, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(format!("http://127.0.0.1:{}/api/sessions", state.port()))
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(response.text().await.unwrap_or_default());
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn mcp_request(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<crate::mcp::RpcRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": -32700, "message": "Invalid JSON-RPC request." }
        }))
        .into_response();
    };
    if state.session(&id).is_none() {
        return Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "error": { "code": -32602, "message": "Unknown session." }
        }))
        .into_response();
    }
    if request.method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => crate::mcp::initialize(request.id),
        "ping" => serde_json::json!({ "jsonrpc": "2.0", "id": request.id, "result": {} }),
        "tools/list" => crate::mcp::tools_list(request.id),
        "tools/call" => {
            let params = request.params.unwrap_or_default();
            let name = params
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let (text, is_error) = crate::mcp::call_tool(&state, &id, name, arguments).await;
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": request.id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": is_error
                }
            })
        }
        other => serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "error": { "code": -32601, "message": format!("Unknown method: {other}") }
        }),
    };

    Json(response).into_response()
}

async fn events(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .try_acquire_events_sse()
        .ok_or_else(|| ApiError::too_many_requests("Too many open event streams."))?;
    let mut receiver = state.subscribe_events();

    let stream = async_stream::stream! {
        let _permit = permit;
        yield Ok(Event::default().comment("connected"));

        let mut heartbeat = interval_at(
            tokio::time::Instant::now() + SSE_HEARTBEAT,
            SSE_HEARTBEAT,
        );
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
                received = receiver.recv() => {
                    match received {
                        Ok(event) => {
                            if let Some(event) = json_event(&event) {
                                yield Ok(event);
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => break,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

async fn question_events(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .try_acquire_events_sse()
        .ok_or_else(|| ApiError::too_many_requests("Too many open event streams."))?;
    let mut receiver = state.subscribe_aux();

    let stream = async_stream::stream! {
        let _permit = permit;
        yield Ok(Event::default().comment("connected"));

        let mut heartbeat = interval_at(
            tokio::time::Instant::now() + SSE_HEARTBEAT,
            SSE_HEARTBEAT,
        );
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
                received = receiver.recv() => {
                    match received {
                        Ok(event) => {
                            if let Some(event) = json_event(&event) {
                                yield Ok(event);
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => break,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

async fn claude_hook(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<HookQuery>, QueryRejection>,
    body: Result<Json<HookInput>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::forbidden("Invalid hook credentials."))?;
    let provided = query.token.as_deref().unwrap_or_default();
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::forbidden("Invalid hook credentials."))?;
    let session = entry.session();
    if session.kind != SessionKind::Agent || session.client != Some(Client::ClaudeCode) {
        return Err(ApiError::forbidden("Invalid hook credentials."));
    }
    let expected = entry
        .claude_token()
        .ok_or_else(|| ApiError::forbidden("Invalid hook credentials."))?;
    if !claude::token_matches(&expected, provided) {
        return Err(ApiError::forbidden("Invalid hook credentials."));
    }

    let Json(input) = body.map_err(|_| ApiError::bad_request("Invalid hook input."))?;

    match input.hook_event_name.as_str() {
        "UserPromptSubmit" => {
            publish_hook_event(&state, entry.mark_working());
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "PostToolUse" => {
            publish_hook_event(&state, entry.finish_tool());
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "Notification" => {
            if is_permission_notification(&input) {
                publish_hook_event(&state, entry.mark_permission_note());
            }
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "Stop" => {
            let preview = hook_preview_text(&input);
            publish_hook_event(&state, entry.mark_done(preview));
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "MessageDisplay" => {
            if let Some(preview) = hook_preview_text(&input) {
                publish_hook_event(&state, entry.hook_preview(&preview));
            }
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        "PreToolUse" => pre_tool_use(&state, &entry, input).await,
        _ => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

struct PermissionGuard<'a> {
    state: &'a AppState,
    entry: &'a SessionEntry,
    request_id: String,
}

impl Drop for PermissionGuard<'_> {
    fn drop(&mut self) {
        if let Some(session) = self.entry.clear_unanswered_permission(&self.request_id) {
            self.state.emit(SessionEvent::updated(session));
        }
    }
}

async fn pre_tool_use(
    state: &AppState,
    entry: &SessionEntry,
    input: HookInput,
) -> Result<Response, ApiError> {
    let tool_name = input.tool_name.as_deref().unwrap_or_default();
    if !claude::tool_needs_card(tool_name) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let request_id = input
        .tool_use_id
        .as_deref()
        .and_then(|id| non_empty_hook_text(id, 128))
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let tool = input
        .tool_name
        .as_deref()
        .and_then(|name| non_empty_hook_text(name, 40))
        .unwrap_or_else(|| "Tool".to_owned());
    let summary = tool_summary(&tool, input.tool_input.as_ref());

    let receiver = match entry.begin_permission(PendingPermission {
        request_id: request_id.clone(),
        tool,
        summary,
    }) {
        BeginPermission::Held(receiver) => receiver,
        BeginPermission::Busy => {
            return Ok(hook_decision(
                HookDecision::Ask,
                "Another permission request is waiting for the user.",
            ));
        }
        BeginPermission::Unavailable => {
            return Ok(hook_decision(
                HookDecision::Deny,
                "The session is not available.",
            ));
        }
    };
    let _guard = PermissionGuard {
        state,
        entry,
        request_id: request_id.clone(),
    };

    match tokio::time::timeout(state.permission_timeout(), receiver).await {
        Ok(Ok(reply)) => Ok(hook_decision(reply.decision.into(), &reply.reason)),
        Ok(Err(_)) => Ok(hook_decision(
            HookDecision::Deny,
            "The permission request was cancelled.",
        )),
        Err(_) => {
            if let Some(session) = entry.clear_unanswered_permission(&request_id) {
                state.emit(SessionEvent::updated(session));
            }
            Ok(hook_decision(
                HookDecision::Deny,
                "No answer from the user. The request expired.",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum HookDecision {
    Allow,
    Deny,
    Ask,
}

impl HookDecision {
    fn as_str(self) -> &'static str {
        match self {
            HookDecision::Allow => "allow",
            HookDecision::Deny => "deny",
            HookDecision::Ask => "ask",
        }
    }
}

impl From<PermissionDecision> for HookDecision {
    fn from(decision: PermissionDecision) -> Self {
        match decision {
            PermissionDecision::Allow => HookDecision::Allow,
            PermissionDecision::Deny => HookDecision::Deny,
        }
    }
}

fn hook_decision(decision: HookDecision, reason: &str) -> Response {
    Json(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision.as_str(),
            "permissionDecisionReason": reason,
        }
    }))
    .into_response()
}

fn publish_hook_event(state: &AppState, session: Option<Session>) {
    if let Some(session) = session {
        state.emit(SessionEvent::updated(session));
    }
}

fn is_permission_notification(input: &HookInput) -> bool {
    if input.notification_type.as_deref() == Some("permission_prompt") {
        return true;
    }
    input
        .message
        .as_ref()
        .and_then(first_hook_string)
        .is_some_and(|message| message.to_lowercase().contains("permission"))
}

fn hook_preview_text(input: &HookInput) -> Option<String> {
    input
        .last_assistant_message
        .clone()
        .or_else(|| {
            input
                .message
                .as_ref()
                .and_then(first_hook_string)
                .map(str::to_owned)
        })
        .or_else(|| {
            input
                .content
                .as_ref()
                .and_then(first_hook_string)
                .map(str::to_owned)
        })
        .or_else(|| {
            input
                .text
                .as_ref()
                .and_then(first_hook_string)
                .map(str::to_owned)
        })
}

fn first_hook_string(value: &serde_json::Value) -> Option<&str> {
    first_hook_string_at(value, 0)
}

fn first_hook_string_at(value: &serde_json::Value, depth: usize) -> Option<&str> {
    if depth >= 4 {
        return None;
    }
    match value {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| first_hook_string_at(item, depth + 1)),
        serde_json::Value::Object(map) => [
            "text",
            "content",
            "message",
            "last_assistant_message",
            "delta",
        ]
        .iter()
        .find_map(|key| {
            map.get(*key)
                .and_then(|item| first_hook_string_at(item, depth + 1))
        }),
        _ => None,
    }
}

fn tool_summary(tool: &str, input: Option<&serde_json::Value>) -> String {
    let Some(input) = input.and_then(serde_json::Value::as_object) else {
        return String::new();
    };

    let preferred = match tool {
        "Bash" | "BashOutput" => ["command"].as_slice(),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Read" => {
            ["file_path", "notebook_path", "path"].as_slice()
        }
        "Glob" | "Grep" => ["pattern", "path"].as_slice(),
        "WebFetch" => ["url"].as_slice(),
        "WebSearch" => ["query"].as_slice(),
        _ => [].as_slice(),
    };
    let fallback = [
        "command",
        "file_path",
        "path",
        "query",
        "url",
        "description",
        "pattern",
        "name",
    ];

    preferred
        .iter()
        .chain(fallback.iter())
        .find_map(|key| input.get(*key).and_then(serde_json::Value::as_str))
        .and_then(|text| non_empty_hook_text(text, 80))
        .unwrap_or_default()
}

fn non_empty_hook_text(value: &str, limit: usize) -> Option<String> {
    let collapsed = value
        .chars()
        .map(|character| {
            if character.is_control() || is_invisible_format(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(limit).collect())
}

fn is_invisible_format(character: char) -> bool {
    let code = character as u32;
    matches!(
        code,
        0x00AD | 0x061C | 0x06DD | 0x070F | 0x08E2 | 0x180E | 0xFEFF | 0x110BD | 0x110CD | 0xE0001
    ) || (0x0600..=0x0605).contains(&code)
        || (0x0890..=0x0891).contains(&code)
        || (0x200B..=0x200F).contains(&code)
        || (0x202A..=0x202E).contains(&code)
        || (0x2060..=0x2064).contains(&code)
        || (0x2066..=0x206F).contains(&code)
        || (0xFFF9..=0xFFFB).contains(&code)
        || (0x13430..=0x1343F).contains(&code)
        || (0x1BCA0..=0x1BCA3).contains(&code)
        || (0x1D173..=0x1D17A).contains(&code)
        || (0xE0020..=0xE007F).contains(&code)
        || (0xE000..=0xF8FF).contains(&code)
        || (0xF0000..=0xFFFFD).contains(&code)
        || (0x100000..=0x10FFFD).contains(&code)
}

async fn not_found() -> ApiError {
    ApiError::not_found("Not found.")
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed("Method not allowed.")
}

fn prepare_terminal(
    state: &AppState,
    request: &CreateSessionRequest,
    project: &Project,
) -> Result<PreparedSession, ApiError> {
    let folder = canonical_directory(requested_folder(request, project))?;

    let command = session_command(state, SessionKind::Terminal, None)?;
    let name = requested_name(request.name.as_deref()).unwrap_or_else(|| folder_name(&folder));
    let short = slugify(&name, "session");
    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        short,
        project_id: project.id.clone(),
        kind: SessionKind::Terminal,
        status: SessionState::Active,
        work_state: Some(WorkState::Working),
        client: None,
        account_id: None,
        branch: None,
        folder: path_string(&folder),
        preview: "Terminal ready.".to_owned(),
        waiting_for: None,
        pending_permission: None,
        first_prompt: None,
        parent_session_id: None,
        parent_session_name: None,
        env_overrides: Vec::new(),
        started_at: timestamp(),
        shortcuts: Vec::new(),
    };

    Ok(PreparedSession {
        session,
        folder,
        command,
        repo_root: None,
        antigravity_account: None,
        environment: Vec::new(),
        startup_input: None,
    })
}

async fn prepare_agent(
    state: &AppState,
    request: &CreateSessionRequest,
    project: &Project,
) -> Result<PreparedSession, ApiError> {
    let account_id = request
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|account| !account.is_empty())
        .ok_or_else(|| ApiError::bad_request("An agent session needs an account."))?;
    let account = state
        .account(account_id)
        .ok_or_else(|| ApiError::not_found("Unknown account."))?;

    let command = session_command(state, SessionKind::Agent, Some(account.client))?;
    let environment = account_environment(state, &account).await?;
    let folder = canonical_directory(requested_folder(request, project))?;
    let repo_root = git::repo_root(folder, state.git_timeout())
        .await
        .ok_or_else(|| ApiError::unprocessable("Agent sessions need a Git repository."))?;
    if state.data_dir().is_none() {
        return Err(ApiError::unprocessable(
            "Agent sessions need a data directory.",
        ));
    }

    let first_prompt = requested_prompt(request.first_prompt.as_deref());
    let name = requested_name(request.name.as_deref())
        .or_else(|| first_prompt.as_deref().map(name_from_prompt))
        .unwrap_or_else(|| format!("{} session", account.client));
    let short = slugify(&name, "session");
    let mut command = command;
    let mut startup_input = None;
    if let Some(prompt) = first_prompt.as_deref() {
        if sessions::client_needs_prompt_input(account.client) {
            startup_input = Some(prompt.to_owned());
        } else {
            for argument in sessions::prompt_arguments(account.client, prompt) {
                command.arg(argument);
            }
        }
    }
    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        short,
        project_id: project.id.clone(),
        kind: SessionKind::Agent,
        status: SessionState::Active,
        work_state: Some(WorkState::Working),
        client: Some(account.client),
        account_id: Some(account.id.clone()),
        branch: None,
        folder: path_string(&repo_root),
        preview: format!("{} is starting.", account.client),
        waiting_for: None,
        pending_permission: None,
        first_prompt,
        parent_session_id: request.parent_session_id.clone(),
        parent_session_name: request.parent_session_name.clone(),
        env_overrides: request.env.clone().unwrap_or_default(),
        started_at: timestamp(),
        shortcuts: shortcuts::for_client(account.client),
    };

    Ok(PreparedSession {
        session,
        folder: repo_root.clone(),
        command,
        repo_root: Some(repo_root),
        antigravity_account: (account.client == Client::Antigravity).then(|| account.id.clone()),
        environment,
        startup_input,
    })
}

fn requested_folder(request: &CreateSessionRequest, project: &Project) -> PathBuf {
    request
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .map(seed::expand_tilde)
        .unwrap_or_else(|| PathBuf::from(&project.folder))
}

fn session_command(
    state: &AppState,
    kind: SessionKind,
    client: Option<Client>,
) -> Result<CommandBuilder, ApiError> {
    if let Some(argv) = state.command_override() {
        if argv.is_empty() {
            return Ok(CommandBuilder::new_default_prog());
        }
        return Ok(CommandBuilder::from_argv(
            argv.iter().map(OsString::from).collect(),
        ));
    }

    match kind {
        SessionKind::Terminal => Ok(CommandBuilder::new_default_prog()),
        SessionKind::Agent => {
            let client = client.ok_or_else(|| {
                ApiError::unprocessable("An agent session needs a supported client.")
            })?;
            if let Some(executable) = state.agent_bin() {
                return Ok(CommandBuilder::new(executable));
            }
            let executable = clients::find_client(client)
                .ok_or_else(|| ApiError::unprocessable(format!("{} is not installed.", client)))?;
            Ok(CommandBuilder::new(executable))
        }
    }
}

fn configure_session_tools(
    state: &AppState,
    entry: &SessionEntry,
    session_id: &str,
    client: Client,
    command: &mut CommandBuilder,
    environment: &mut Vec<(String, String)>,
) -> Result<(), ApiError> {
    let Some(data_dir) = state.data_dir() else {
        return Ok(());
    };
    let url = format!(
        "http://127.0.0.1:{}/internal/mcp/{session_id}",
        state.port()
    );
    let add_args = state.command_override().is_none();
    match client {
        Client::ClaudeCode => {
            let path =
                crate::mcp::write_claude_config(data_dir, session_id, state.port()).map_err(|error| {
                    tracing::error!(session = %session_id, %error, "could not write MCP config");
                    ApiError::unprocessable("Could not prepare session tools.")
                })?;
            if add_args {
                command.arg("--mcp-config");
                command.arg(path.as_os_str());
            }
            entry.set_mcp_config(path);
            if let Ok(plugin) = crate::mcp::write_skill_plugin(data_dir)
                && add_args
            {
                command.arg("--plugin-dir");
                command.arg(plugin.as_os_str());
            }
        }
        Client::Codex => {
            if add_args {
                command.arg("-c");
                command.arg(format!("mcp_servers.elesdee.url=\"{url}\""));
            }
        }
        Client::OpenCode2 => {
            let path = crate::mcp::write_opencode_config(data_dir, session_id, state.port())
                .map_err(|error| {
                    tracing::error!(session = %session_id, %error, "could not write MCP config");
                    ApiError::unprocessable("Could not prepare session tools.")
                })?;
            entry.set_mcp_config(path.clone());
            environment.push((
                "OPENCODE_CONFIG".to_owned(),
                path.to_string_lossy().into_owned(),
            ));
        }
        Client::Antigravity => {}
    }
    Ok(())
}

fn start_opencode_poller(
    state: AppState,
    entry: Arc<SessionEntry>,
    base_url: String,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let session = entry.session();
            if session.status == SessionState::Closed
                || session.client != Some(Client::OpenCode2)
                || entry.process_exited()
            {
                break;
            }
            let Ok(permissions) = crate::opencode::permissions(&base_url).await else {
                continue;
            };
            match permissions.first() {
                Some(permission) => {
                    let already = session
                        .pending_permission
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == permission.id);
                    if !already {
                        entry.mark_external_permission(PendingPermission {
                            request_id: permission.id.clone(),
                            tool: permission.permission.clone(),
                            summary: permission.summary(),
                        });
                    }
                }
                None => {
                    if let Some(pending) = session.pending_permission
                        && let Some(updated) =
                            entry.clear_external_permission(&pending.request_id)
                    {
                        state.emit(SessionEvent::updated(updated));
                    }
                }
            }
        }
    });
}

fn start_codex_runtime(
    entry: &Arc<SessionEntry>,
    command: &mut CommandBuilder,
) -> Result<(), ApiError> {
    let (url, server) = if let Some(url) = crate::codex::external_url() {
        (url, None)
    } else {
        let port = crate::codex::free_port()
            .ok_or_else(|| ApiError::unprocessable("Could not find a port for Codex."))?;
        let server = crate::codex::start_server(port).map_err(|error| {
            tracing::error!(%error, "could not start the Codex app server");
            ApiError::unprocessable("Could not start the Codex app server.")
        })?;
        (server.url.clone(), Some(server))
    };
    crate::codex::launch_args(command, &url);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    entry.set_codex_runtime(crate::sessions::CodexRuntime {
        url: url.clone(),
        sender,
        server,
    });
    tokio::spawn(crate::codex::run(url, Arc::clone(entry), receiver));
    Ok(())
}

fn install_claude_settings(
    state: &AppState,
    entry: &SessionEntry,
    session_id: &str,
) -> Result<PathBuf, ApiError> {
    let data_dir = state
        .data_dir()
        .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
    let settings = claude::write_settings(data_dir, session_id, state.port()).map_err(|error| {
        tracing::error!(session = %session_id, %error, "could not write Claude hook settings");
        ApiError::unprocessable("Could not prepare Claude Code hooks.")
    })?;
    entry.set_claude_settings(settings.token, settings.path.clone());
    Ok(settings.path)
}

fn canonical_directory(folder: PathBuf) -> Result<PathBuf, ApiError> {
    let folder = folder
        .canonicalize()
        .map_err(|_| ApiError::unprocessable("Folder does not exist or is not a directory."))?;
    if folder.is_dir() {
        Ok(folder)
    } else {
        Err(ApiError::unprocessable(
            "Folder does not exist or is not a directory.",
        ))
    }
}

fn requested_size(cols: Option<u16>, rows: Option<u16>) -> Result<TerminalSize, ApiError> {
    TerminalSize::new(
        cols.unwrap_or(TerminalSize::DEFAULT.cols),
        rows.unwrap_or(TerminalSize::DEFAULT.rows),
    )
    .ok_or_else(|| ApiError::bad_request(TerminalSize::range_error()))
}

fn requested_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| clip(name, 64))
}

fn requested_prompt(prompt: Option<&str>) -> Option<String> {
    prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| clip(prompt, 8000))
}

fn validated_env(
    values: Option<Vec<crate::state::EnvOverride>>,
) -> Result<Vec<crate::state::EnvOverride>, ApiError> {
    let Some(values) = values else {
        return Ok(Vec::new());
    };
    if values.len() > 32 {
        return Err(ApiError::bad_request("Too many environment variables."));
    }
    let mut seen = std::collections::HashSet::new();
    for item in &values {
        let name = item.name.trim();
        let valid_name = !name.is_empty()
            && name.chars().count() <= 64
            && name
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_');
        if !valid_name {
            return Err(ApiError::bad_request("An environment variable name is not valid."));
        }
        if item.value.chars().count() > 2000 || item.value.contains('\0') {
            return Err(ApiError::bad_request("An environment variable value is not valid."));
        }
        if !seen.insert(name.to_owned()) {
            return Err(ApiError::bad_request("An environment variable is set twice."));
        }
    }
    Ok(values
        .into_iter()
        .map(|item| crate::state::EnvOverride {
            name: item.name.trim().to_owned(),
            value: item.value,
        })
        .collect())
}

fn name_from_prompt(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or_default();
    let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = words.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '#' | '*' | '>' | '-' | '`')
    });
    if words.is_empty() {
        "Agent session".to_owned()
    } else {
        clip(words, 48)
    }
}

fn safe_branch_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 200
        && !name.starts_with('-')
        && !name.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && !name.contains("..")
        && !name.ends_with('/')
        && !name.ends_with('.')
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "Terminal".to_owned())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn clip(input: &str, limit: usize) -> String {
    let mut output = input.chars().take(limit).collect::<String>();
    if input.chars().count() > limit {
        output.push('…');
    }
    output
}

fn invalid_json(rejection: JsonRejection) -> ApiError {
    ApiError::bad_request(format!("Invalid request body: {rejection}"))
}

fn json_event<T: Serialize>(value: &T) -> Option<Event> {
    serde_json::to_string(value)
        .ok()
        .map(|data| Event::default().data(data))
}
