use std::collections::BTreeMap;
use std::convert::Infallible;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, HOST, ORIGIN, VARY,
};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
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
use crate::files;
use crate::git;
use crate::history::{ClaudeHookPayload as HookInput, HistoryStatus, claude_changes};
use crate::link::{ApproveFailure, PairReject};
use crate::overrides;
use crate::seed;
use crate::sessions::{
    self, BeginPermission, CloseFailure, InputFailure, PermissionAnswerFailure, ResizeFailure,
    SessionEntry, TerminalSize,
};
use crate::shared;
use crate::shortcuts;
use crate::slug::slugify;
use crate::state::{
    AiConfig, AppSettings, AppState, Auth, AuxEvent, Bootstrap, Client, DeleteSessionFailure,
    EnvOverride, HealthResponse, PairedDevice, PendingPermission, PermissionDecision, Project,
    Provider, ProviderView, Question, QuestionFailure, Session, SessionEvent, SessionKind,
    SessionState, SettingsPatch, WorkState,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SSE_HEARTBEAT: Duration = Duration::from_secs(15);
const STARTUP_INPUT_DELAY: Duration = Duration::from_millis(1200);
const PAUSE_INSTRUCTION: &str = "elesdee: This session is out of usage and will pause. Finish safely now: commit all your work to the session branch, write the handoff note with the write_handoff_note tool when it is available, and push the branch to the remote. Then stop and do not start new work.";
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

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
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

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
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

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
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
    config_id: Option<String>,
    /// The old account name for a session. It resolves to the provider's
    /// first config, so a phone or an older client keeps working.
    account_id: Option<String>,
    /// A provider and client pair resolves to a config when no config id is
    /// sent.
    provider_id: Option<String>,
    client: Option<Client>,
    #[serde(default)]
    allowed_config_ids: Option<Vec<String>>,
    #[serde(default)]
    allowed_account_ids: Option<Vec<String>>,
    name: Option<String>,
    folder: Option<String>,
    source_branch: Option<String>,
    #[serde(default)]
    copy_uncommitted_changes: bool,
    first_prompt: Option<String>,
    parent_session_id: Option<String>,
    parent_session_name: Option<String>,
    env: Option<Vec<EnvOverride>>,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateConfigRequest {
    name: Option<String>,
    provider_id: String,
    client: Client,
    model: Option<String>,
    effort: Option<String>,
    args: Option<Vec<String>>,
    env: Option<Vec<EnvOverride>>,
    config: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateConfigRequest {
    name: Option<String>,
    provider_id: Option<String>,
    client: Option<Client>,
    #[serde(default, deserialize_with = "optional_field")]
    model: Option<Option<String>>,
    #[serde(default, deserialize_with = "optional_field")]
    effort: Option<Option<String>>,
    args: Option<Vec<String>>,
    env: Option<Vec<EnvOverride>>,
    config: Option<BTreeMap<String, String>>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloneGitHubRepoRequest {
    full_name: String,
    name: Option<String>,
    parent_folder: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProjectRequest {
    #[serde(default, deserialize_with = "optional_field")]
    default_config_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "optional_field")]
    default_source_branch: Option<Option<String>>,
}

fn optional_field<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn cleaned_optional(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Resolves a config id, or an old provider id to that provider's first config.
fn resolve_config(
    state: &AppState,
    id: &str,
    client: Option<Client>,
) -> Option<(AiConfig, Provider)> {
    if let Some(pair) = state.config_with_provider(id) {
        return Some(pair);
    }
    let configs = state.configs();
    let config = configs
        .iter()
        .find(|config| config.provider_id == id && client.is_some_and(|c| c == config.client))
        .or_else(|| configs.iter().find(|config| config.provider_id == id))
        .cloned()?;
    let provider = state.provider(&config.provider_id)?;
    Some((config, provider))
}

fn resolve_config_id(state: &AppState, id: &str) -> Option<String> {
    resolve_config(state, id, None).map(|(config, _)| config.id)
}

fn validated_allowed_configs(
    state: &AppState,
    raw: &[String],
    current_config_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut ids: Vec<String> = Vec::new();
    for value in raw {
        let id = value.trim();
        if id.is_empty() {
            return Err(ApiError::bad_request("An allowed config id is empty."));
        }
        if !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_owned());
        }
    }
    if ids.is_empty() {
        return Err(ApiError::bad_request("Allow at least one AI config."));
    }
    for id in &ids {
        if state.config(id).is_none() {
            return Err(ApiError::unprocessable("Unknown AI config."));
        }
    }
    if !ids.iter().any(|id| id == current_config_id) {
        return Err(ApiError::unprocessable(
            "The current AI config must stay allowed.",
        ));
    }
    Ok(ids)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProviderRequest {
    name: Option<String>,
    vendor: String,
    auth: Auth,
    key: Option<String>,
    server: Option<String>,
    api_style: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProviderRequest {
    name: Option<String>,
    vendor: Option<String>,
    auth: Option<Auth>,
    /// Absent or blank keeps the key that is already stored.
    key: Option<String>,
    server: Option<Option<String>>,
    api_style: Option<Option<String>>,
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
struct FolderQuery {
    folder: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitStatusResponse {
    git: bool,
    dirty: bool,
}

#[derive(Debug, Deserialize)]
struct InputRequest {
    data: String,
}

#[derive(Debug, Deserialize)]
struct CloseRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchConfigRequest {
    config_id: String,
}

#[derive(Debug, Deserialize)]
struct UsageRequest {
    rows: Vec<crate::state::UsageRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SwitchOptions {
    /// Every AI config on this computer. The allowed list says which ones the
    /// session may use.
    configs: Vec<AiConfig>,
    /// The config ids the session may use, including the current one.
    allowed_config_ids: Vec<String>,
    /// The allowed config the automatic switch would move to next, if one has
    /// room.
    next_config_id: Option<String>,
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
#[serde(rename_all = "camelCase")]
struct RenameRequest {
    name: Option<String>,
    #[serde(default)]
    allowed_config_ids: Option<Vec<String>>,
    #[serde(default)]
    allowed_account_ids: Option<Vec<String>>,
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
struct OutputQuery {
    after: Option<String>,
}

struct PreparedSession {
    session: Session,
    folder: PathBuf,
    command: CommandBuilder,
    repo_root: Option<PathBuf>,
    antigravity_provider: Option<String>,
    environment: Vec<(String, String)>,
    startup_input: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WriteFileRequest {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserOpenRequest {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserTabRequest {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserScreenshotQuery {
    tab: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let streaming = Router::new()
        .route("/api/sessions/{id}/output", get(session_output))
        .route("/api/sessions/{id}/history", get(session_history))
        .route("/api/events", get(events))
        .route("/api/questions/events", get(question_events))
        .route("/internal/hooks/claude/{id}", post(claude_hook));

    let regular = Router::new()
        .route("/api/health", get(health))
        .route("/api/clients/{client}/install", post(start_client_install))
        .route("/api/clients/install/{id}", get(client_install))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", patch(update_project))
        .route("/api/github/status", get(github_status))
        .route("/api/github/device", post(github_start_device))
        .route(
            "/api/github/device/{id}",
            get(github_device_status).delete(github_cancel_device),
        )
        .route("/api/github/sign-out", post(github_sign_out))
        .route("/api/github/repos", get(github_repos))
        .route("/api/github/clone", post(github_clone))
        .route("/api/branches", get(list_branches))
        .route("/api/git-status", get(git_status))
        .route("/api/provider-options", get(provider_options))
        .route("/api/providers", get(list_providers).post(create_provider))
        .route(
            "/api/providers/{id}",
            patch(update_provider).delete(delete_provider),
        )
        .route("/api/providers/{id}/models", get(provider_models))
        .route(
            "/api/providers/{id}/usage",
            get(provider_usage).post(update_provider_usage),
        )
        .route(
            "/api/providers/{id}/usage/refresh",
            post(refresh_provider_usage),
        )
        .route("/api/sign-in", post(start_sign_in))
        .route(
            "/api/sign-in/{id}",
            get(sign_in_status).delete(cancel_sign_in),
        )
        .route("/api/sign-in/{id}/code", post(submit_sign_in_code))
        .route("/api/accounts", get(list_legacy_accounts))
        .route("/internal/statusline/claude/{id}", post(claude_statusline))
        .route("/api/configs", get(list_configs).post(create_config))
        .route(
            "/api/configs/{id}",
            patch(update_config).delete(delete_config),
        )
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
        .route(
            "/api/pairing",
            post(create_pairing)
                .get(pairing_status)
                .delete(cancel_pairing),
        )
        .route("/api/pairing/qr", get(pairing_qr))
        .route("/api/pairing/approve", post(approve_pairing))
        .route("/api/pairing/deny", post(deny_pairing))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}", delete(remove_device))
        .route("/api/settings", patch(update_settings))
        .route("/api/diagnostics/export", post(export_diagnostics))
        .route("/api/demo-servers", get(list_demo_servers))
        .route(
            "/api/sessions/{id}/demo-servers/{server_id}/stop",
            post(stop_demo_server),
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
        .route("/api/sessions/{id}/config", post(switch_session_config))
        .route(
            "/api/sessions/{id}/account",
            post(switch_session_account_alias),
        )
        .route("/api/sessions/{id}/switch-options", get(switch_options))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route("/api/sessions/{id}/permission", post(answer_permission))
        .route("/api/sessions/{id}/files", get(list_session_files))
        .route(
            "/api/sessions/{id}/file",
            get(read_session_file).post(write_session_file),
        )
        .route("/api/sessions/{id}/changes", get(session_changes))
        .route("/api/sessions/{id}/shared", get(list_shared_files))
        .route("/api/sessions/{id}/shared/{file_id}", get(read_shared_file))
        .route("/api/sessions/{id}/browser", get(browser_state))
        .route("/api/sessions/{id}/browser/start", post(browser_start))
        .route("/api/sessions/{id}/browser/stop", post(browser_stop))
        .route("/api/sessions/{id}/browser/setup", post(browser_setup))
        .route("/api/sessions/{id}/browser/open", post(browser_open))
        .route("/api/sessions/{id}/browser/back", post(browser_back))
        .route("/api/sessions/{id}/browser/forward", post(browser_forward))
        .route("/api/sessions/{id}/browser/reload", post(browser_reload))
        .route("/api/sessions/{id}/browser/tabs", post(browser_new_tab))
        .route(
            "/api/sessions/{id}/browser/tabs/{tab}/activate",
            post(browser_activate),
        )
        .route(
            "/api/sessions/{id}/browser/tabs/{tab}/close",
            post(browser_close_tab),
        )
        .route(
            "/api/sessions/{id}/browser/screenshot",
            get(browser_screenshot),
        )
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

    if (request.method() == Method::POST || request.method() == Method::PATCH)
        && let Err(error) = validate_content_type(&request)
    {
        let mut response = error.into_response();
        apply_cors(&mut response, origin.as_deref(), false);
        return security_headers(response);
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

#[derive(Deserialize)]
struct InstallRequest {}

async fn start_client_install(
    State(state): State<AppState>,
    UrlPath(client): UrlPath<String>,
    Json(_request): Json<InstallRequest>,
) -> Result<(StatusCode, Json<clients::ClientInstall>), ApiError> {
    let client =
        Client::from_slug(&client).ok_or_else(|| ApiError::not_found("Unknown client."))?;
    let data_dir = state
        .data_dir()
        .ok_or_else(|| ApiError::unprocessable("Client installation needs a data directory."))?
        .to_path_buf();
    match clients::start_install(client, data_dir) {
        Ok(install) => Ok((StatusCode::ACCEPTED, Json(install))),
        Err(clients::InstallStartError::Busy) => Err(ApiError::conflict(
            "An install is already running for this client.",
        )),
        Err(clients::InstallStartError::NoDataDirectory) => Err(ApiError::unprocessable(
            "Client installation needs a data directory.",
        )),
    }
}

async fn client_install(
    UrlPath(id): UrlPath<String>,
) -> Result<Json<clients::ClientInstall>, ApiError> {
    clients::install_status(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Unknown install."))
}

async fn bootstrap(State(state): State<AppState>) -> Json<Bootstrap> {
    Json(state.bootstrap())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingCreated {
    expires_at: u64,
    relay_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket: Option<String>,
}

async fn create_pairing(State(state): State<AppState>) -> Result<Json<PairingCreated>, ApiError> {
    let created = state
        .link()
        .create_ticket(crate::link::now_unix(), true)
        .await
        .map_err(pairing_error)?;
    let debug_ticket = cfg!(debug_assertions) && std::env::var_os("ELESDEE_DEV").is_some();
    Ok(Json(PairingCreated {
        expires_at: created.expires_at,
        relay_ready: created.relay_ready,
        ticket: debug_ticket.then_some(created.ticket),
    }))
}

fn pairing_error(reject: PairReject) -> ApiError {
    match reject {
        PairReject::NotReady => ApiError::service_unavailable(reject.message()),
        PairReject::InvalidRequest(_) => ApiError::internal(reject.message()),
        _ => ApiError::conflict(reject.message()),
    }
}

async fn pairing_status(State(state): State<AppState>) -> Json<crate::link::PairingSnapshot> {
    Json(state.link().snapshot(crate::link::now_unix()))
}

async fn pairing_qr(State(state): State<AppState>) -> Result<Response, ApiError> {
    let Some(ticket) = state.link().active_ticket(crate::link::now_unix()) else {
        return Err(ApiError::not_found("There is no active pairing code."));
    };
    let png = crate::link::qr::ticket_png(&ticket).map_err(|error| {
        tracing::error!(%error, "could not render the pairing QR code");
        ApiError::internal("Could not render the pairing code.")
    })?;
    Ok(([(CONTENT_TYPE, HeaderValue::from_static("image/png"))], png).into_response())
}

async fn approve_pairing(State(state): State<AppState>) -> Result<Json<PairedDevice>, ApiError> {
    match state.link().approve(&state) {
        Ok(device) => Ok(Json(device)),
        Err(ApproveFailure::NoPending) => {
            Err(ApiError::conflict("No phone is waiting for confirmation."))
        }
        Err(ApproveFailure::DeviceLimit) => Err(ApiError::conflict(
            "This computer reached its paired phone limit.",
        )),
    }
}

async fn deny_pairing(State(state): State<AppState>) -> StatusCode {
    state.link().deny();
    StatusCode::NO_CONTENT
}

async fn cancel_pairing(State(state): State<AppState>) -> StatusCode {
    state.link().cancel();
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceView {
    #[serde(flatten)]
    device: PairedDevice,
    connected: bool,
}

async fn list_devices(State(state): State<AppState>) -> Json<Vec<DeviceView>> {
    let link = state.link();
    Json(
        state
            .paired_devices()
            .into_iter()
            .map(|device| DeviceView {
                connected: link.is_connected(&device.endpoint_id),
                device,
            })
            .collect(),
    )
}

async fn remove_device(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    if !state.remove_paired_device(&id) {
        return Err(ApiError::not_found("Unknown device."));
    }
    state.save();
    state.link().mark_revoked(&id);
    state.link().revoke_connections(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn update_settings(
    State(state): State<AppState>,
    body: Result<Json<SettingsPatch>, JsonRejection>,
) -> Result<Json<AppSettings>, ApiError> {
    let Json(patch) = body.map_err(invalid_json)?;
    Ok(Json(state.update_settings(patch)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportDiagnosticsRequest {
    folder: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportDiagnosticsResponse {
    path: String,
}

async fn export_diagnostics(
    State(state): State<AppState>,
    body: Result<Json<ExportDiagnosticsRequest>, JsonRejection>,
) -> Result<Json<ExportDiagnosticsResponse>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let folder = PathBuf::from(request.folder.trim());
    if !folder.is_absolute() {
        return Err(ApiError::bad_request("The folder path must be absolute."));
    }
    if !folder.is_dir() {
        return Err(ApiError::unprocessable("The folder does not exist."));
    }
    let report = crate::diagnostics::DiagnosticsReport::collect(&state);
    let path =
        tokio::task::spawn_blocking(move || crate::diagnostics::write_bundle(&folder, &report))
            .await
            .map_err(|_| ApiError::internal("The diagnostics export failed."))?
            .map_err(|error| {
                tracing::error!(%error, "diagnostics export failed");
                ApiError::internal("The diagnostics export failed.")
            })?;
    Ok(Json(ExportDiagnosticsResponse {
        path: path.display().to_string(),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DemoServerEntry {
    #[serde(flatten)]
    server: crate::demo::DemoServer,
    session_name: String,
}

async fn list_demo_servers(State(state): State<AppState>) -> Json<Vec<DemoServerEntry>> {
    Json(
        state
            .active_demo_servers()
            .into_iter()
            .map(|(session_name, server)| DemoServerEntry {
                server,
                session_name,
            })
            .collect(),
    )
}

async fn stop_demo_server(
    State(state): State<AppState>,
    UrlPath((id, server_id)): UrlPath<(String, String)>,
) -> Result<Json<Session>, ApiError> {
    let (session, port) = state
        .stop_demo_server(&id, &server_id)
        .map_err(demo_error)?;
    state.link().close_forward(port);
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

fn demo_error(error: crate::demo::DemoFailure) -> ApiError {
    match error {
        crate::demo::DemoFailure::UnknownSession | crate::demo::DemoFailure::NotFound => {
            ApiError::not_found(error.to_string())
        }
        crate::demo::DemoFailure::Closed => ApiError::conflict(error.to_string()),
        _ => ApiError::unprocessable(error.to_string()),
    }
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

async fn git_status(
    State(state): State<AppState>,
    query: Result<Query<FolderQuery>, QueryRejection>,
) -> Result<Json<GitStatusResponse>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid folder query."))?;
    let folder = canonical_directory(seed::expand_tilde(query.folder.trim()))?;
    let Some(repo_root) = git::repo_root(folder, state.git_timeout()).await else {
        return Ok(Json(GitStatusResponse {
            git: false,
            dirty: false,
        }));
    };
    let dirty = git::has_uncommitted_changes(repo_root, state.git_timeout())
        .await
        .unwrap_or(false);
    Ok(Json(GitStatusResponse { git: true, dirty }))
}

async fn provider_options() -> Json<Vec<ProviderOption>> {
    Json(
        crate::providers::PROVIDERS
            .iter()
            .map(|provider| ProviderOption {
                id: provider.id.to_owned(),
                label: provider.label.to_owned(),
                auth: provider.auth.to_vec(),
                supported_clients: provider.clients.to_vec(),
                efforts: provider
                    .clients
                    .iter()
                    .map(|client| {
                        (
                            client.display_name().to_owned(),
                            crate::providers::effort_levels(*client)
                                .iter()
                                .map(|level| (*level).to_owned())
                                .collect(),
                        )
                    })
                    .collect(),
                default_model: provider.default_model.map(str::to_owned),
                custom: provider.custom,
                sign_in_client: crate::providers::sign_in_client(provider.id),
            })
            .collect(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderOption {
    id: String,
    label: String,
    auth: Vec<Auth>,
    supported_clients: Vec<Client>,
    efforts: BTreeMap<String, Vec<String>>,
    default_model: Option<String>,
    custom: bool,
    /// The client that owns this vendor's subscription login.
    sign_in_client: Option<Client>,
}

async fn list_providers(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<ProviderView>>, ApiError> {
    let filter = list_filter(query)?;
    let mut providers = state.provider_views();
    if let Some(filter) = filter.as_deref() {
        providers.retain(|provider| {
            matches_filter(&provider.provider.name, filter)
                || matches_filter(&provider.provider.vendor, filter)
                || matches_filter(&provider.provider.plan, filter)
        });
    }
    providers.sort_by_cached_key(|provider| {
        (
            provider.provider.name.to_lowercase(),
            provider.provider.id.clone(),
        )
    });
    Ok(Json(providers))
}

/// The phone app lists accounts by the old name. It gets the providers.
async fn list_legacy_accounts(State(state): State<AppState>) -> Json<Vec<ProviderView>> {
    Json(state.provider_views())
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

struct ProviderFields {
    name: String,
    vendor: String,
    auth: Auth,
    server: Option<String>,
    api_style: Option<String>,
    plan: &'static str,
}

fn validated_provider_fields(
    name: Option<&str>,
    vendor: &str,
    auth: Auth,
    server: Option<&str>,
    api_style: Option<&str>,
) -> Result<ProviderFields, ApiError> {
    let vendor = vendor.trim().to_ascii_lowercase();
    let definition = crate::providers::find(&vendor)
        .ok_or_else(|| ApiError::unprocessable("This provider is not supported."))?;
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(definition.label);
    if name.chars().count() > 80 {
        return Err(ApiError::bad_request(
            "A provider name must be 1 to 80 characters.",
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(ApiError::bad_request("The provider name is not valid."));
    }
    if !definition.auth.contains(&auth) {
        return Err(ApiError::unprocessable(
            "This provider does not support that authentication method.",
        ));
    }

    let api_style = api_style
        .map(str::trim)
        .map(str::to_lowercase)
        .filter(|style| !style.is_empty());
    if definition.custom && api_style.is_none() {
        return Err(ApiError::bad_request("Choose an API style."));
    }
    if let Some(style) = api_style.as_deref()
        && !matches!(style, "openai" | "anthropic")
    {
        return Err(ApiError::bad_request("The API style is not valid."));
    }

    let needs_server =
        definition.custom || auth == Auth::Local || matches!(definition.id, "ollama" | "lmstudio");
    let server = server
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

    let plan = match auth {
        Auth::Subscription => "subscription",
        Auth::Local => "local",
        Auth::Key => "pay as you go",
    };

    Ok(ProviderFields {
        name: name.to_owned(),
        vendor,
        auth,
        server: if needs_server { server } else { None },
        api_style: if definition.custom { api_style } else { None },
        plan,
    })
}

async fn create_provider(
    State(state): State<AppState>,
    body: Result<Json<CreateProviderRequest>, JsonRejection>,
) -> Result<Json<ProviderView>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let fields = validated_provider_fields(
        request.name.as_deref(),
        &request.vendor,
        request.auth,
        request.server.as_deref(),
        request.api_style.as_deref(),
    )?;
    let key = request
        .key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());
    if fields.auth == Auth::Key && key.is_none() {
        return Err(ApiError::bad_request("API key is required."));
    }

    let provider = state.add_provider(Provider {
        id: String::new(),
        name: fields.name,
        vendor: fields.vendor,
        auth: fields.auth,
        server: fields.server,
        api_style: fields.api_style,
        sample: false,
        connected: fields.auth != Auth::Subscription,
        plan: fields.plan.to_owned(),
        rows: Vec::new(),
        foot: "No official usage source.".to_owned(),
        usage_read_at: None,
        usage_checked_at: None,
        usage_status: None,
        usage_source: None,
        limit_reported_at: None,
    });
    if fields.auth == Auth::Key
        && let Some(key) = key
        && let Err(error) = store_account_key(&state, &provider.id, key).await
    {
        state.remove_provider(&provider.id);
        tracing::error!(provider = %provider.id, %error, "could not store provider key");
        return Err(ApiError::internal("Could not store the provider key."));
    }
    state.save();
    Ok(Json(ProviderView::new(provider)))
}

async fn update_provider(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Result<Json<ProviderView>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let existing = state
        .provider(&id)
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;

    let fields = validated_provider_fields(
        request.name.as_deref().or(Some(&existing.name)),
        request.vendor.as_deref().unwrap_or(&existing.vendor),
        request.auth.unwrap_or(existing.auth),
        request
            .server
            .as_ref()
            .map(|value| value.as_deref())
            .unwrap_or(existing.server.as_deref()),
        request
            .api_style
            .as_ref()
            .map(|value| value.as_deref())
            .unwrap_or(existing.api_style.as_deref()),
    )?;

    let replacement_key = if fields.auth == Auth::Key {
        request
            .key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    } else {
        None
    };
    if fields.auth == Auth::Key
        && replacement_key.is_none()
        && !account_key_available(&state, &id).await?
    {
        return Err(ApiError::bad_request("API key is required."));
    }

    let candidate = Provider {
        id: id.clone(),
        name: fields.name,
        vendor: fields.vendor,
        auth: fields.auth,
        server: fields.server,
        api_style: fields.api_style,
        sample: false,
        connected: if fields.auth == Auth::Subscription {
            existing.connected
        } else {
            true
        },
        plan: fields.plan.to_owned(),
        rows: existing.rows.clone(),
        foot: existing.foot.clone(),
        usage_read_at: existing.usage_read_at.clone(),
        usage_checked_at: existing.usage_checked_at.clone(),
        usage_status: existing.usage_status,
        usage_source: existing.usage_source.clone(),
        limit_reported_at: existing.limit_reported_at.clone(),
    };
    let supported = crate::providers::supported_clients(&candidate);
    if let Some(config) = state
        .configs()
        .into_iter()
        .find(|config| config.provider_id == id && !supported.contains(&config.client))
    {
        return Err(ApiError::conflict(format!(
            "{} runs on {} through this provider. Remove or change it first.",
            config.name, config.client
        )));
    }

    if let Some(key) = replacement_key
        && let Err(error) = store_account_key(&state, &id, key).await
    {
        tracing::error!(provider = %id, %error, "could not store provider key");
        return Err(ApiError::internal("Could not store the provider key."));
    }

    let identity_changed = existing.vendor != candidate.vendor
        || existing.server != candidate.server
        || existing.api_style != candidate.api_style;
    let mut updated = candidate;
    if identity_changed {
        updated.rows.clear();
        updated.foot = "No official usage source.".to_owned();
        updated.usage_read_at = None;
        updated.usage_checked_at = None;
        updated.usage_status = None;
        updated.usage_source = None;
        updated.limit_reported_at = None;
    }
    let updated = state
        .replace_provider(updated)
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;
    state.save();
    state.publish_provider(updated.clone());
    Ok(Json(ProviderView::new(updated)))
}

async fn delete_provider(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    if state.provider(&id).is_none() {
        return Err(ApiError::not_found("Unknown provider."));
    }
    let used = state.config_count_for_provider(&id);
    if used > 0 {
        let message = if used == 1 {
            "An AI config uses this provider. Remove it first.".to_owned()
        } else {
            format!("{used} AI configs use this provider. Remove them first.")
        };
        return Err(ApiError::conflict(message));
    }
    if let Err(error) = delete_account_key(&state, &id).await {
        tracing::error!(provider = %id, %error, "could not delete provider key");
        return Err(ApiError::internal("Could not delete the provider key."));
    }
    state
        .remove_provider(&id)
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;
    state.save();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelQuery {
    client: String,
}

const MODEL_CACHE_TTL: Duration = Duration::from_secs(60);

async fn provider_models(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<ModelQuery>, QueryRejection>,
) -> Result<Json<crate::model_list::ModelList>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Choose a client."))?;
    let client: Client = serde_json::from_value(serde_json::Value::String(query.client.clone()))
        .map_err(|_| ApiError::bad_request("The client is not valid."))?;
    let provider = state
        .provider(&id)
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;
    if !crate::providers::supports_client(&provider, client) {
        return Err(ApiError::unprocessable(
            "This provider does not support that client.",
        ));
    }

    let cache_key = format!("{}:{}", provider.id, client.display_name());
    if let Some(models) = state.cached_models(&cache_key, MODEL_CACHE_TTL) {
        return Ok(Json(crate::model_list::ModelList {
            models,
            source: "cache".to_owned(),
            fallback: false,
        }));
    }

    let gate = state.model_gate(&cache_key);
    let _guard = gate.lock().await;
    if let Some(models) = state.cached_models(&cache_key, MODEL_CACHE_TTL) {
        return Ok(Json(crate::model_list::ModelList {
            models,
            source: "cache".to_owned(),
            fallback: false,
        }));
    }

    let key = if provider.auth == Auth::Key {
        account_key(&state, &provider.id).await.ok()
    } else {
        None
    };
    match crate::model_list::fetch(&provider, client, key.as_deref()).await {
        Ok(models) => {
            state.cache_models(&cache_key, models.clone());
            Ok(Json(crate::model_list::ModelList {
                models,
                source: "provider".to_owned(),
                fallback: false,
            }))
        }
        Err(()) => {
            tracing::info!(
                provider = %provider.id,
                client = %client,
                "the provider model read failed; using the known list"
            );
            Ok(Json(crate::model_list::ModelList::known(&provider.vendor)))
        }
    }
}

async fn provider_usage(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<ProviderView>, ApiError> {
    state
        .provider(&id)
        .map(ProviderView::new)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Unknown provider."))
}

async fn update_provider_usage(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<UsageRequest>, JsonRejection>,
) -> Result<Json<ProviderView>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    if request.rows.len() > 16 {
        return Err(ApiError::bad_request("Too many usage rows."));
    }
    if request
        .rows
        .iter()
        .any(|row| row.label.chars().count() > 80)
    {
        return Err(ApiError::bad_request("A usage label is too long."));
    }
    let provider = crate::usage::apply_usage(&state, &id, crate::usage::MANUAL, request.rows)
        .await
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;
    Ok(Json(ProviderView::new(provider)))
}

async fn refresh_provider_usage(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<ProviderView>, ApiError> {
    match crate::usage::refresh_provider(&state, &id, true).await {
        Ok(provider) => Ok(Json(ProviderView::new(provider))),
        Err(crate::usage::RefreshError::Unknown) => Err(ApiError::not_found("Unknown provider.")),
        Err(crate::usage::RefreshError::Read) => {
            Err(ApiError::bad_gateway("The usage read failed."))
        }
    }
}

async fn start_sign_in(
    State(state): State<AppState>,
    body: Result<Json<crate::auth::SignInRequest>, JsonRejection>,
) -> Result<Json<crate::auth::SignInView>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    crate::auth::start(&state, request)
        .await
        .map(Json)
        .map_err(sign_in_error)
}

async fn sign_in_status(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::auth::SignInView>, ApiError> {
    crate::auth::view(&state, &id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Unknown sign-in."))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitCodeRequest {
    code: String,
}

async fn submit_sign_in_code(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<SubmitCodeRequest>, JsonRejection>,
) -> Result<Json<crate::auth::SignInView>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    crate::auth::submit_code(&state, &id, &request.code)
        .await
        .map(Json)
        .map_err(sign_in_error)
}

async fn cancel_sign_in(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    crate::auth::cancel(&state, &id)
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or_else(|| ApiError::not_found("Unknown sign-in."))
}

fn sign_in_error(error: crate::auth::SignInError) -> ApiError {
    match error {
        crate::auth::SignInError::UnsupportedVendor => {
            ApiError::unprocessable("This provider does not support subscription sign-in.")
        }
        crate::auth::SignInError::InvalidName => {
            ApiError::bad_request("A provider name must be 1 to 80 characters.")
        }
        crate::auth::SignInError::UnknownProvider => ApiError::not_found("Unknown provider."),
        crate::auth::SignInError::AlreadyRunning => {
            ApiError::conflict("A sign-in is already running.")
        }
        crate::auth::SignInError::UnknownAttempt => ApiError::not_found("Unknown sign-in."),
        crate::auth::SignInError::InvalidCode => {
            ApiError::bad_request("The one-time code is not valid.")
        }
        crate::auth::SignInError::NotWaiting => {
            ApiError::conflict("The sign-in is not waiting for a code.")
        }
        crate::auth::SignInError::Internal => ApiError::internal("The sign-in could not start."),
    }
}

async fn list_configs(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<AiConfig>>, ApiError> {
    let filter = list_filter(query)?;
    let providers = state.providers();
    let mut configs = state.configs();
    if let Some(filter) = filter.as_deref() {
        configs.retain(|config| {
            let provider = providers
                .iter()
                .find(|provider| provider.id == config.provider_id);
            matches_filter(&config.name, filter)
                || matches_filter(config.client.display_name(), filter)
                || config
                    .model
                    .as_deref()
                    .is_some_and(|model| matches_filter(model, filter))
                || provider.is_some_and(|provider| {
                    matches_filter(&provider.name, filter)
                        || matches_filter(&provider.vendor, filter)
                })
        });
    }
    configs.sort_by_cached_key(|config| (config.name.to_lowercase(), config.id.clone()));
    Ok(Json(configs))
}

fn validated_name(value: &str, kind: &str) -> Result<String, ApiError> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request(format!(
            "An AI config name must be 1 to 64 characters ({kind})."
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ApiError::bad_request("The AI config name is not valid."));
    }
    Ok(name.to_owned())
}

fn validated_model(client: Client, value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(model) = cleaned_optional(value) else {
        return Ok(None);
    };
    if model.chars().count() > 120 {
        return Err(ApiError::bad_request("The model name is too long."));
    }
    let valid = model.chars().all(|character| match client {
        Client::Antigravity => {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        }
        _ => {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | '-' | ':' | '/' | '@' | '+')
        }
    }) && !model
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '.' | '-' | ':' | '/'));
    if !valid {
        return Err(ApiError::unprocessable(
            "The model is not valid for this client.",
        ));
    }
    Ok(Some(model))
}

fn validated_effort(client: Client, value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(effort) = cleaned_optional(value) else {
        return Ok(None);
    };
    if !crate::providers::valid_effort(client, &effort) {
        return Err(ApiError::unprocessable(
            "The effort level is not valid for this client.",
        ));
    }
    Ok(Some(effort))
}

fn validated_args(args: Option<Vec<String>>) -> Result<Vec<String>, ApiError> {
    let Some(args) = args else {
        return Ok(Vec::new());
    };
    if args.len() > 64 {
        return Err(ApiError::bad_request("Too many command-line arguments."));
    }
    let mut cleaned = Vec::with_capacity(args.len());
    for argument in args {
        if argument.chars().count() > 2000 || argument.contains('\0') {
            return Err(ApiError::bad_request(
                "A command-line argument is not valid.",
            ));
        }
        cleaned.push(argument);
    }
    Ok(cleaned)
}

async fn create_config(
    State(state): State<AppState>,
    body: Result<Json<CreateConfigRequest>, JsonRejection>,
) -> Result<Json<AiConfig>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let provider_id = request.provider_id.trim();
    let provider = state
        .provider(provider_id)
        .ok_or_else(|| ApiError::unprocessable("The provider does not exist."))?;
    if !crate::providers::supports_client(&provider, request.client) {
        return Err(ApiError::unprocessable(
            "This provider does not support that client.",
        ));
    }
    let fallback = format!("{} ({})", provider.name, request.client);
    let name = match cleaned_optional(request.name) {
        Some(name) => validated_name(&name, "name")?,
        None => validated_name(&fallback, "default name")?,
    };
    let default_model = crate::providers::find(&provider.vendor)
        .and_then(|definition| definition.default_model)
        .map(str::to_owned);
    let model = validated_model(request.client, request.model.or(default_model))?;
    let effort = validated_effort(request.client, request.effort)?;
    let args = validated_args(request.args)?;
    let env = validated_env(request.env)?;
    let config = validated_config(request.config)?;

    let config = state.add_config(AiConfig {
        id: String::new(),
        name,
        provider_id: provider.id,
        client: request.client,
        model,
        effort,
        args,
        env,
        config,
        created_at: String::new(),
        updated_at: String::new(),
    });
    state.save();
    Ok(Json(config))
}

async fn update_config(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<UpdateConfigRequest>, JsonRejection>,
) -> Result<Json<AiConfig>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let existing = state
        .config(&id)
        .ok_or_else(|| ApiError::not_found("Unknown AI config."))?;
    let provider_id = request
        .provider_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(&existing.provider_id)
        .to_owned();
    let provider = state
        .provider(&provider_id)
        .ok_or_else(|| ApiError::unprocessable("The provider does not exist."))?;
    let client = request.client.unwrap_or(existing.client);
    if !crate::providers::supports_client(&provider, client) {
        return Err(ApiError::unprocessable(
            "This provider does not support that client.",
        ));
    }

    let name = request
        .name
        .as_deref()
        .map(|name| validated_name(name, "name"))
        .transpose()?
        .unwrap_or(existing.name);
    let model = match request.model {
        Some(value) => validated_model(client, value)?,
        None => validated_model(client, existing.model)?,
    };
    let effort = match request.effort {
        Some(value) => validated_effort(client, value)?,
        None => existing.effort,
    };
    let args = match request.args {
        Some(args) => validated_args(Some(args))?,
        None => existing.args,
    };
    let env = match request.env {
        Some(env) => validated_env(Some(env))?,
        None => existing.env,
    };
    let config = match request.config {
        Some(config) => validated_config(Some(config))?,
        None => existing.config,
    };

    let updated = state
        .replace_config(AiConfig {
            id,
            name,
            provider_id: provider.id,
            client,
            model,
            effort,
            args,
            env,
            config,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
        })
        .ok_or_else(|| ApiError::not_found("Unknown AI config."))?;
    state.save();
    Ok(Json(updated))
}

async fn delete_config(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    if state.config(&id).is_none() {
        return Err(ApiError::not_found("Unknown AI config."));
    }
    let blockers = state
        .sessions()
        .into_iter()
        .map(|entry| entry.session())
        .filter(|session| {
            session.status == crate::state::SessionState::Active
                && session.config_id.as_deref() == Some(id.as_str())
        })
        .map(|session| session.name)
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        let message = if blockers.len() == 1 {
            format!(
                "{} is using this AI config. Switch or close it first.",
                blockers[0]
            )
        } else {
            format!(
                "{} active sessions use this AI config. Switch or close them first.",
                blockers.len()
            )
        };
        return Err(ApiError::conflict(message));
    }
    state
        .remove_config(&id)
        .ok_or_else(|| ApiError::not_found("Unknown AI config."))?;
    state.save();
    Ok(StatusCode::NO_CONTENT)
}

fn validated_config(
    config: Option<BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, ApiError> {
    let Some(config) = config else {
        return Ok(BTreeMap::new());
    };
    if config.len() > 32 {
        return Err(ApiError::bad_request("Too many config overrides."));
    }
    for (key, value) in &config {
        let valid_key = !key.is_empty()
            && key.chars().count() <= 120
            && key.split('.').all(|segment| {
                !segment.is_empty()
                    && segment.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                    })
            });
        if !valid_key {
            return Err(ApiError::bad_request("A config key is not valid."));
        }
        if value.chars().count() > 2000 || value.contains('\0') {
            return Err(ApiError::bad_request("A config value is not valid."));
        }
        if key.split('.').any(secret_like_name) {
            return Err(ApiError::unprocessable(
                "API keys belong in an account, not in a config value.",
            ));
        }
    }
    Ok(config)
}

fn secret_like_name(name: &str) -> bool {
    let compact = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    compact.contains("APIKEY")
        || compact.contains("AUTHTOKEN")
        || compact.contains("ACCESSTOKEN")
        || compact.contains("PRIVATEKEY")
        || compact.contains("PASSWORD")
        || compact == "SECRET"
        || compact.ends_with("SECRET")
}

/// Adds the model, effort and config arguments for a client.
///
/// OpenCode 2 and Antigravity read these values from their generated launch
/// files, so only their extra arguments are added here.
pub(crate) fn apply_model_arguments(
    client: Client,
    model: Option<&str>,
    effort: Option<&str>,
    config: &BTreeMap<String, String>,
    command: &mut CommandBuilder,
) {
    match client {
        Client::ClaudeCode => {
            if let Some(model) = model {
                command.arg("--model");
                command.arg(model);
            }
            if let Some(effort) = effort {
                command.arg("--effort");
                command.arg(effort);
            }
        }
        Client::Codex => {
            for argument in overrides::codex_arguments(config) {
                command.arg(argument);
            }
            if let Some(model) = model {
                command.arg("-c");
                command.arg(format!("model={}", overrides::codex_value(model)));
            }
        }
        Client::OpenCode2 | Client::Antigravity => {}
    }
}

/// Appends the user's extra command-line arguments after every generated one.
pub(crate) fn apply_extra_arguments(args: &[String], command: &mut CommandBuilder) {
    for argument in args {
        command.arg(argument);
    }
}

/// Copies an AI config into a session launch snapshot.
pub(crate) fn launch_config_map(config: &AiConfig) -> BTreeMap<String, String> {
    let mut map = config.config.clone();
    if let Some(effort) = config.effort.as_deref() {
        let key = match config.client {
            Client::Codex => Some("model_reasoning_effort"),
            Client::OpenCode2 => Some("reasoningEffort"),
            Client::Antigravity => Some("effort"),
            Client::ClaudeCode => None,
        };
        if let Some(key) = key {
            map.insert(key.to_owned(), effort.to_owned());
        }
    }
    map
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
    if state.try_save().is_err() {
        state.remove_project(&project.id);
        return Err(ApiError::internal("The project could not be saved."));
    }

    Ok(Json(project))
}

fn github_error(error: crate::github::GitHubError) -> ApiError {
    use crate::github::GitHubError;
    match error {
        GitHubError::NotConfigured => {
            ApiError::unprocessable("GitHub sign-in is not configured in this build.")
        }
        GitHubError::NotConnected => ApiError::unauthorized("GitHub is not connected."),
        GitHubError::Unauthorized => {
            ApiError::unauthorized("GitHub rejected the saved token. Connect again.")
        }
        GitHubError::Upstream => ApiError::bad_gateway("GitHub could not be reached."),
        GitHubError::NotFound => ApiError::not_found("The repository was not found."),
        GitHubError::Conflict(message) => ApiError::conflict(message),
        GitHubError::Invalid(message) => ApiError::unprocessable(message),
        GitHubError::Failed => ApiError::internal("The GitHub action failed."),
    }
}

async fn github_status(State(state): State<AppState>) -> Json<crate::github::GitHubStatus> {
    Json(crate::github::status(&state).await)
}

async fn github_start_device(
    State(state): State<AppState>,
) -> Result<Json<crate::github::GitHubDeviceView>, ApiError> {
    crate::github::start_device(&state)
        .await
        .map(Json)
        .map_err(github_error)
}

async fn github_device_status(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::github::GitHubDeviceView>, ApiError> {
    state
        .github()
        .device_view(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Unknown GitHub sign-in."))
}

async fn github_cancel_device(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    if state.github().cancel_device(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Unknown GitHub sign-in."))
    }
}

async fn github_sign_out(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    crate::github::sign_out(&state)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(github_error)
}

async fn github_repos(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<crate::github::GitHubRepo>>, ApiError> {
    crate::github::list_repos(&state, query.q.as_deref().unwrap_or_default())
        .await
        .map(Json)
        .map_err(github_error)
}

async fn github_clone(
    State(state): State<AppState>,
    body: Result<Json<CloneGitHubRepoRequest>, JsonRejection>,
) -> Result<Json<Project>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    crate::github::clone_repo(
        &state,
        crate::github::CloneInput {
            full_name: request.full_name,
            name: request.name,
            parent_folder: request.parent_folder,
        },
    )
    .await
    .map(Json)
    .map_err(github_error)
}

async fn update_project(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<UpdateProjectRequest>, JsonRejection>,
) -> Result<Json<Project>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let project = state
        .project(&id)
        .ok_or_else(|| ApiError::not_found("Unknown project."))?;

    let config_update = request.default_config_id.map(cleaned_optional);
    let branch_update = request.default_source_branch.map(cleaned_optional);

    if let Some(Some(config_id)) = config_update.as_ref()
        && state.config(config_id).is_none()
    {
        return Err(ApiError::unprocessable(
            "The default AI config does not exist.",
        ));
    }

    if let Some(Some(branch)) = branch_update.as_ref() {
        if !safe_branch_name(branch) {
            return Err(ApiError::bad_request(
                "The source branch name is not valid.",
            ));
        }

        let repo_root = git::repo_root(PathBuf::from(&project.folder), state.git_timeout())
            .await
            .ok_or_else(|| {
                ApiError::unprocessable("The project folder is not a Git repository.")
            })?;
        if !git::branch_or_remote_exists(repo_root, branch.clone(), state.git_timeout()).await {
            return Err(ApiError::unprocessable("The source branch does not exist."));
        }
    }

    let updated = state
        .update_project_defaults(&id, config_update, branch_update)
        .ok_or_else(|| ApiError::not_found("Unknown project."))?;
    if state.try_save().is_err() {
        state.update_project_defaults(
            &id,
            Some(project.default_config_id.clone()),
            Some(project.default_source_branch.clone()),
        );
        return Err(ApiError::internal("The project could not be saved."));
    }

    Ok(Json(updated))
}

fn safe_server_url(value: &str) -> bool {
    (value.starts_with("https://") || value.starts_with("http://"))
        && value.chars().count() <= 500
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
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

pub(crate) async fn account_key(state: &AppState, account_id: &str) -> Result<String, ApiError> {
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

async fn account_key_available(state: &AppState, account_id: &str) -> Result<bool, ApiError> {
    let store = state.secret_store();
    let id = account_id.to_owned();
    let result = tokio::task::spawn_blocking(move || store.get_key(&id)).await;
    match result {
        Ok(Ok(Some(key))) => Ok(!key.is_empty()),
        Ok(Ok(None)) => Ok(false),
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

async fn delete_account_key(state: &AppState, account_id: &str) -> Result<(), String> {
    let store = state.secret_store();
    let account_id = account_id.to_owned();
    match tokio::task::spawn_blocking(move || store.delete_key(&account_id)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

async fn provider_environment(
    state: &AppState,
    provider: &Provider,
    client: Client,
) -> Result<Vec<(String, String)>, ApiError> {
    let key = if provider.auth == Auth::Key && provider.vendor != antigravity::PROVIDER_ID {
        Some(account_key(state, &provider.id).await?)
    } else {
        None
    };
    Ok(crate::providers::environment(
        provider,
        client,
        key.as_deref(),
    ))
}

pub(crate) fn secret_environment_values(environment: &[(String, String)]) -> Vec<String> {
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
    let Json(mut request) = body.map_err(invalid_json)?;
    let size = requested_size(request.cols, request.rows)?;
    let launch_env = validated_env(request.env.take())?;
    if request.kind == SessionKind::Terminal && request.config_id.is_some() {
        return Err(ApiError::bad_request(
            "AI configs apply to agent sessions only.",
        ));
    }
    if request.kind == SessionKind::Terminal && request.allowed_config_ids.is_some() {
        return Err(ApiError::bad_request(
            "Allowed AI configs apply to agent sessions only.",
        ));
    }
    let project = state
        .project(&request.project_id)
        .ok_or_else(|| ApiError::not_found("Unknown project."))?;

    if request.allowed_config_ids.is_none()
        && let Some(legacy) = request.allowed_account_ids.take()
    {
        request.allowed_config_ids = Some(
            legacy
                .iter()
                .map(|id| {
                    let id = id.trim();
                    resolve_config_id(&state, id).unwrap_or_else(|| id.to_owned())
                })
                .collect(),
        );
    }
    let config = match request.kind {
        SessionKind::Agent => {
            let requested = request
                .config_id
                .as_deref()
                .or(request.account_id.as_deref())
                .or(request.provider_id.as_deref())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| ApiError::bad_request("An agent session needs an AI config."))?;
            let pair = resolve_config(&state, requested, request.client)
                .ok_or_else(|| ApiError::not_found("Unknown AI config."))?;
            request.config_id = Some(pair.0.id.clone());
            Some(pair)
        }
        SessionKind::Terminal => None,
    };

    let mut prepared = match request.kind {
        SessionKind::Terminal => prepare_terminal(&state, &request, &project)?,
        SessionKind::Agent => {
            let (config, provider) = config.as_ref().expect("an agent config");
            prepare_agent(&state, &request, &project, config, provider).await?
        }
    };
    prepared.session.env_overrides = launch_env.clone();
    let is_claude_agent = prepared.session.kind == SessionKind::Agent
        && prepared.session.client == Some(Client::ClaudeCode);
    let agent_client = prepared.session.client;
    let launch_model = prepared.session.model.clone();
    let launch_effort = prepared.session.effort.clone();
    let launch_args = prepared.session.args.clone();
    let launch_config = prepared.session.config.clone();
    let config_environment = config
        .as_ref()
        .map(|(config, _)| config.env.clone())
        .unwrap_or_default();
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
        sessions::SessionRuntimeConfig::new(
            state.event_sender(),
            state.aux_sender(),
            state.preview_throttle(),
        ),
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
                return Err(ApiError::bad_request(
                    "The source branch name is not valid.",
                ));
            }
            Some(branch) => {
                if !git::branch_or_remote_exists(
                    repo_root.clone(),
                    branch.to_owned(),
                    state.git_timeout(),
                )
                .await
                {
                    rollback_created_session(&state, &entry, &session_id, None).await;
                    return Err(ApiError::unprocessable("The source branch does not exist."));
                }
                Some(branch.to_owned())
            }
            None => None,
        };
        let synced = match git::sync_source(
            repo_root.clone(),
            source,
            state.git_push_timeout(),
            state.git_timeout(),
        )
        .await
        {
            Ok(synced) => synced,
            Err(error) => {
                rollback_created_session(&state, &entry, &session_id, None).await;
                return Err(ApiError::unprocessable(error.message()));
            }
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
            synced.base,
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
        if request.copy_uncommitted_changes
            && let Err(error) = git::copy_uncommitted_changes(
                repo_root.clone(),
                worktree.clone(),
                state.git_timeout(),
            )
            .await
        {
            rollback_created_session(&state, &entry, &session_id, worktree_cleanup.clone()).await;
            return Err(ApiError::unprocessable(error.message()));
        }
        entry.set_agent_worktree(path_string(&worktree), branch, path_string(&repo_root));
        worktree
    } else {
        prepared.folder
    };
    drop(identity_guard);

    if let Some(client) = agent_client {
        apply_model_arguments(
            client,
            launch_model.as_deref(),
            launch_effort.as_deref(),
            &launch_config,
            &mut prepared.command,
        );
    }

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
    if let Some(provider_id) = prepared.antigravity_provider.as_deref() {
        let Some(data_dir) = state.data_dir().map(Path::to_path_buf) else {
            rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
            return Err(ApiError::unprocessable(
                "Agent sessions need a data directory.",
            ));
        };
        let key = match account_key(&state, provider_id).await {
            Ok(key) => key,
            Err(error) => {
                rollback_created_session(&state, &entry, &session_id, worktree_cleanup).await;
                return Err(error);
            }
        };
        match antigravity::prepare_home(
            &data_dir,
            &session_id,
            launch_model.as_deref(),
            &launch_config,
        ) {
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

    for item in &config_environment {
        environment.push((item.name.clone(), item.value.clone()));
    }
    for item in &launch_env {
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
            && state.command_override().is_none()
            && let Err(error) = start_codex_runtime(&state, &entry, &mut prepared.command)
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

    if agent_client.is_some() {
        apply_extra_arguments(&launch_args, &mut prepared.command);
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
            tokio::time::sleep(STARTUP_INPUT_DELAY).await;
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

    state.stop_browser(&id).await;
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
                        closing.handoff_note.clone(),
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
    state.close_demo_tunnels(&session);
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
    let Json(mut request) = body.map_err(invalid_json)?;
    if request.allowed_config_ids.is_none()
        && let Some(legacy) = request.allowed_account_ids.take()
    {
        request.allowed_config_ids = Some(
            legacy
                .iter()
                .map(|id| {
                    let id = id.trim();
                    resolve_config_id(&state, id).unwrap_or_else(|| id.to_owned())
                })
                .collect(),
        );
    }
    if request.name.is_none() && request.allowed_config_ids.is_none() {
        return Err(ApiError::bad_request(
            "Send a session name or allowed AI configs.",
        ));
    }
    let name = match request.name.as_deref() {
        Some(name) => {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 64 {
                return Err(ApiError::bad_request(
                    "Session name must be between 1 and 64 characters.",
                ));
            }
            Some(name.to_owned())
        }
        None => None,
    };

    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    let allowed = match request.allowed_config_ids.as_ref() {
        Some(ids) => {
            if session.kind != SessionKind::Agent {
                return Err(ApiError::unprocessable(
                    "Only agent sessions have AI configs.",
                ));
            }
            if session.status != SessionState::Active {
                return Err(ApiError::conflict(
                    "Reopen the session before changing its AI configs.",
                ));
            }
            let current = session
                .config_id
                .as_deref()
                .ok_or_else(|| ApiError::unprocessable("The session has no AI config."))?;
            Some(validated_allowed_configs(&state, ids, current)?)
        }
        None => None,
    };

    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("Session is closing."))?;
    if entry.is_closing() {
        return Err(ApiError::conflict("Session is closing."));
    }
    let mut updated = match name {
        Some(name) => entry.rename(name),
        None => entry.session(),
    };
    if let Some(allowed) = allowed {
        updated = entry
            .set_allowed_configs(allowed)
            .ok_or_else(|| ApiError::conflict("The session is busy."))?;
    }
    state.emit(SessionEvent::updated(updated.clone()));
    state.save();
    Ok(Json(updated))
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
        let config_id = suspended
            .config_id
            .as_deref()
            .ok_or_else(|| ApiError::unprocessable("The AI config is no longer available."))?;
        let (config, provider) = state
            .config_with_provider(config_id)
            .ok_or_else(|| ApiError::unprocessable("The AI config is no longer available."))?;
        environment = provider_environment(&state, &provider, config.client).await?;
        if state.command_override().is_none()
            && state.agent_bin().is_none()
            && clients::find_client(config.client).is_none()
        {
            return Err(ApiError::unprocessable(format!(
                "{} is not installed.",
                config.client
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
        for argument in sessions::resume_arguments(client, suspended.agent_session_id.as_deref()) {
            command.arg(argument);
        }
        apply_model_arguments(
            client,
            suspended.model.as_deref(),
            suspended.effort.as_deref(),
            &suspended.config,
            &mut command,
        );
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
        let provider_id = suspended
            .provider_id
            .as_deref()
            .ok_or_else(|| ApiError::unprocessable("The provider is no longer available."))?;
        let data_dir = state
            .data_dir()
            .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
        let key = account_key(&state, provider_id).await?;
        let home = antigravity::prepare_home(
            data_dir,
            &id,
            suspended.model.as_deref(),
            &suspended.config,
        )
        .map_err(|error| {
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
        if client == Client::Codex && state.command_override().is_none() {
            start_codex_runtime(&state, &entry, &mut command)?;
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
        apply_extra_arguments(&suspended.args, &mut command);
    }

    let handoff_context = (suspended.kind == SessionKind::Agent
        && suspended.agent_session_id.is_none())
    .then(|| entry.handoff_context())
    .flatten();
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
    if let Some(context) = handoff_context {
        deliver_handoff_context(&entry, context);
    }

    let session = entry.session();
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
    Ok(Json(session))
}

async fn switch_options(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<SwitchOptions>, ApiError> {
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    if session.kind != SessionKind::Agent {
        return Err(ApiError::unprocessable(
            "Only agent sessions have AI configs.",
        ));
    }
    let configs = state.configs();
    let providers = state.providers();
    let next = crate::providers::auto_switch_target(
        &configs,
        &providers,
        &session.allowed_config_ids,
        session.client,
        session.config_id.as_deref(),
    );
    Ok(Json(SwitchOptions {
        configs,
        allowed_config_ids: session.allowed_config_ids.clone(),
        next_config_id: next.map(|config| config.id),
    }))
}

async fn switch_session_config(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<SwitchConfigRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    switch_to_config(&state, &id, &request.config_id).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySwitchRequest {
    account_id: String,
}

/// The phone app switches by provider id. It gets the provider's config for
/// the session's client when one exists.
async fn switch_session_account_alias(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<LegacySwitchRequest>, JsonRejection>,
) -> Result<Json<Session>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let account_id = request.account_id.trim();
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    let config = state
        .configs()
        .into_iter()
        .find(|config| config.provider_id == account_id && Some(config.client) == session.client)
        .or_else(|| {
            state
                .configs()
                .into_iter()
                .find(|config| config.provider_id == account_id)
        })
        .ok_or_else(|| ApiError::not_found("Unknown provider."))?;
    switch_to_config(&state, &id, &config.id).await
}

async fn switch_to_config(
    state: &AppState,
    session_id: &str,
    config_id: &str,
) -> Result<Json<Session>, ApiError> {
    let entry = state
        .session(session_id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    if session.kind != SessionKind::Agent {
        return Err(ApiError::unprocessable(
            "Only agent sessions have AI configs.",
        ));
    }
    if session.status != SessionState::Active {
        return Err(ApiError::conflict("Reopen the session before switching."));
    }
    let config_id = config_id.trim();
    if config_id.is_empty() {
        return Err(ApiError::bad_request("Choose an AI config."));
    }
    let (config, provider) = state
        .config_with_provider(config_id)
        .ok_or_else(|| ApiError::not_found("Unknown AI config."))?;
    if !session
        .allowed_config_ids
        .iter()
        .any(|allowed| allowed == config_id)
    {
        return Err(ApiError::unprocessable(
            "The AI config is not allowed for this session.",
        ));
    }
    let same_client = session.client == Some(config.client);
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("The session is busy."))?;
    relaunch_with_config(state, &entry, &config, &provider, same_client).await
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
    let configs = state.configs();
    let config = match crate::providers::auto_switch_target(
        &configs,
        &state.providers(),
        &session.allowed_config_ids,
        Some(client),
        None,
    ) {
        Some(config) => config,
        None => {
            // A manual Resume works at any time. When no config reports room,
            // keep the session on its current config and clear a reported
            // limit that no official source can replace.
            let current = session.config_id.as_deref();
            let config = configs
                .iter()
                .find(|config| {
                    Some(config.id.as_str()) == current
                        && session.allowed_config_ids.iter().any(|id| id == &config.id)
                })
                .cloned()
                .ok_or_else(|| ApiError::unprocessable("No allowed AI config has usage left."))?;
            if let Some(provider) = state.clear_provider_limit_report(&config.provider_id) {
                state.publish_provider(provider);
            }
            config
        }
    };
    let provider = state
        .provider(&config.provider_id)
        .ok_or_else(|| ApiError::unprocessable("The provider is no longer available."))?;
    let _guard = entry
        .try_lifecycle()
        .ok_or_else(|| ApiError::conflict("The session is busy."))?;
    let same_client = session.client == Some(config.client);
    relaunch_with_config(&state, &entry, &config, &provider, same_client).await
}

async fn relaunch_with_config(
    state: &AppState,
    entry: &Arc<SessionEntry>,
    config: &AiConfig,
    provider: &Provider,
    resume: bool,
) -> Result<Json<Session>, ApiError> {
    let session = entry.session();
    if state.command_override().is_none()
        && state.agent_bin().is_none()
        && clients::find_client(config.client).is_none()
    {
        return Err(ApiError::unprocessable(format!(
            "{} is not installed.",
            config.client
        )));
    }

    let mut command = session_command(state, SessionKind::Agent, Some(config.client))?;
    let saved_agent_session_id = resume.then(|| session.agent_session_id.clone()).flatten();
    if resume {
        for argument in sessions::resume_arguments(config.client, saved_agent_session_id.as_deref())
        {
            command.arg(argument);
        }
    }
    let mut environment = provider_environment(state, provider, config.client).await?;
    for item in &config.env {
        environment.push((item.name.clone(), item.value.clone()));
    }
    let folder = canonical_directory(PathBuf::from(&session.folder))?;
    let mut opencode_base = None;
    let handoff_context = (!resume).then(|| entry.handoff_context()).flatten();

    let should_wait = entry
        .begin_close()
        .map_err(|_| ApiError::conflict("The session is closing."))?;
    if should_wait {
        let _ = wait_for_process_exit(entry, Duration::from_secs(2)).await;
    }
    entry.remove_session_files();

    let launch_config = launch_config_map(config);
    let launched = entry.set_launch_config(config, launch_config.clone());
    debug_assert_eq!(launched.id, session.id);
    apply_model_arguments(
        config.client,
        config.model.as_deref(),
        config.effort.as_deref(),
        &launch_config,
        &mut command,
    );

    if config.client == Client::ClaudeCode {
        match install_claude_settings(state, entry, &session.id) {
            Ok(settings_path) => {
                if state.command_override().is_none() {
                    claude::add_settings_argument(&mut command, &settings_path);
                }
            }
            Err(error) => {
                let failed = entry.mark_switch_failed("The AI config switch failed.");
                state.emit(SessionEvent::updated(failed));
                state.save();
                return Err(error);
            }
        }
    }
    if config.client == Client::Antigravity {
        let key = account_key(state, &provider.id).await?;
        let data_dir = state
            .data_dir()
            .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
        match antigravity::prepare_home(
            data_dir,
            &session.id,
            config.model.as_deref(),
            &launch_config,
        ) {
            Ok(home) => {
                entry.set_home(home.clone());
                environment.extend(antigravity::environment(&home, &key));
            }
            Err(error) => {
                tracing::error!(session = %session.id, %error, "could not prepare the Antigravity home");
                let failed = entry.mark_switch_failed("The AI config switch failed.");
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
        config.client,
        &mut command,
        &mut environment,
    ) {
        let failed = entry.mark_switch_failed("The AI config switch failed.");
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(error);
    }
    apply_extra_arguments(&config.args, &mut command);
    if config.client == Client::Codex
        && state.command_override().is_none()
        && let Err(error) = start_codex_runtime(state, entry, &mut command)
    {
        let failed = entry.mark_switch_failed("The AI config switch failed.");
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(error);
    }
    if config.client == Client::OpenCode2
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
        config.id.clone(),
        provider.id.clone(),
        config.client,
        shortcuts::for_client(config.client),
        saved_agent_session_id,
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
        let failed = entry.mark_switch_failed(&format!("The AI config switch failed: {error}"));
        state.emit(SessionEvent::updated(failed));
        state.save();
        return Err(ApiError::unprocessable("The AI config switch failed."));
    }

    if let Some(base_url) = opencode_base {
        start_opencode_poller(state.clone(), Arc::clone(entry), base_url);
    }
    if let Some(context) = handoff_context {
        deliver_handoff_context(entry, context);
    }

    state.emit(SessionEvent::updated(switched.clone()));
    state.save();
    Ok(Json(switched))
}

fn deliver_handoff_context(entry: &Arc<SessionEntry>, context: String) {
    let entry = Arc::clone(entry);
    tokio::spawn(async move {
        tokio::time::sleep(STARTUP_INPUT_DELAY).await;
        let mut bytes = context.into_bytes();
        bytes.push(b'\n');
        let _ = entry.write_input(bytes).await;
    });
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

async fn claude_statusline(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<HookQuery>, QueryRejection>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::forbidden("Invalid hook credentials."))?;
    let entry = authorize_claude_session(&state, &id, query.token.as_deref())?;
    let session = entry.session();
    let Json(input) = body.map_err(|_| ApiError::bad_request("Invalid status line input."))?;

    if let Some(provider_id) = session.provider_id.as_deref() {
        let rows = crate::usage::claude_rows(&input);
        if !rows.is_empty() {
            crate::usage::apply_usage(&state, provider_id, crate::usage::STATUS_LINE, rows).await;
        }
    }

    record_claude_session_id(
        &state,
        &entry,
        input.get("session_id").and_then(serde_json::Value::as_str),
    );
    let context_left = input.get("context_window").and_then(context_remaining);
    if let Some(session) = entry.note_context_left(context_left) {
        state.emit(SessionEvent::updated(session));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn auto_switch_provider(state: &AppState, provider: &Provider) {
    let usage = crate::providers::usage_state(provider);
    if usage == crate::providers::UsageState::Room {
        resume_paused_sessions(state).await;
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
            || session.provider_id.as_deref() != Some(provider.id.as_str())
        {
            continue;
        }

        let target = crate::providers::auto_switch_target(
            &state.configs(),
            &state.providers(),
            &session.allowed_config_ids,
            session.client,
            session.config_id.as_deref(),
        );

        let Some(target) = target else {
            let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
                continue;
            };
            let current = entry.session();
            if current.provider_id.as_deref() != Some(provider.id.as_str())
                || current.work_state == Some(WorkState::Paused)
            {
                continue;
            }
            pause_session(
                state,
                &entry,
                "Paused. No allowed AI config has usage left.",
            );
            continue;
        };
        let Some(target_provider) = state.provider(&target.provider_id) else {
            continue;
        };

        let same_client = session.client == Some(target.client);
        let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
            continue;
        };
        let current = entry.session();
        if current.provider_id.as_deref() != Some(provider.id.as_str())
            || current.work_state == Some(WorkState::Paused)
        {
            continue;
        }
        if relaunch_with_config(state, &entry, &target, &target_provider, same_client)
            .await
            .is_err()
        {
            pause_session(state, &entry, "Paused. The AI config switch failed.");
        }
    }
}

async fn resume_paused_sessions(state: &AppState) {
    for entry in state.sessions() {
        let session = entry.session();
        if session.status != SessionState::Active
            || session.kind != SessionKind::Agent
            || session.work_state != Some(WorkState::Paused)
        {
            continue;
        }
        let Some(target) = crate::providers::auto_switch_target(
            &state.configs(),
            &state.providers(),
            &session.allowed_config_ids,
            session.client,
            None,
        ) else {
            continue;
        };
        let Some(target_provider) = state.provider(&target.provider_id) else {
            continue;
        };
        let same_client = session.client == Some(target.client);
        let Ok(_guard) = entry.try_lifecycle().ok_or(()) else {
            continue;
        };
        if entry.session().work_state != Some(WorkState::Paused) {
            continue;
        }
        if relaunch_with_config(state, &entry, &target, &target_provider, same_client)
            .await
            .is_err()
        {
            pause_session(state, &entry, "Paused. The AI config switch failed.");
        }
    }
}

/// Asks the agent to finish safely, then pauses the session in the background.
///
/// The usage paths that call this must not wait for the agent: the same task
/// may also deliver the agent's own completion events.
fn pause_session(state: &AppState, entry: &Arc<SessionEntry>, reason: &str) {
    let state = state.clone();
    let entry = Arc::clone(entry);
    let reason = reason.to_owned();
    tokio::spawn(async move {
        run_pause(state, entry, reason).await;
    });
}

async fn run_pause(state: AppState, entry: Arc<SessionEntry>, reason: String) {
    let _guard = entry.lock_lifecycle().await;
    let current = entry.session();
    if current.status != SessionState::Active {
        return;
    }
    if current.work_state == Some(WorkState::Paused) {
        // A failed resume reports its reason on a session that is already
        // paused. Keep the reason without repeating the pause sequence.
        let session = entry.pause(&reason);
        state.emit(SessionEvent::updated(session.clone()));
        state.save();
        return;
    }
    if entry.is_closing() {
        return;
    }

    let mut instruction = PAUSE_INSTRUCTION.as_bytes().to_vec();
    instruction.push(b'\n');
    let told = entry.write_input(instruction).await.is_ok();

    // Give the agent a bounded time to commit, write the note and push.
    let deadline = tokio::time::Instant::now() + state.pause_grace();
    while tokio::time::Instant::now() < deadline {
        if entry.process_exited() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    state.stop_browser(&entry.id()).await;
    let Ok(should_wait) = entry.begin_close().map_err(|_| ()) else {
        return;
    };
    if should_wait {
        let _ = wait_for_process_exit(&entry, Duration::from_secs(2)).await;
    }
    entry.remove_session_files();

    let current = entry.session();
    let outcome = match current.branch {
        Some(branch) => {
            git::pause_worktree(
                PathBuf::from(&current.folder),
                branch,
                current.name.clone(),
                current.handoff_note.clone(),
                git::GitTimeouts {
                    local: state.git_timeout(),
                    push: state.git_push_timeout(),
                },
            )
            .await
        }
        None => git::PauseOutcome::Unavailable,
    };
    let reason = if told {
        format!("{reason} {}", outcome.preview())
    } else {
        format!("{reason} The agent was not running. {}", outcome.preview())
    };
    let session = entry.pause(&reason);
    state.close_demo_tunnels(&session);
    state.emit(SessionEvent::updated(session.clone()));
    state.save();
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

    state.stop_browser(&id).await;
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

async fn list_session_files(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<FileQuery>, QueryRejection>,
) -> Result<Json<files::FileList>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid file query."))?;
    let root = session_folder(&state, &id)?;
    let path = query.path.unwrap_or_default();
    let list = tokio::task::spawn_blocking(move || files::list(&root, &path))
        .await
        .map_err(|_| ApiError::internal("The file listing failed."))?
        .map_err(file_error)?;
    Ok(Json(list))
}

async fn read_session_file(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<FileQuery>, QueryRejection>,
) -> Result<Json<files::FileContent>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid file query."))?;
    let path = query
        .path
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("A file path is required."))?;
    let root = session_folder(&state, &id)?;
    let content = tokio::task::spawn_blocking(move || files::read(&root, &path))
        .await
        .map_err(|_| ApiError::internal("The file read failed."))?
        .map_err(file_error)?;
    Ok(Json(content))
}

async fn write_session_file(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<WriteFileRequest>, JsonRejection>,
) -> Result<Json<files::FileContent>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    let root = session_folder(&state, &id)?;
    let written =
        tokio::task::spawn_blocking(move || files::write(&root, &request.path, &request.content))
            .await
            .map_err(|_| ApiError::internal("The file save failed."))?
            .map_err(file_error)?;
    Ok(Json(written))
}

async fn list_shared_files(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Vec<shared::SharedFile>>, ApiError> {
    if state.session(&id).is_none() {
        return Err(ApiError::not_found("Unknown session."));
    }
    let data_dir = shared_data_dir(&state)?;
    let session_id = id.clone();
    let files = tokio::task::spawn_blocking(move || shared::list(&data_dir, &session_id))
        .await
        .map_err(|_| ApiError::internal("The shared file list failed."))?
        .map_err(shared_error)?;
    Ok(Json(files))
}

async fn read_shared_file(
    State(state): State<AppState>,
    UrlPath((id, file_id)): UrlPath<(String, String)>,
) -> Result<Response, ApiError> {
    if state.session(&id).is_none() {
        return Err(ApiError::not_found("Unknown session."));
    }
    let data_dir = shared_data_dir(&state)?;
    let (file, bytes) = tokio::task::spawn_blocking(move || shared::read(&data_dir, &id, &file_id))
        .await
        .map_err(|_| ApiError::internal("The shared file read failed."))?
        .map_err(shared_error)?;
    let inline = file.image;
    let mut response = Response::new(axum::body::Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&file.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&file.name, inline))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(NOSNIFF, HeaderValue::from_static("nosniff"));
    Ok(response)
}

fn active_session(state: &AppState, id: &str) -> Result<Arc<SessionEntry>, ApiError> {
    let entry = state
        .session(id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    if entry.is_closed() {
        return Err(ApiError::conflict("The session is closed."));
    }
    Ok(entry)
}

fn browser_error(error: crate::browser::BrowserError) -> ApiError {
    use crate::browser::BrowserError;
    match error {
        BrowserError::InvalidUrl(_) => ApiError::bad_request(error.message()),
        BrowserError::NoPage => ApiError::conflict(error.message()),
        BrowserError::TabLimit => ApiError::conflict(error.message()),
        BrowserError::Unavailable(_) => ApiError::service_unavailable(error.message()),
        BrowserError::Engine(_) => ApiError::bad_gateway(error.message()),
    }
}

async fn browser_state(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    if state.session(&id).is_none() {
        return Err(ApiError::not_found("Unknown session."));
    }
    Ok(Json(state.browser().state(&id)))
}

async fn browser_start(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state.browser().start(&id).await.map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_stop(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    if state.session(&id).is_none() {
        return Err(ApiError::not_found("Unknown session."));
    }
    state.stop_browser(&id).await;
    Ok(Json(state.browser().state(&id)))
}

async fn browser_setup(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    state.browser().setup().map_err(browser_error)?;
    Ok(Json(state.browser().state(&id)))
}

async fn browser_open(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<BrowserOpenRequest>, JsonRejection>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    active_session(&state, &id)?;
    let state_value = state
        .browser()
        .open(&id, &request.url)
        .await
        .map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_back(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state.browser().back(&id).await.map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_forward(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state.browser().forward(&id).await.map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_reload(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state.browser().reload(&id).await.map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_new_tab(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    body: Result<Json<BrowserTabRequest>, JsonRejection>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    let Json(request) = body.map_err(invalid_json)?;
    active_session(&state, &id)?;
    let state_value = state
        .browser()
        .new_tab(&id, request.url.as_deref())
        .await
        .map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_activate(
    State(state): State<AppState>,
    UrlPath((id, tab)): UrlPath<(String, String)>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state
        .browser()
        .activate(&id, &tab)
        .await
        .map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_close_tab(
    State(state): State<AppState>,
    UrlPath((id, tab)): UrlPath<(String, String)>,
) -> Result<Json<crate::browser::BrowserState>, ApiError> {
    active_session(&state, &id)?;
    let state_value = state
        .browser()
        .close_tab(&id, &tab)
        .await
        .map_err(browser_error)?;
    Ok(Json(state_value))
}

async fn browser_screenshot(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<BrowserScreenshotQuery>,
) -> Result<Response, ApiError> {
    if state.session(&id).is_none() {
        return Err(ApiError::not_found("Unknown session."));
    }
    let tab = query
        .tab
        .or_else(|| state.browser().state(&id).active_tab_id);
    let Some(tab) = tab else {
        return Err(ApiError::not_found("No page is open."));
    };
    let Some(bytes) = state.browser().screenshot_bytes(&id, &tab) else {
        return Err(ApiError::not_found("No screenshot is available."));
    };
    let mut response = Response::new(axum::body::Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(NOSNIFF, HeaderValue::from_static("nosniff"));
    Ok(response)
}

fn shared_data_dir(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .data_dir()
        .map(Path::to_path_buf)
        .ok_or_else(|| ApiError::internal("The shared file store is not available."))
}
fn shared_error(error: shared::ShareError) -> ApiError {
    match error {
        shared::ShareError::InvalidPath | shared::ShareError::OutsideFolder => {
            ApiError::bad_request("The shared file path is not valid.")
        }
        shared::ShareError::NotFound => ApiError::not_found("The shared file does not exist."),
        shared::ShareError::NotAFile => ApiError::unprocessable("The shared file is not a file."),
        shared::ShareError::TooLarge(size) => ApiError::payload_too_large(format!(
            "The shared file is too large. The limit is {} bytes; this file is {size} bytes.",
            shared::MAX_SHARED_BYTES
        )),
        shared::ShareError::Linked => {
            ApiError::unprocessable("Hard-linked files are not supported.")
        }
        shared::ShareError::Secret => ApiError::unprocessable("The shared file is not available."),
        shared::ShareError::Unavailable => {
            ApiError::internal("The shared file store is not available.")
        }
        shared::ShareError::Io(error) => {
            tracing::error!(%error, "the shared file route failed");
            ApiError::internal("The shared file operation failed.")
        }
    }
}

fn content_disposition(name: &str, inline: bool) -> String {
    let name = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || " ._-()".contains(character) {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect::<String>();
    format!(
        "{}; filename=\"{}\"",
        if inline { "inline" } else { "attachment" },
        name
    )
}

async fn session_changes(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<git::Changes>, ApiError> {
    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let session = entry.session();
    let agent_repo_root = if session.kind == SessionKind::Agent {
        entry.repo_root().map(PathBuf::from)
    } else {
        None
    };
    let root = session_folder(&state, &id)?;
    let changes = git::changes(root, agent_repo_root, state.git_timeout()).await;
    Ok(Json(changes))
}

fn session_folder(state: &AppState, id: &str) -> Result<PathBuf, ApiError> {
    let entry = state
        .session(id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let folder = PathBuf::from(entry.session().folder);
    let root = folder
        .canonicalize()
        .map_err(|_| ApiError::not_found("The session folder is no longer available."))?;
    if !root.is_dir() {
        return Err(ApiError::not_found(
            "The session folder is no longer available.",
        ));
    }
    Ok(root)
}

fn file_error(error: files::FileError) -> ApiError {
    match error {
        files::FileError::InvalidPath | files::FileError::OutsideRoot => {
            ApiError::bad_request("The path must stay inside the session folder.")
        }
        files::FileError::NotFound => ApiError::not_found("The file does not exist."),
        files::FileError::NotAFile => ApiError::unprocessable("The path is a folder, not a file."),
        files::FileError::NotADirectory => ApiError::unprocessable("The path is not a folder."),
        files::FileError::TooLarge(size) => ApiError::payload_too_large(format!(
            "The file is too large. The limit is {} bytes; this file is {size} bytes.",
            files::MAX_FILE_BYTES
        )),
        files::FileError::Binary => ApiError::unprocessable("The file is not UTF-8 text."),
        files::FileError::Protected => ApiError::unprocessable("The path is not available."),
        files::FileError::Linked => ApiError::unprocessable("Hard-linked files are not supported."),
        files::FileError::Io(error) => {
            tracing::error!(%error, "file operation failed");
            ApiError::internal("The file operation failed.")
        }
    }
}

async fn session_output(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<OutputQuery>, QueryRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let Query(query) =
        query.map_err(|error| ApiError::bad_request(format!("Invalid output query: {error}")))?;

    let after = parse_after(query.after.as_deref(), "output")?;

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
        .map_err(|failure| history_snapshot_error(failure, "Output"))?;
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

/// Streams the structured conversation for one session.
///
/// The first event tells the phone whether the client has a structured source.
/// The retained items follow, then live versions. An item update arrives as a
/// new version with a higher sequence and the same id.
async fn session_history(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<OutputQuery>, QueryRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let Query(query) =
        query.map_err(|error| ApiError::bad_request(format!("Invalid history query: {error}")))?;
    let after = parse_after(query.after.as_deref(), "history")?;

    let entry = state
        .session(&id)
        .ok_or_else(|| ApiError::not_found("Unknown session."))?;
    let permit = state
        .try_acquire_events_sse()
        .ok_or_else(|| ApiError::too_many_requests("Too many open event streams."))?;

    let mut receiver = state.subscribe_aux();
    let mut close_receiver = entry.subscribe_close();
    let status = HistoryStatus::new(
        entry.history_supported(),
        entry
            .session()
            .client
            .map(|client| client.display_name().to_owned()),
    );
    let snapshot = entry
        .history_snapshot(after)
        .map_err(|failure| history_snapshot_error(failure, "History"))?;
    let mut last_seq = snapshot.newest_seq;
    let replay = snapshot.items;
    let session_id = id.clone();

    let stream = async_stream::stream! {
        let _permit = permit;
        yield Ok(Event::default().comment("connected"));
        if let Some(event) = json_event(&status) {
            yield Ok(event);
        }

        for item in replay {
            if let Some(event) = json_event(&item) {
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
                            Ok(AuxEvent::History { session_id: event_session, item })
                                if event_session == session_id =>
                            {
                                if item.seq > last_seq {
                                    last_seq = item.seq;
                                    if let Some(event) = json_event(&item) {
                                        yield Ok(event);
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                match entry.history_snapshot(last_seq) {
                                    Ok(snapshot) => {
                                        last_seq = snapshot.newest_seq;
                                        for item in snapshot.items {
                                            if let Some(event) = json_event(&item) {
                                                yield Ok(event);
                                            }
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

fn parse_after(value: Option<&str>, label: &str) -> Result<u64, ApiError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value.parse::<u64>().map_err(|_| {
            ApiError::bad_request(format!(
                "Query parameter after must be a non-negative integer ({label})."
            ))
        }),
        None => Ok(0),
    }
}

fn history_snapshot_error(failure: sessions::SnapshotFailure, label: &str) -> ApiError {
    match failure {
        sessions::SnapshotFailure::AfterAhead { newest } => ApiError::bad_request(format!(
            "Query parameter after is beyond the newest sequence ({newest})."
        )),
        sessions::SnapshotFailure::LogUnavailable => {
            ApiError::bad_request(format!("{label} at that position is no longer retained."))
        }
    }
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
        QuestionFailure::NotPending => ApiError::conflict("This question is no longer pending."),
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
            "configId": pending.config_id,
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

fn authorize_claude_session(
    state: &AppState,
    id: &str,
    token: Option<&str>,
) -> Result<Arc<SessionEntry>, ApiError> {
    let entry = state
        .session(id)
        .ok_or_else(|| ApiError::forbidden("Invalid hook credentials."))?;
    let session = entry.session();
    if session.kind != SessionKind::Agent || session.client != Some(Client::ClaudeCode) {
        return Err(ApiError::forbidden("Invalid hook credentials."));
    }
    let expected = entry
        .claude_token()
        .ok_or_else(|| ApiError::forbidden("Invalid hook credentials."))?;
    if !claude::token_matches(&expected, token.unwrap_or_default()) {
        return Err(ApiError::forbidden("Invalid hook credentials."));
    }
    Ok(entry)
}

fn context_remaining(context_window: &serde_json::Value) -> Option<u8> {
    let remaining = context_window.get("remaining_percentage")?.as_f64()?;
    Some(remaining.clamp(0.0, 100.0).round() as u8)
}

async fn claude_hook(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    query: Result<Query<HookQuery>, QueryRejection>,
    body: Result<Json<HookInput>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::forbidden("Invalid hook credentials."))?;
    let entry = authorize_claude_session(&state, &id, query.token.as_deref())?;

    let Json(input) = body.map_err(|_| ApiError::bad_request("Invalid hook input."))?;
    record_claude_session_id(&state, &entry, input.session_id.as_deref());
    for change in claude_changes(&input) {
        entry.record_history(change);
    }

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

fn record_claude_session_id(state: &AppState, entry: &SessionEntry, value: Option<&str>) {
    let Some(id) = value
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.chars().count() <= 128)
        .filter(|id| !id.chars().any(char::is_control))
    else {
        return;
    };
    if let Some(session) = entry.note_agent_session_id(id) {
        state.emit(SessionEvent::updated(session));
        state.save();
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
    crate::text::clean_label(value, limit)
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
        config_id: None,
        allowed_config_ids: Vec::new(),
        provider_id: None,
        legacy_account_id: None,
        legacy_allowed_account_ids: Vec::new(),
        legacy_preset_id: None,
        branch: None,
        folder: path_string(&folder),
        preview: "Terminal ready.".to_owned(),
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
        started_at: timestamp(),
        shortcuts: Vec::new(),
    };

    Ok(PreparedSession {
        session,
        folder,
        command,
        repo_root: None,
        antigravity_provider: None,
        environment: Vec::new(),
        startup_input: None,
    })
}

async fn prepare_agent(
    state: &AppState,
    request: &CreateSessionRequest,
    project: &Project,
    config: &AiConfig,
    provider: &Provider,
) -> Result<PreparedSession, ApiError> {
    let allowed_config_ids = match request.allowed_config_ids.as_ref() {
        Some(ids) => validated_allowed_configs(state, ids, &config.id)?,
        None => crate::providers::default_allowed_configs(
            &state.configs(),
            &state.providers(),
            Some(config.client),
            Some(&config.id),
        ),
    };

    let command = session_command(state, SessionKind::Agent, Some(config.client))?;
    let environment = provider_environment(state, provider, config.client).await?;
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
        .unwrap_or_else(|| format!("{} session", config.client));
    let short = slugify(&name, "session");
    let agent_session_id =
        (config.client == Client::ClaudeCode).then(|| uuid::Uuid::new_v4().to_string());
    let mut command = command;
    if let Some(id) = agent_session_id.as_deref()
        && state.command_override().is_none()
    {
        command.arg("--session-id");
        command.arg(id);
    }
    let mut startup_input = None;
    if let Some(prompt) = first_prompt.as_deref() {
        if sessions::client_needs_prompt_input(config.client) {
            startup_input = Some(prompt.to_owned());
        } else {
            for argument in sessions::prompt_arguments(config.client, prompt) {
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
        client: Some(config.client),
        config_id: Some(config.id.clone()),
        allowed_config_ids,
        provider_id: Some(provider.id.clone()),
        legacy_account_id: None,
        legacy_allowed_account_ids: Vec::new(),
        legacy_preset_id: None,
        branch: None,
        folder: path_string(&repo_root),
        preview: format!("{} is starting.", config.client),
        waiting_for: None,
        pending_permission: None,
        first_prompt,
        parent_session_id: request.parent_session_id.clone(),
        parent_session_name: request.parent_session_name.clone(),
        env_overrides: request.env.clone().unwrap_or_default(),
        model: config.model.clone(),
        effort: config.effort.clone(),
        args: config.args.clone(),
        config: launch_config_map(config),
        agent_session_id,
        handoff_note: None,
        context_left: None,
        demo_servers: Vec::new(),
        started_at: timestamp(),
        shortcuts: shortcuts::for_client(config.client),
    };

    Ok(PreparedSession {
        session,
        folder: repo_root.clone(),
        command,
        repo_root: Some(repo_root),
        antigravity_provider: (config.client == Client::Antigravity).then(|| provider.id.clone()),
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

pub(crate) fn session_command(
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

fn prepare_skill_kit(
    entry: &SessionEntry,
    data_dir: &Path,
    session_id: &str,
) -> Result<PathBuf, ApiError> {
    let kit = crate::mcp::prepare_skill_kit(data_dir, session_id).map_err(|error| {
        tracing::error!(session = %session_id, %error, "could not write the skill pack");
        ApiError::unprocessable("Could not prepare session tools.")
    })?;
    entry.set_skill_dir(kit.clone());
    Ok(kit)
}

pub(crate) fn configure_session_tools(
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
            let path = crate::mcp::write_claude_config(data_dir, session_id, state.port())
                .map_err(|error| {
                    tracing::error!(session = %session_id, %error, "could not write MCP config");
                    ApiError::unprocessable("Could not prepare session tools.")
                })?;
            if add_args {
                command.arg("--mcp-config");
                command.arg(path.as_os_str());
            }
            entry.set_mcp_config(path);
            let plugin = prepare_skill_kit(entry, data_dir, session_id)?;
            if add_args {
                command.arg("--plugin-dir");
                command.arg(plugin.as_os_str());
            }
        }
        Client::Codex => {
            let home = crate::mcp::prepare_codex_home(
                data_dir,
                session_id,
                crate::mcp::user_codex_home().as_deref(),
            )
            .map_err(|error| {
                tracing::error!(session = %session_id, %error, "could not prepare Codex skills");
                ApiError::unprocessable("Could not prepare session tools.")
            })?;
            entry.set_skill_dir(crate::mcp::skill_kit_path(data_dir, session_id));
            environment.push(("CODEX_HOME".to_owned(), home.to_string_lossy().into_owned()));
            if add_args {
                command.arg("-c");
                command.arg(format!("mcp_servers.elesdee.url=\"{url}\""));
            }
        }
        Client::OpenCode2 => {
            let session = entry.session();
            let plugin = prepare_skill_kit(entry, data_dir, session_id)?;
            let path = crate::mcp::write_opencode_config(
                data_dir,
                session_id,
                state.port(),
                session.model.as_deref(),
                &session.config,
                &plugin.join("skills"),
            )
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

pub(crate) fn start_opencode_poller(state: AppState, entry: Arc<SessionEntry>, base_url: String) {
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
                        && let Some(updated) = entry.clear_external_permission(&pending.request_id)
                    {
                        state.emit(SessionEvent::updated(updated));
                    }
                }
            }
            if entry.session().agent_session_id.is_none()
                && let Ok(sessions) = crate::opencode::sessions(&base_url).await
            {
                capture_opencode_session(&state, &entry, &sessions);
            }
            if let Some(agent_session_id) = entry.session().agent_session_id
                && let Ok(messages) = crate::opencode::messages(&base_url, &agent_session_id).await
            {
                for change in crate::history::opencode_changes(&messages) {
                    entry.record_history(change);
                }
            }
        }
    });
}

fn capture_opencode_session(
    state: &AppState,
    entry: &SessionEntry,
    sessions: &[crate::opencode::SessionSummary],
) {
    let folder = PathBuf::from(entry.session().folder);
    let folder = folder.canonicalize().unwrap_or(folder);
    let newest = sessions
        .iter()
        .filter(|session| {
            session
                .directory
                .as_deref()
                .is_some_and(|directory| same_directory(directory, &folder))
        })
        .max_by_key(|session| session.updated_at());
    if let Some(newest) = newest
        && let Some(updated) = entry.note_agent_session_id(&newest.id)
    {
        state.emit(SessionEvent::updated(updated));
        state.save();
    }
}

fn same_directory(directory: &str, folder: &Path) -> bool {
    let directory = Path::new(directory);
    directory == folder
        || directory
            .canonicalize()
            .is_ok_and(|directory| directory == folder)
}

pub(crate) fn start_codex_runtime(
    state: &AppState,
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
    tokio::spawn(crate::codex::run(
        url,
        Arc::clone(entry),
        receiver,
        state.clone(),
    ));
    Ok(())
}

pub(crate) fn install_claude_settings(
    state: &AppState,
    entry: &SessionEntry,
    session_id: &str,
) -> Result<PathBuf, ApiError> {
    let data_dir = state
        .data_dir()
        .ok_or_else(|| ApiError::unprocessable("Agent sessions need a data directory."))?;
    let session = entry.session();
    let settings = claude::write_settings(
        data_dir,
        session_id,
        state.port(),
        session.model.as_deref(),
        &session.config,
    )
    .map_err(|error| {
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
            return Err(ApiError::bad_request(
                "An environment variable name is not valid.",
            ));
        }
        if item.value.chars().count() > 2000 || item.value.contains('\0') {
            return Err(ApiError::bad_request(
                "An environment variable value is not valid.",
            ));
        }
        if secret_like_name(name) {
            return Err(ApiError::unprocessable(
                "API keys belong in an account, not in an environment value.",
            ));
        }
        if !seen.insert(name.to_owned()) {
            return Err(ApiError::bad_request(
                "An environment variable is set twice.",
            ));
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
        && !name.contains("@{")
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
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::payload_too_large("The request body is too large.");
    }
    ApiError::bad_request(format!("Invalid request body: {rejection}"))
}

fn json_event<T: Serialize>(value: &T) -> Option<Event> {
    serde_json::to_string(value)
        .ok()
        .map(|data| Event::default().data(data))
}
