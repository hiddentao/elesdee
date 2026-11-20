use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State as AxumState;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use elesdee_core::api::router;
use elesdee_core::auth::SignInOverrides;
use elesdee_core::files::MAX_FILE_BYTES;
use elesdee_core::state::{AiConfig, Auth, Client, Provider, TestBrowserInstall};
use elesdee_core::{AppState, MemorySecretStore, SecretStore};

const TEST_HOST: &str = "127.0.0.1:4319";
const TIMEOUT: Duration = Duration::from_secs(5);

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "elesdee-core-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create temporary folder");
        Self { path }
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn request_builder(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, TEST_HOST)
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("call route");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = request_builder(method, uri);
    let request = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            builder.body(Body::from(value.to_string()))
        }
        None => builder.body(Body::empty()),
    }
    .expect("build request");

    send(app, request).await
}

async fn ensure_test_project(app: &Router, folder: &Path) {
    let (_, bootstrap) = call(app, "GET", "/api/bootstrap", None).await;
    if bootstrap["projects"]
        .as_array()
        .is_some_and(|projects| projects.iter().any(|project| project["id"] == "web"))
    {
        return;
    }

    let (status, project) = call(
        app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "web",
            "folder": folder.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create test project: {project}");
    assert_eq!(project["id"], "web");
}

async fn create_test_project(app: &Router, name: &str, folder: &Path) -> Value {
    let (status, project) = call(
        app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": name,
            "folder": folder.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create project: {project}");
    project
}

async fn create_terminal(app: &Router, folder: &Path) -> Value {
    ensure_test_project(app, folder).await;
    let (status, body) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": folder.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create terminal session: {body}");
    body
}

async fn close_session(app: &Router, id: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/api/sessions/{id}/close"),
        Some(json!({})),
    )
    .await
}

async fn write_handoff_note(app: &Router, id: &str, note: &str) {
    let (status, body) = call(
        app,
        "POST",
        &format!("/internal/mcp/{id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 91,
            "method": "tools/call",
            "params": {
                "name": "write_handoff_note",
                "arguments": { "note": note }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["isError"], false, "{body}");
}

async fn mcp_text(app: &Router, id: &str, name: &str, arguments: Value) -> (bool, String) {
    let (status, body) = call(
        app,
        "POST",
        &format!("/internal/mcp/{id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 92,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (
        body["result"]["isError"]
            .as_bool()
            .unwrap_or_else(|| panic!("missing isError: {body}")),
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

fn demo_servers(session: &Value) -> Vec<Value> {
    session["demoServers"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

async fn wait_for_file_contains(path: &Path, needle: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "file {} did not contain {needle:?}: {text:?}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn get_raw(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let request = request_builder("GET", uri)
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("call route");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}

async fn post_upload(
    app: &Router,
    uri: &str,
    name: Option<&str>,
    content_type: &str,
    bytes: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = request_builder("POST", uri).header(header::CONTENT_TYPE, content_type);
    if let Some(name) = name {
        builder = builder.header("x-el-esdee-filename", name);
    }
    let request = builder.body(Body::from(bytes)).expect("build request");
    send(app, request).await
}

async fn read_sse_until(mut body: Body, ready: impl Fn(&str) -> bool) -> String {
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + TIMEOUT;

    while !ready(&text) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, body.frame())
            .await
            .expect("SSE response timed out")
            .expect("SSE stream ended")
            .expect("SSE frame failed");
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }

    text
}

fn sse_values(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|payload| serde_json::from_str(payload.trim()).ok())
        .collect()
}

fn sse_data_text(text: &str) -> String {
    sse_values(text)
        .iter()
        .filter_map(|event| event["data"].as_str())
        .collect()
}

async fn read_output_until(mut body: Body, ready: impl Fn(&str) -> bool) -> String {
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + TIMEOUT;

    while !ready(&sse_data_text(&text)) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, body.frame())
            .await
            .expect("output stream timed out")
            .expect("output stream ended")
            .expect("output frame failed");
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }

    sse_data_text(&text)
}

async fn read_events_for(mut body: Body, duration: Duration) -> String {
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + duration;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, BodyExt::frame(&mut body)).await {
            Ok(Some(Ok(frame))) => {
                if let Ok(data) = frame.into_data() {
                    text.push_str(&String::from_utf8_lossy(&data));
                }
            }
            Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
        }
    }

    text
}

async fn wait_for_preview(app: &Router, id: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let (status, body) = call(app, "GET", "/api/bootstrap", None).await;
        assert_eq!(status, StatusCode::OK);
        let preview = body["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .find(|session| session["id"] == id)
            .and_then(|session| session["preview"].as_str())
            .unwrap_or_default()
            .to_owned();
        if preview == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "preview was {preview:?}, expected {expected:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn bootstrap_has_sample_accounts_and_no_projects() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(&app, "GET", "/api/bootstrap", None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body["projects"]
            .as_array()
            .expect("projects array")
            .is_empty(),
        "a fresh state must have no projects"
    );

    let accounts = body["providers"].as_array().expect("providers array");
    assert_eq!(accounts.len(), 4);
    let configs = body["configs"].as_array().expect("configs array");
    assert_eq!(configs.len(), 4);

    let max = accounts
        .iter()
        .find(|account| account["name"] == "Anthropic Max 20×")
        .expect("Anthropic Max provider");
    assert_eq!(
        max["supportedClients"],
        json!(["Claude Code", "OpenCode 2"])
    );
    assert_eq!(max["auth"], "subscription");

    let codex = accounts
        .iter()
        .find(|account| account["name"] == "ChatGPT Pro")
        .expect("ChatGPT Pro provider");
    assert_eq!(codex["supportedClients"], json!(["Codex", "OpenCode 2"]));

    let deepseek = accounts
        .iter()
        .find(|account| account["name"] == "DeepSeek API")
        .expect("DeepSeek provider");
    assert_eq!(
        deepseek["supportedClients"],
        json!(["Claude Code", "Codex", "OpenCode 2"])
    );

    let litellm = accounts
        .iter()
        .find(|account| account["name"] == "LiteLLM gateway")
        .expect("LiteLLM provider");
    assert_eq!(litellm["server"], "http://localhost:4000/v1");
    let litellm_config = configs
        .iter()
        .find(|config| config["name"] == "LiteLLM gateway (Codex)")
        .expect("LiteLLM config");
    assert_eq!(litellm_config["model"], "gpt-5.2");
    assert_eq!(litellm_config["client"], "Codex");

    for account in accounts {
        assert_eq!(
            account["rows"].as_array().expect("usage rows").len(),
            0,
            "seed usage rows are invented"
        );
        assert_eq!(account["foot"], "No official usage source.");
        assert_eq!(account["sample"], true, "seed providers are samples");
    }
    assert!(
        configs
            .iter()
            .all(|config| config["name"].as_str().unwrap_or_default().contains('(')),
        "seed configs are named after the provider and client"
    );

    assert_eq!(
        body["sessions"].as_array().expect("sessions array").len(),
        0
    );
}

#[tokio::test]
async fn fresh_data_dir_starts_with_no_projects() {
    let data_dir = TempDirectory::new("fresh-first-launch");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, body) = call(&app, "GET", "/api/bootstrap", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["projects"].as_array().expect("projects").is_empty());
    assert_eq!(body["providers"].as_array().expect("providers").len(), 4);
    assert_eq!(body["configs"].as_array().expect("configs").len(), 4);
    assert!(body["sessions"].as_array().expect("sessions").is_empty());
}

#[tokio::test]
async fn host_and_origin_are_validated() {
    let app = router(AppState::ephemeral());

    for host in [
        "127.0.0.1",
        "127.0.0.1:4319",
        "localhost:5173",
        "[::1]:4444",
    ] {
        let request = Request::builder()
            .method("GET")
            .uri("/api/bootstrap")
            .header(header::HOST, host)
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::OK, "allowed host {host}: {body}");
    }

    for host in [
        "evil.example:4319",
        "127.0.0.2:4319",
        "localhost:not-a-port",
        "[::2]:4319",
    ] {
        let request = Request::builder()
            .method("GET")
            .uri("/api/bootstrap")
            .header(header::HOST, host)
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "bad host {host}: {body}");
    }

    let request = Request::builder()
        .method("GET")
        .uri("/api/bootstrap")
        .body(Body::empty())
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "missing host: {body}");

    for origin in [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ] {
        let request = request_builder("GET", "/api/bootstrap")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::OK, "allowed origin {origin}: {body}");
    }

    let request = request_builder("GET", "/api/bootstrap")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "bad origin: {body}");
}

#[tokio::test]
async fn list_endpoints_keep_the_host_and_origin_rules() {
    let app = router(AppState::ephemeral());

    for path in ["/api/projects?q=", "/api/providers?q="] {
        let request = Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} host: {body}");

        let request = Request::builder()
            .method("GET")
            .uri(path)
            .header(header::HOST, "evil.example")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} host: {body}");

        let request = request_builder("GET", path)
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} origin: {body}");

        let request = request_builder("GET", path)
            .header(header::ORIGIN, "http://localhost:5173")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::OK, "{path} allowed: {body}");
    }
}

#[tokio::test]
async fn post_requests_need_json_and_close_takes_an_object() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 5".to_owned(),
    ]));
    let folder = TempDirectory::new("close-body");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let close_uri = format!("/api/sessions/{id}/close");

    let request = request_builder("POST", "/api/sessions")
        .body(Body::from(
            json!({"projectId": "web", "kind": "terminal"}).to_string(),
        ))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");

    let request = request_builder("POST", &close_uri)
        .body(Body::from("{}"))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");

    let request = request_builder("POST", &close_uri)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("{}"))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");

    let request = request_builder("POST", &close_uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "closed");
}

#[tokio::test]
async fn terminal_session_with_a_missing_folder_is_rejected() {
    let app = router(AppState::ephemeral());
    let parent = TempDirectory::new("missing-folder");
    ensure_test_project(&app, &parent.path).await;
    let missing = parent.path.join("not-here");

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": missing.to_string_lossy()
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn agent_session_with_an_unknown_account_is_rejected() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("unknown-account-project");
    ensure_test_project(&app, &folder.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": "missing-account"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn closing_an_unknown_session_returns_not_found() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(&app, "POST", "/api/sessions/unknown/close", Some(json!({}))).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn input_to_a_closed_session_returns_conflict() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 5".to_owned(),
    ]));
    let folder = TempDirectory::new("closed-input");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["status"], "active");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "closed");
    assert!(closed["workState"].is_null());
    assert!(closed["waitingFor"].is_null());

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "hello" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn output_replays_with_sequence_numbers() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf hello".to_owned(),
    ]));
    let folder = TempDirectory::new("folder with spaces");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);

    let text = read_sse_until(response.into_body(), |text| {
        text.contains("hello") && text.contains("\n\n")
    })
    .await;
    let events = sse_values(&text);

    assert!(
        events
            .iter()
            .any(|event| event["seq"] == 1 && event["data"] == "hello"),
        "output events: {text}"
    );
}

#[tokio::test]
async fn close_emits_a_session_updated_event() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 5".to_owned(),
    ]));
    let folder = TempDirectory::new("close-event");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", "/api/events")
        .body(Body::empty())
        .expect("build events request");
    let response = app.clone().oneshot(request).await.expect("open events");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let text = read_sse_until(response.into_body(), |text| {
        text.contains("session.updated") && text.contains(&id)
    })
    .await;
    let events = sse_values(&text);

    assert!(
        events.iter().any(|event| {
            event["type"] == "session.updated"
                && event["session"]["id"] == id
                && event["session"]["status"] == "closed"
        }),
        "session events: {text}"
    );
}

#[tokio::test]
async fn output_stream_ends_when_the_session_closes() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 5".to_owned(),
    ]));
    let folder = TempDirectory::new("output-close");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    loop {
        let frame = tokio::time::timeout(TIMEOUT, BodyExt::frame(&mut body))
            .await
            .expect("output stream did not end");
        match frame {
            None => break,
            Some(Ok(_)) => {}
            Some(Err(error)) => panic!("output frame failed: {error}"),
        }
    }
}

#[tokio::test]
async fn session_cap_rejects_new_sessions_and_frees_closed_slots() {
    let app = router(AppState::with_test_command_and_session_cap(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "true".to_owned()],
        2,
    ));
    let folder = TempDirectory::new("session-cap");

    let first = create_terminal(&app, &folder.path).await;
    create_terminal(&app, &folder.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": folder.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("Session limit"))
    );

    let first_id = first["id"].as_str().expect("first session id");
    let (status, closed) = close_session(&app, first_id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let replacement = create_terminal(&app, &folder.path).await;
    assert_eq!(replacement["status"], "active");
}

#[tokio::test]
async fn blocked_input_does_not_freeze_other_requests() {
    let app = router(AppState::with_test_stalled_writer(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 60".to_owned(),
    ]));
    let folder = TempDirectory::new("blocked-input");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let payload = json!({ "data": "hello" }).to_string();
    let request = request_builder("POST", &format!("/api/sessions/{id}/input"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload))
        .expect("build input request");

    let input_app = app.clone();
    let input_task = tokio::spawn(async move {
        let response = input_app.oneshot(request).await.expect("input request");
        response.status()
    });

    tokio::time::sleep(Duration::from_millis(150)).await;
    if input_task.is_finished() {
        let status = input_task.await.expect("input task failed");
        panic!("the test input finished before it could block with status {status}");
    }

    let (status, body) = tokio::time::timeout(TIMEOUT, call(&app, "GET", "/api/bootstrap", None))
        .await
        .expect("bootstrap blocked behind input");
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, closed) = tokio::time::timeout(TIMEOUT, close_session(&app, &id))
        .await
        .expect("close blocked behind input");
    assert_eq!(status, StatusCode::OK, "{closed}");

    let input_status = tokio::time::timeout(TIMEOUT, input_task)
        .await
        .expect("input stayed blocked after close")
        .expect("input task failed");
    assert!(
        input_status == StatusCode::NO_CONTENT || input_status == StatusCode::CONFLICT,
        "unexpected input status: {input_status}"
    );
}

#[tokio::test]
async fn two_kb_input_round_trip_does_not_wedge_the_session() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("input-round-trip");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let payload = "a".repeat(2048);
    let (status, body) = tokio::time::timeout(
        TIMEOUT,
        call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/input"),
            Some(json!({ "data": payload })),
        ),
    )
    .await
    .expect("2 KB input wedged the server");
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = tokio::time::timeout(TIMEOUT, call(&app, "GET", "/api/bootstrap", None))
        .await
        .expect("bootstrap blocked after 2 KB input");
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn child_exit_marks_the_session_done_and_rejects_input() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "true".to_owned(),
    ]));

    let request = request_builder("GET", "/api/events")
        .body(Body::empty())
        .expect("build events request");
    let events = app.clone().oneshot(request).await.expect("open events");
    assert_eq!(events.status(), StatusCode::OK);

    let folder = TempDirectory::new("child-exit");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let text = read_sse_until(events.into_body(), |text| {
        text.contains(&id) && text.contains("\"workState\":\"done\"")
    })
    .await;
    let updates = sse_values(&text);
    assert!(
        updates.iter().any(|event| {
            event["type"] == "session.updated"
                && event["session"]["id"] == id
                && event["session"]["status"] == "active"
                && event["session"]["workState"] == "done"
        }),
        "child exit events: {text}"
    );

    let (status, body) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let stored = body["sessions"]
        .as_array()
        .expect("sessions array")
        .iter()
        .find(|stored| stored["id"] == id)
        .cloned()
        .expect("stored session");
    assert_eq!(stored["status"], "active");
    assert_eq!(stored["workState"], "done");
    assert!(stored["waitingFor"].is_null());

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "hello" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn output_after_is_bounded_by_the_newest_sequence() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf hello; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("after-bound");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("hello")).await;
    assert!(sse_values(&text).iter().any(|event| event["seq"] == 1));

    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/output?after=999999"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("newest"))
    );

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=1"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn output_after_replays_only_later_sequences() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf first; sleep 0.3; printf second; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("after-mid-log");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| {
        text.contains("first") && text.contains("second")
    })
    .await;
    let events = sse_values(&text);
    let first_seq = events
        .iter()
        .find(|event| event["data"] == "first")
        .and_then(|event| event["seq"].as_u64())
        .expect("first sequence");
    let second_seq = events
        .iter()
        .find(|event| event["data"] == "second")
        .and_then(|event| event["seq"].as_u64())
        .expect("second sequence");
    assert!(second_seq > first_seq);

    let request = request_builder(
        "GET",
        &format!("/api/sessions/{id}/output?after={first_seq}"),
    )
    .body(Body::empty())
    .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("second")).await;
    let replayed = sse_values(&text);
    assert!(
        replayed
            .iter()
            .any(|event| event["seq"] == second_seq && event["data"] == "second")
    );
    assert!(!text.contains("\"data\":\"first\""));

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn rolled_output_log_signals_the_gap() {
    let script = concat!(
        "i=0; while [ $i -lt 4000 ]; do ",
        "printf '0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789\\n'; ",
        "i=$((i+1)); done; touch gap-done; sleep 30"
    );
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
    ]));
    let folder = TempDirectory::new("log-gap");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let marker = folder.path.join("gap-done");
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !marker.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "log writer did not finish"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("\"gap\"")).await;
    let events = sse_values(&text);
    let gap = events
        .iter()
        .find_map(|event| event.get("gap"))
        .expect("gap metadata");

    assert!(gap["oldestSeq"].as_u64().is_some_and(|seq| seq > 1));
    assert!(
        gap["newestSeq"]
            .as_u64()
            .is_some_and(|newest| newest >= gap["oldestSeq"].as_u64().unwrap_or_default())
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[cfg(unix)]
#[tokio::test]
async fn close_kills_a_nohup_child() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("nohup-child");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({
            "data": "nohup sleep 60 >/dev/null 2>&1 & echo CHILD:$!\r"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| child_pid(text).is_some()).await;
    let pid = child_pid(&text).expect("child pid in output");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while process_alive(pid) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!process_alive(pid), "nohup child {pid} survived close");
}

#[tokio::test]
async fn agent_sessions_use_the_supplied_folder_and_unique_short_names() {
    let data_dir = TempDirectory::new("agent-data");
    let repo = TempDirectory::new("agent-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let body = json!({
        "projectId": "web",
        "kind": "agent",
        "providerId": "max",
            "client": "Claude Code",
        "name": "Fix login",
        "folder": repo.path.to_string_lossy()
    });

    let (status, first) = call(&app, "POST", "/api/sessions", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = call(&app, "POST", "/api/sessions", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{second}");

    let data_dir = data_dir.path.clone();
    assert_eq!(first["branch"], "elesdee/web/fix-login");
    assert_eq!(second["branch"], "elesdee/web/fix-login-2");
    assert_eq!(first["client"], "Claude Code");
    assert_eq!(first["shortcuts"].as_array().expect("shortcuts").len(), 5);
    assert_eq!(first["short"], "fix-login");
    assert_eq!(second["short"], "fix-login-2");
    let first_worktree = data_dir
        .join("worktrees")
        .join(first["id"].as_str().expect("session id"));
    assert_eq!(first["folder"], first_worktree.to_string_lossy().as_ref());
    assert!(first_worktree.is_dir());
    assert!(git_ref_exists(
        &repo.path,
        "refs/heads/elesdee/web/fix-login"
    ));
    assert!(git_ref_exists(
        &repo.path,
        "refs/heads/elesdee/web/fix-login-2"
    ));

    for session in [&first, &second] {
        let id = session["id"].as_str().expect("session id");
        let (status, closed) = close_session(&app, id).await;
        assert_eq!(status, StatusCode::OK, "{closed}");
    }
    assert!(!first_worktree.exists());
    assert!(git_ref_exists(
        &repo.path,
        "refs/heads/elesdee/web/fix-login"
    ));
}

#[tokio::test]
async fn branch_listing_reports_local_and_remote_branches() {
    let repo = TempDirectory::new("branch-list");
    init_git_repo(&repo.path);
    assert!(
        run_git(&repo.path, &["checkout", "-b", "feature"])
            .status
            .success(),
        "create feature branch"
    );
    assert!(run_git(&repo.path, &["checkout", "main"]).status.success());
    let app = router(AppState::ephemeral());
    ensure_test_project(&app, &repo.path).await;

    let (status, branches) = call(
        &app,
        "GET",
        &format!(
            "/api/branches?folder={}",
            url_escape(&repo.path.to_string_lossy())
        ),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{branches}");
    let branches = branches.as_array().expect("branch list");
    let names = branches
        .iter()
        .map(|branch| branch["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(names.contains(&"main"), "{names:?}");
    assert!(names.contains(&"feature"), "{names:?}");
    let main = branches
        .iter()
        .find(|branch| branch["name"] == "main")
        .expect("main branch");
    assert_eq!(main["current"], true);
}

#[tokio::test]
async fn session_name_comes_from_the_first_prompt() {
    let data_dir = TempDirectory::new("prompt-data");
    let repo = TempDirectory::new("prompt-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "folder": repo.path.to_string_lossy(),
            "firstPrompt": "Fix the login redirect\nand add a test"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["name"], "Fix the login redirect");
    assert_eq!(
        session["firstPrompt"],
        "Fix the login redirect\nand add a test"
    );
    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn source_branch_sets_the_worktree_base() {
    let data_dir = TempDirectory::new("source-data");
    let repo = TempDirectory::new("source-repo");
    init_git_repo(&repo.path);
    assert!(
        run_git(&repo.path, &["checkout", "-b", "feature"])
            .status
            .success(),
        "create feature branch"
    );
    fs::write(repo.path.join("feature.txt"), "from feature\n").expect("write feature file");
    assert!(run_git(&repo.path, &["add", "."]).status.success());
    assert!(
        run_git(
            &repo.path,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "feature commit"
            ]
        )
        .status
        .success()
    );
    assert!(run_git(&repo.path, &["checkout", "main"]).status.success());
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "From feature",
            "folder": repo.path.to_string_lossy(),
            "sourceBranch": "feature"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert!(
        folder.join("feature.txt").is_file(),
        "worktree did not start from the source branch"
    );
    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_source_branch_is_rejected() {
    let data_dir = TempDirectory::new("source-missing-data");
    let repo = TempDirectory::new("source-missing-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Missing base",
            "folder": repo.path.to_string_lossy(),
            "sourceBranch": "no-such-branch"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("source branch"))
    );
    assert_eq!(
        git_ref_count(&repo.path),
        1,
        "a failed launch left a branch"
    );
}

fn test_app(data_dir: &Path) -> Router {
    router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.to_path_buf(),
    ))
}

fn push_to_remote(repo: &Path, remote: &str, branch: &str) {
    let output = run_git(repo, &["push", "-q", remote, branch]);
    assert!(
        output.status.success(),
        "git push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clone_remote(remote: &Path, parent: &Path, name: &str) -> PathBuf {
    let target = parent.join(name);
    let output = Command::new("git")
        .args(["clone", "--quiet"])
        .arg(remote)
        .arg(&target)
        .output()
        .expect("run git clone");
    assert!(
        output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    target
}

fn commit_file(repo: &Path, name: &str, content: &str, message: &str) {
    fs::write(repo.join(name), content).expect("write commit file");
    assert!(run_git(repo, &["add", "."]).status.success());
    let output = run_git(
        repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            message,
        ],
    );
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn launch_agent(
    app: &Router,
    repo: &Path,
    name: &str,
    source: Option<&str>,
    copy: bool,
) -> (StatusCode, Value) {
    let mut body = json!({
        "projectId": "web",
        "kind": "agent",
        "providerId": "max",
            "client": "Claude Code",
        "name": name,
        "folder": repo.to_string_lossy(),
        "copyUncommittedChanges": copy
    });
    if let Some(source) = source {
        body["sourceBranch"] = Value::String(source.to_owned());
    }
    call(app, "POST", "/api/sessions", Some(body)).await
}

#[tokio::test]
async fn launch_fetches_a_tracking_branch_and_keeps_the_checkout() {
    let data_dir = TempDirectory::new("sync-data");
    let repo = TempDirectory::new("sync-repo");
    let remote_dir = TempDirectory::new("sync-remote");
    let clone_dir = TempDirectory::new("sync-clone");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);
    push_to_remote(&repo.path, "origin", "main");
    assert!(
        run_git(
            &repo.path,
            &["branch", "--set-upstream-to=origin/main", "main"]
        )
        .status
        .success(),
        "set the tracking branch"
    );
    let before = git_stdout(&repo.path, &["rev-parse", "main"]);

    let clone = clone_remote(&remote, &clone_dir.path, "clone");
    commit_file(&clone, "remote.txt", "from remote\n", "remote update");
    push_to_remote(&clone, "origin", "main");

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = launch_agent(&app, &repo.path, "Sync Main", Some("main"), false).await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert_eq!(
        fs::read_to_string(folder.join("remote.txt")).expect("remote file"),
        "from remote\n"
    );
    assert_eq!(
        git_stdout(&repo.path, &["rev-parse", "main"]),
        before,
        "the checked-out branch moved"
    );
    assert_eq!(
        git_stdout(&repo.path, &["status", "--porcelain"]),
        "",
        "the checkout changed"
    );

    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_fast_forwards_a_tracking_branch_that_is_not_checked_out() {
    let data_dir = TempDirectory::new("sync-work-data");
    let repo = TempDirectory::new("sync-work-repo");
    let remote_dir = TempDirectory::new("sync-work-remote");
    let clone_dir = TempDirectory::new("sync-work-clone");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);
    push_to_remote(&repo.path, "origin", "main");
    assert!(
        run_git(&repo.path, &["checkout", "-q", "-b", "work"])
            .status
            .success()
    );
    assert!(
        run_git(&repo.path, &["push", "-q", "-u", "origin", "work"])
            .status
            .success(),
        "push the work branch with its upstream"
    );
    assert!(
        run_git(&repo.path, &["checkout", "-q", "main"])
            .status
            .success()
    );
    let before = git_stdout(&repo.path, &["rev-parse", "work"]);

    let clone = clone_remote(&remote, &clone_dir.path, "clone");
    assert!(
        run_git(&clone, &["checkout", "-q", "work"])
            .status
            .success()
    );
    commit_file(&clone, "later.txt", "from the remote\n", "work update");
    push_to_remote(&clone, "origin", "work");

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = launch_agent(&app, &repo.path, "Sync Work", Some("work"), false).await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert!(folder.join("later.txt").is_file(), "the worktree is stale");
    assert_ne!(git_stdout(&repo.path, &["rev-parse", "work"]), before);
    assert_eq!(
        git_stdout(&repo.path, &["rev-parse", "work"]),
        git_stdout(&repo.path, &["rev-parse", "origin/work"]),
        "the local branch did not move to its upstream"
    );

    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_from_a_remote_branch_fetches_the_latest_commit() {
    let data_dir = TempDirectory::new("sync-remote-data");
    let repo = TempDirectory::new("sync-remote-repo");
    let remote_dir = TempDirectory::new("sync-remote-bare");
    let clone_dir = TempDirectory::new("sync-remote-clone");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);
    push_to_remote(&repo.path, "origin", "main");
    assert!(
        run_git(&repo.path, &["fetch", "-q", "origin"])
            .status
            .success()
    );

    let clone = clone_remote(&remote, &clone_dir.path, "clone");
    commit_file(&clone, "fresh.txt", "fresh remote\n", "remote update");
    push_to_remote(&clone, "origin", "main");

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, session) =
        launch_agent(&app, &repo.path, "Remote Main", Some("origin/main"), false).await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert_eq!(
        fs::read_to_string(folder.join("fresh.txt")).expect("fresh file"),
        "fresh remote\n"
    );

    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_without_a_remote_uses_the_local_branch() {
    let data_dir = TempDirectory::new("sync-none-data");
    let repo = TempDirectory::new("sync-none-repo");
    init_git_repo(&repo.path);
    assert!(
        run_git(&repo.path, &["checkout", "-q", "-b", "feature"])
            .status
            .success()
    );
    commit_file(
        &repo.path,
        "feature.txt",
        "local feature\n",
        "feature commit",
    );
    assert!(
        run_git(&repo.path, &["checkout", "-q", "main"])
            .status
            .success()
    );

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, session) =
        launch_agent(&app, &repo.path, "No Remote", Some("feature"), false).await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert!(folder.join("feature.txt").is_file());

    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_without_an_upstream_does_not_fetch() {
    let data_dir = TempDirectory::new("sync-upstream-data");
    let repo = TempDirectory::new("sync-upstream-repo");
    init_git_repo(&repo.path);
    add_origin(&repo.path, &repo.path.join("missing-remote.git"));

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, session) =
        launch_agent(&app, &repo.path, "No Upstream", Some("main"), false).await;

    assert_eq!(status, StatusCode::OK, "{session}");
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    assert!(folder.join("README.md").is_file());

    let id = session["id"].as_str().expect("session id");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_reports_a_failed_fetch() {
    let data_dir = TempDirectory::new("sync-fail-data");
    let repo = TempDirectory::new("sync-fail-repo");
    init_git_repo(&repo.path);
    add_origin(&repo.path, &repo.path.join("missing-remote.git"));
    assert!(
        run_git(&repo.path, &["config", "branch.main.remote", "origin"])
            .status
            .success()
    );
    assert!(
        run_git(
            &repo.path,
            &["config", "branch.main.merge", "refs/heads/main"]
        )
        .status
        .success()
    );

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let (status, body) = launch_agent(&app, &repo.path, "Fetch Fails", Some("main"), false).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("fetch")),
        "{body}"
    );
    assert_eq!(
        git_ref_count(&repo.path),
        1,
        "a failed launch left a branch"
    );
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap["sessions"].as_array().map(Vec::len),
        Some(0),
        "a failed launch left a session"
    );
}

#[tokio::test]
async fn launch_stops_when_the_tracking_branch_has_diverged() {
    let data_dir = TempDirectory::new("sync-diverge-data");
    let repo = TempDirectory::new("sync-diverge-repo");
    let remote_dir = TempDirectory::new("sync-diverge-remote");
    let clone_dir = TempDirectory::new("sync-diverge-clone");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);
    push_to_remote(&repo.path, "origin", "main");
    assert!(
        run_git(
            &repo.path,
            &["branch", "--set-upstream-to=origin/main", "main"]
        )
        .status
        .success()
    );
    commit_file(&repo.path, "local.txt", "local work\n", "local commit");

    let clone = clone_remote(&remote, &clone_dir.path, "clone");
    commit_file(&clone, "remote.txt", "remote work\n", "remote commit");
    push_to_remote(&clone, "origin", "main");

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;
    let before = git_stdout(&repo.path, &["rev-parse", "main"]);
    let (status, body) = launch_agent(&app, &repo.path, "Diverged", Some("main"), false).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("diverged")),
        "{body}"
    );
    assert_eq!(git_stdout(&repo.path, &["rev-parse", "main"]), before);
    assert_eq!(git_ref_count(&repo.path), 1);
}

#[tokio::test]
async fn launch_copies_chosen_uncommitted_changes() {
    let data_dir = TempDirectory::new("copy-data");
    let repo = TempDirectory::new("copy-repo");
    init_git_repo(&repo.path);
    fs::write(repo.path.join("README.md"), "dirty change\n").expect("dirty README");
    fs::write(repo.path.join("notes.txt"), "untracked notes\n").expect("untracked file");

    let app = test_app(&data_dir.path);
    ensure_test_project(&app, &repo.path).await;

    let (status, copied) = launch_agent(&app, &repo.path, "Copy Changes", Some("main"), true).await;
    assert_eq!(status, StatusCode::OK, "{copied}");
    let copied_folder = PathBuf::from(copied["folder"].as_str().expect("session folder"));
    assert_eq!(
        fs::read_to_string(copied_folder.join("README.md")).expect("copied README"),
        "dirty change\n"
    );
    assert_eq!(
        fs::read_to_string(copied_folder.join("notes.txt")).expect("copied notes"),
        "untracked notes\n"
    );

    let (status, left) = launch_agent(&app, &repo.path, "Leave Changes", Some("main"), false).await;
    assert_eq!(status, StatusCode::OK, "{left}");
    let left_folder = PathBuf::from(left["folder"].as_str().expect("session folder"));
    assert_eq!(
        fs::read_to_string(left_folder.join("README.md")).expect("base README"),
        "initial\n"
    );
    assert!(!left_folder.join("notes.txt").exists());

    assert_eq!(
        fs::read_to_string(repo.path.join("README.md")).expect("checkout README"),
        "dirty change\n",
        "the checkout changed"
    );
    assert!(repo.path.join("notes.txt").is_file());

    for session in [copied, left] {
        let id = session["id"].as_str().expect("session id");
        let (status, _) = close_session(&app, id).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn git_status_reports_dirty_work_and_plain_folders() {
    let data_dir = TempDirectory::new("status-data");
    let repo = TempDirectory::new("status-repo");
    let plain = TempDirectory::new("status-plain");
    init_git_repo(&repo.path);
    let app = test_app(&data_dir.path);

    let clean = format!(
        "/api/git-status?folder={}",
        url_escape(&repo.path.to_string_lossy())
    );
    let (status, body) = call(&app, "GET", &clean, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["git"], true);
    assert_eq!(body["dirty"], false);

    fs::write(repo.path.join("notes.txt"), "notes\n").expect("write notes");
    let (status, body) = call(&app, "GET", &clean, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["dirty"], true);

    let plain_url = format!(
        "/api/git-status?folder={}",
        url_escape(&plain.path.to_string_lossy())
    );
    let (status, body) = call(&app, "GET", &plain_url, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["git"], false);
    assert_eq!(body["dirty"], false);
}

fn url_escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('#', "%23")
}

fn git_ref_count(repo: &Path) -> usize {
    let output = run_git(repo, &["for-each-ref", "--format=%(refname)", "refs/heads"]);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

#[tokio::test]
async fn account_switch_relaunches_the_session_and_keeps_it_active() {
    let data_dir = TempDirectory::new("switch-data");
    let repo = TempDirectory::new("switch-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Switch A", "sk-switch-a").await;
    let second = create_test_account(&app, "Switch B", "sk-switch-b").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": first,
            "name": "Switch me",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");

    let (_, settings) = claude_settings(&data_dir.path, id);
    let token = settings_token(&settings);
    let (status, body) = send_statusline(
        &app,
        id,
        Some(&token),
        json!({ "context_window": { "remaining_percentage": 71 } }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap_session(&bootstrap, id)["contextLeft"], 71);

    let (status, switched) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": second })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["providerId"], second);
    assert_eq!(switched["status"], "active");
    assert_eq!(switched["client"], "Claude Code");
    assert_eq!(switched["contextLeft"], Value::Null);

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn status_line_reports_context_left_for_claude_sessions_only() {
    let data_dir = TempDirectory::new("context-data");
    let repo = TempDirectory::new("context-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Context left").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let (status, body) = send_statusline(
        &app,
        &id,
        Some(&token),
        json!({
            "session_id": "claude-context",
            "context_window": {
                "context_window_size": 200000,
                "used_percentage": 36.6,
                "remaining_percentage": 63.4
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap_session(&bootstrap, &id)["contextLeft"], 63);

    let (status, body) = send_statusline(
        &app,
        &id,
        Some(&token),
        json!({
            "session_id": "claude-context",
            "context_window": { "remaining_percentage": null }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap_session(&bootstrap, &id)["contextLeft"],
        Value::Null
    );

    let (status, body) = send_statusline(
        &app,
        &id,
        Some(&token),
        json!({ "context_window": { "remaining_percentage": 140 } }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap_session(&bootstrap, &id)["contextLeft"], 100);

    let (status, body) = send_statusline(
        &app,
        &id,
        Some("wrong-token"),
        json!({ "context_window": { "remaining_percentage": 40 } }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let codex = create_agent(&app, &repo.path, "codex", "Context codex").await;
    let codex_id = codex["id"].as_str().expect("codex session id");
    let (status, body) = send_statusline(
        &app,
        codex_id,
        Some(&token),
        json!({ "context_window": { "remaining_percentage": 40 } }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn switch_options_list_every_account_and_the_next_automatic_target() {
    let data_dir = TempDirectory::new("switch-options-data");
    let repo = TempDirectory::new("switch-options-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Options A", "sk-options-a").await;
    let second = create_test_account(&app, "Options B", "sk-options-b").await;
    let codex = create_test_account_for(&app, "Options Codex", "openai", "sk-options-codex").await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let id = create_agent_session(&app, &repo.path, &first, "Options session").await;
    let (status, options) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/switch-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let first_config = config_for_account(&app, &first).await;
    let second_config = config_for_account(&app, &second).await;
    let codex_config = config_for_account(&app, &codex).await;
    assert_eq!(options["nextConfigId"], second_config);
    assert_eq!(
        options["allowedConfigIds"],
        json!([&first_config, &second_config]),
        "the default allows the starting client"
    );
    let ids = options["configs"]
        .as_array()
        .expect("configs")
        .iter()
        .filter_map(|config| config["id"].as_str())
        .collect::<Vec<_>>();
    for expected in [
        first_config.as_str(),
        second_config.as_str(),
        codex_config.as_str(),
    ] {
        assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
    }

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, options) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/switch-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["nextConfigId"], Value::Null);

    let terminal = create_terminal(&app, &repo.path).await;
    let terminal_id = terminal["id"].as_str().expect("terminal id");
    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{terminal_id}/switch-options"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(&app, "GET", "/api/sessions/unknown/switch-options", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn usage_near_the_limit_switches_then_pauses_without_room() {
    let data_dir = TempDirectory::new("auto-data");
    let repo = TempDirectory::new("auto-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Auto A", "sk-auto-a").await;
    let second = create_test_account(&app, "Auto B", "sk-auto-b").await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": first,
            "name": "Auto switch",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, id, |session| session["providerId"] == second).await;
    assert_eq!(switched["status"], "active");
    assert_eq!(switched["workState"], "working");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let paused = wait_for_session(&app, id, |session| session["workState"] == "paused").await;
    assert_eq!(paused["status"], "active");
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("No allowed AI config"))
    );

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 30 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resumed = wait_for_session(&app, id, |session| session["workState"] == "working").await;
    assert_eq!(resumed["providerId"], first);

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn allowed_accounts_are_saved_and_guard_the_switch() {
    let data_dir = TempDirectory::new("allowed-data");
    let repo = TempDirectory::new("allowed-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Allowed A", "sk-allowed-a").await;
    let second = create_test_account(&app, "Allowed B", "sk-allowed-b").await;
    let first_config = config_for_account(&app, &first).await;
    let second_config = config_for_account(&app, &second).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &first,
            "allowedAccountIds": [&first],
            "name": "Allowed list",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["allowedConfigIds"], json!([&first_config]));

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "allowedAccountIds": [&second] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "allowedAccountIds": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "allowedAccountIds": ["missing"] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "allowedAccountIds": [&first, &second] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["allowedConfigIds"],
        json!([&first_config, &second_config])
    );

    let (status, switched) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": &second })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["providerId"], second);

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "allowedAccountIds": [&second] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, refused) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": &first })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| error.contains("not allowed"))
    );

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    let state: Value = serde_json::from_str(&state_text).expect("state json");
    let stored = state["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|session| session["session"]["id"] == id)
        })
        .expect("stored session");
    assert_eq!(
        stored["session"]["allowedConfigIds"],
        json!([&second_config])
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auto_switch_prefers_subscriptions_then_crosses_to_pay_per_use() {
    let data_dir = TempDirectory::new("order-data");
    let repo = TempDirectory::new("order-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let claude = create_test_subscription_account(&app, "Order Claude", "anthropic").await;
    let codex = create_test_subscription_account(&app, "Order Codex", "openai").await;
    let deepseek = create_test_account_for(&app, "Order DeepSeek", "deepseek", "sk-order").await;

    for (account, used) in [(&codex, 50), (&deepseek, 5)] {
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/providers/{account}/usage"),
            Some(json!({ "rows": [{ "label": "Weekly", "used": used }] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &claude,
            "allowedAccountIds": [&claude, &codex, &deepseek],
            "name": "Order switch",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{claude}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, &id, |session| session["providerId"] == codex).await;
    assert_eq!(
        switched["client"], "Codex",
        "a subscription account beats a pay-per-use account"
    );

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{codex}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, &id, |session| session["providerId"] == deepseek).await;
    assert_eq!(switched["client"], "Claude Code");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auto_switch_follows_the_prepaid_minimum_balance() {
    let data_dir = TempDirectory::new("balance-data");
    let repo = TempDirectory::new("balance-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let starter = create_test_subscription_account(&app, "Balance Claude", "anthropic").await;
    let deepseek =
        create_test_account_for(&app, "Balance DeepSeek", "deepseek", "sk-balance").await;

    let (status, provider) = call(
        &app,
        "GET",
        &format!("/api/providers/{deepseek}/usage"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    assert_eq!(provider["minimumBalance"], json!(5.0), "the default floor");
    let (status, cleared) = call(
        &app,
        "PATCH",
        &format!("/api/providers/{deepseek}"),
        Some(json!({ "minimumBalance": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert!(
        cleared["minimumBalance"].is_null(),
        "an explicit null clears"
    );
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/api/providers/{deepseek}"),
        Some(json!({ "minimumBalance": -1 })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a negative floor is refused"
    );
    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/providers/{deepseek}"),
        Some(json!({ "minimumBalance": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["minimumBalance"], json!(10.0));

    let set_balance = |balance: &'static str| {
        let app = app.clone();
        let deepseek = deepseek.clone();
        async move {
            let (status, _) = call(
                &app,
                "POST",
                &format!("/api/providers/{deepseek}/usage"),
                Some(json!({
                    "rows": [{
                        "label": "Balance",
                        "window": "balance",
                        "value": balance
                    }]
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
    };

    set_balance("USD 11.00").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &starter,
            "allowedAccountIds": [&starter, &deepseek],
            "name": "Balance switch",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{starter}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let switched = wait_for_session(&app, &id, |session| session["providerId"] == deepseek).await;
    assert_eq!(
        switched["workState"], "working",
        "a balance above the floor has room"
    );

    set_balance("USD 10.00").await;
    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert_eq!(
        paused["providerId"], deepseek,
        "a balance at the floor has no room"
    );

    set_balance("USD 9.99").await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap_session(&bootstrap, &id)["workState"],
        "paused",
        "a balance below the floor has no room"
    );

    set_balance("USD 10.01").await;
    let resumed = wait_for_session(&app, &id, |session| session["workState"] == "working").await;
    assert_eq!(
        resumed["providerId"], deepseek,
        "the floor is a strict boundary"
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auto_switch_keeps_the_current_client_when_subscriptions_tie() {
    let data_dir = TempDirectory::new("tie-data");
    let repo = TempDirectory::new("tie-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let starter = create_test_subscription_account(&app, "Tie Starter", "anthropic").await;
    let same = create_test_subscription_account(&app, "Tie Same", "anthropic").await;
    let other = create_test_subscription_account(&app, "Tie Other", "openai").await;

    for account in [&same, &other] {
        let (status, _) = call(
            &app,
            "POST",
            &format!("/api/providers/{account}/usage"),
            Some(json!({ "rows": [{ "label": "Weekly", "used": 60 }] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &starter,
            "allowedAccountIds": [&starter, &same, &other],
            "name": "Tie switch",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{starter}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, &id, |session| session["providerId"] == same).await;
    assert_eq!(switched["client"], "Claude Code");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auto_switch_to_another_client_delivers_the_handoff_note() {
    let data_dir = TempDirectory::new("auto-handoff-data");
    let repo = TempDirectory::new("auto-handoff-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "cat >> handoff-input.txt".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let claude = create_test_subscription_account(&app, "Auto handoff Claude", "anthropic").await;
    let codex = create_test_subscription_account(&app, "Auto handoff Codex", "openai").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{codex}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &claude,
            "allowedAccountIds": [&claude, &codex],
            "name": "Auto handoff",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let input_file =
        PathBuf::from(session["folder"].as_str().expect("folder")).join("handoff-input.txt");
    let note = "Goal: move the session to Codex.";
    write_handoff_note(&app, &id, note).await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{claude}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, &id, |session| session["providerId"] == codex).await;
    assert_eq!(switched["client"], "Codex");
    let text = wait_for_file_contains(&input_file, "Handoff from the previous agent").await;
    assert!(text.contains(note), "{text}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auto_switch_pauses_when_only_disallowed_accounts_have_room() {
    let data_dir = TempDirectory::new("allowed-pause-data");
    let repo = TempDirectory::new("allowed-pause-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Pause allowed", "sk-pause-allowed").await;
    let second = create_test_account(&app, "Pause refused", "sk-pause-refused").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &first,
            "allowedAccountIds": [&first],
            "name": "Allowed pause",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert_eq!(paused["providerId"], first);
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("No allowed AI config"))
    );

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 30 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resumed = wait_for_session(&app, &id, |session| session["workState"] == "working").await;
    assert_eq!(resumed["providerId"], first);

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn pause_tells_the_agent_to_finish_and_saves_the_work() {
    let data_dir = TempDirectory::new("pause-finish-data");
    let repo = TempDirectory::new("pause-finish-repo");
    let remote_dir = TempDirectory::new("pause-finish-remote");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);

    let app = router(
        AppState::with_test_command_and_data_dir(
            vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "cat >> pause-input.txt".to_owned(),
            ],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(300)),
    );
    ensure_test_project(&app, &repo.path).await;
    let only = create_test_account(&app, "Pause finish", "sk-pause-finish").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &only,
            "allowedAccountIds": [&only],
            "name": "Pause finish",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "saved work\n").expect("write worktree file");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{only}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert_eq!(paused["status"], "active");
    assert!(worktree.exists(), "a pause must keep the worktree");
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("committed and pushed")),
        "{}",
        paused["preview"]
    );

    let instruction = fs::read_to_string(worktree.join("pause-input.txt")).expect("pause input");
    assert!(
        instruction.contains("commit all your work"),
        "{instruction}"
    );
    assert!(
        instruction.contains("write the handoff note"),
        "{instruction}"
    );
    assert!(instruction.contains("push the branch"), "{instruction}");

    assert_eq!(
        git_stdout(&worktree, &["status", "--porcelain"]),
        "",
        "the pause left uncommitted work"
    );
    assert_eq!(
        git_stdout(&repo.path, &["show", &format!("{branch}:new.txt")]),
        "saved work"
    );
    assert!(remote_ref_exists(&remote, &format!("refs/heads/{branch}")));

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn pause_keeps_the_worktree_when_the_push_fails() {
    let data_dir = TempDirectory::new("pause-push-fail-data");
    let repo = TempDirectory::new("pause-push-fail-repo");
    init_git_repo(&repo.path);
    add_origin(&repo.path, &repo.path.join("missing-remote.git"));

    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let only = create_test_account(&app, "Pause push fail", "sk-pause-push-fail").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &only,
            "allowedAccountIds": [&only],
            "name": "Pause push fail",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "kept work\n").expect("write worktree file");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{only}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert!(worktree.exists(), "a failed push must keep the worktree");
    assert_eq!(
        fs::read_to_string(worktree.join("new.txt")).expect("kept file"),
        "kept work\n"
    );
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("push failed")),
        "{}",
        paused["preview"]
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn agent_reported_limit_switches_at_once() {
    let data_dir = TempDirectory::new("reported-limit-data");
    let repo = TempDirectory::new("reported-limit-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Report A", "sk-report-a").await;
    let second = create_test_account(&app, "Report B", "sk-report-b").await;
    let second_config = config_for_account(&app, &second).await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &first,
            "allowedAccountIds": [&first, &second],
            "name": "Reported limit",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(&app, &id, "report_limit", json!({})).await;
    assert!(!failed, "{text}");
    assert!(text.contains("switch or pause"), "{text}");

    let switched =
        wait_for_session(&app, &id, |session| session["configId"] == second_config).await;
    assert_eq!(switched["status"], "active");
    assert_eq!(switched["workState"], "working");

    let (_, providers) = call(&app, "GET", "/api/providers", None).await;
    let reported = providers
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == first)
        .expect("reported provider");
    assert!(
        reported["limitReportedAt"].is_string(),
        "the report was not stored: {reported}"
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn agent_reported_limit_pauses_when_no_config_has_room() {
    let data_dir = TempDirectory::new("reported-pause-data");
    let repo = TempDirectory::new("reported-pause-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let only = create_test_account(&app, "Report only", "sk-report-only").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &only,
            "allowedAccountIds": [&only],
            "name": "Reported pause",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(&app, &id, "report_limit", json!({})).await;
    assert!(!failed, "{text}");

    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert_eq!(paused["status"], "active");
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("No allowed AI config")),
        "{}",
        paused["preview"]
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_manual_resume_overrides_a_reported_limit() {
    let data_dir = TempDirectory::new("reported-resume-data");
    let repo = TempDirectory::new("reported-resume-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let only = create_test_account(&app, "Resume only", "sk-resume-only").await;
    let config = config_for_account(&app, &only).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &only,
            "allowedAccountIds": [&only],
            "name": "Resume reported",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(&app, &id, "report_limit", json!({})).await;
    assert!(!failed, "{text}");
    wait_for_session(&app, &id, |session| session["workState"] == "paused").await;

    let (status, resumed) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/resume"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["workState"], "working");
    assert_eq!(resumed["configId"], config);

    let (_, providers) = call(&app, "GET", "/api/providers", None).await;
    let provider = providers
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == only)
        .expect("provider");
    assert!(
        provider["limitReportedAt"].is_null(),
        "the manual resume did not clear the report: {provider}"
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_paused_session_resumes_on_another_client_with_the_note() {
    let data_dir = TempDirectory::new("pause-resume-data");
    let repo = TempDirectory::new("pause-resume-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "cat >> resume-input.txt".to_owned(),
            ],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(100)),
    );
    ensure_test_project(&app, &repo.path).await;
    let claude = create_test_account(&app, "Resume Claude", "sk-resume-claude").await;
    let codex = create_test_account_for(&app, "Resume Codex", "openai", "sk-resume-codex").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": &claude,
            "allowedAccountIds": [&claude, &codex],
            "name": "Resume note",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    let note = "Goal: resume on another client.";
    write_handoff_note(&app, &id, note).await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{claude}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let paused = wait_for_session(&app, &id, |session| session["workState"] == "paused").await;
    assert_eq!(paused["handoffNote"], note);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{codex}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resumed = wait_for_session(&app, &id, |session| {
        session["client"] == "Codex" && session["workState"] == "working"
    })
    .await;
    assert_eq!(resumed["handoffNote"], note);

    let text = wait_for_file_contains(&worktree.join("resume-input.txt"), note).await;
    assert!(
        text.contains("Handoff from the previous agent"),
        "the cross-client resume did not deliver the handoff: {text}"
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn cross_client_switch_gives_the_note_and_the_end_of_the_log() {
    let data_dir = TempDirectory::new("handoff-switch-data");
    let repo = TempDirectory::new("handoff-switch-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "cat >> handoff-input.txt".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let claude = create_test_account(&app, "Handoff Claude", "sk-handoff-claude").await;
    let codex = create_test_account_for(&app, "Handoff Codex", "openai", "sk-handoff-codex").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": claude,
            "allowedAccountIds": [claude, codex],
            "name": "Handoff switch",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    let input_file = worktree.join("handoff-input.txt");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "previous work line\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let note = "Goal: finish the parser.\nDone: the lexer.\nNext: add the tests.";
    write_handoff_note(&app, &id, note).await;

    let (status, switched) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": codex })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["client"], "Codex");
    assert_eq!(switched["handoffNote"], note);

    let text = wait_for_file_contains(&input_file, "Handoff from the previous agent").await;
    assert!(text.contains("Goal: finish the parser."), "{text}");
    assert!(text.contains("End of the previous log:"), "{text}");
    assert!(text.contains("previous work line"), "{text}");
}

#[tokio::test]
async fn same_client_switch_sends_no_handoff_context() {
    let data_dir = TempDirectory::new("handoff-same-data");
    let repo = TempDirectory::new("handoff-same-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "cat >> handoff-input.txt".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Same A", "sk-same-a").await;
    let second = create_test_account(&app, "Same B", "sk-same-b").await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": first,
            "name": "Same client",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    let input_file = worktree.join("handoff-input.txt");

    write_handoff_note(&app, &id, "Goal: keep the conversation.").await;

    let (status, switched) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": second })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["handoffNote"], "Goal: keep the conversation.");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "same client marker\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    wait_for_file_contains(&input_file, "same client marker").await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let text = fs::read_to_string(&input_file).unwrap_or_default();
    assert!(
        !text.contains("Handoff from the previous agent"),
        "the same-client switch sent a handoff context: {text}"
    );
}

#[tokio::test]
async fn pause_keeps_the_handoff_note_and_resume_keeps_it() {
    let data_dir = TempDirectory::new("handoff-pause-data");
    let repo = TempDirectory::new("handoff-pause-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_pause_grace(Duration::from_millis(50)),
    );
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Pause A", "sk-pause-a").await;
    let second = create_test_account(&app, "Pause B", "sk-pause-b").await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": first,
            "name": "Pause handoff",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let note = "Goal: survive the usage pause.";
    write_handoff_note(&app, &id, note).await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let switched =
        wait_for_session(&app, id.as_str(), |session| session["providerId"] == second).await;
    assert_eq!(switched["handoffNote"], note);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let paused = wait_for_session(&app, id.as_str(), |session| {
        session["workState"] == "paused"
    })
    .await;
    assert_eq!(paused["handoffNote"], note);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 30 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resumed = wait_for_session(&app, id.as_str(), |session| {
        session["workState"] == "working"
    })
    .await;
    assert_eq!(resumed["handoffNote"], note);

    let (status, _) = close_session(&app, id.as_str()).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn handoff_note_survives_a_reload() {
    let data_dir = TempDirectory::new("handoff-reload-data");
    let repo = TempDirectory::new("handoff-reload-repo");
    init_git_repo(&repo.path);
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Reload note",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let note = "Goal: survive a reload.";
    write_handoff_note(&app, &id, note).await;
    drop(app);

    let app = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let restored = bootstrap["sessions"]
        .as_array()
        .and_then(|sessions| sessions.iter().find(|session| session["id"] == id))
        .expect("restored session");
    assert_eq!(restored["handoffNote"], note);
}

async fn create_test_account(app: &Router, name: &str, key: &str) -> String {
    create_test_account_for(app, name, "anthropic", key).await
}

async fn create_test_subscription_account(app: &Router, name: &str, vendor: &str) -> String {
    create_account_with_auth(app, name, vendor, "subscription", None).await
}

async fn create_test_account_for(app: &Router, name: &str, vendor: &str, key: &str) -> String {
    create_account_with_auth(app, name, vendor, "key", Some(key)).await
}

async fn create_account_with_auth(
    app: &Router,
    name: &str,
    vendor: &str,
    auth: &str,
    key: Option<&str>,
) -> String {
    let (status, provider) = call(
        app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": name,
            "vendor": vendor,
            "auth": auth,
            "key": key
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    let provider_id = provider["id"].as_str().expect("provider id").to_owned();
    let clients = provider["supportedClients"]
        .as_array()
        .expect("supported clients");
    let client = clients
        .iter()
        .find(|client| client.as_str() != Some("Antigravity"))
        .or_else(|| clients.first())
        .expect("a supported client");
    let (status, config) = call(
        app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": client
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    provider_id
}

/// Finds the AI config for a provider, or returns the id when it is a config.
async fn config_for_account(app: &Router, id: &str) -> String {
    let (_, configs) = call(app, "GET", "/api/configs", None).await;
    let configs = configs.as_array().expect("configs");
    if configs.iter().any(|config| config["id"] == id) {
        return id.to_owned();
    }
    configs
        .iter()
        .find(|config| config["providerId"] == id)
        .and_then(|config| config["id"].as_str())
        .expect("a config for the provider")
        .to_owned()
}

async fn wait_for_session(app: &Router, id: &str, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (_, bootstrap) = call(app, "GET", "/api/bootstrap", None).await;
        if let Some(session) = bootstrap["sessions"]
            .as_array()
            .and_then(|sessions| sessions.iter().find(|session| session["id"] == id))
            && ready(session)
        {
            return session.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session did not reach the expected state: {bootstrap}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

const SESSION_SKILLS: [&str; 6] = [
    "elesdee",
    "ask-and-notify",
    "coordinate-sessions",
    "handoff",
    "frontend-design",
    "browser-checks",
];

fn skill_kit(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("skills").join(id)
}

fn assert_skill_set(skills_root: &Path) {
    for name in SESSION_SKILLS {
        let path = skills_root.join(name).join("SKILL.md");
        assert!(
            path.is_file(),
            "the {name} skill was not written at {}",
            path.display()
        );
        let body = fs::read_to_string(&path).expect("read skill");
        assert!(body.starts_with("---\nname: "), "{name} has no frontmatter");
        assert!(body.contains("description:"), "{name} has no description");
    }
}

fn assert_clean_repository(repo: &Path) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain"])
        .output()
        .expect("run git status");
    assert!(output.status.success(), "git status failed");
    let status = String::from_utf8_lossy(&output.stdout);
    assert!(
        status.trim().is_empty(),
        "the session wrote into the repository: {status}"
    );
}

async fn wait_for_file(path: &Path) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(path) {
            return text;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "file was not written: {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn create_agent_session(app: &Router, repo: &Path, account_id: &str, name: &str) -> String {
    let config_id = config_for_account(app, account_id).await;
    let (status, session) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": config_id,
            "name": name,
            "folder": repo.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    session["id"].as_str().expect("session id").to_owned()
}

#[tokio::test]
async fn claude_sessions_get_and_clean_up_their_skill_pack() {
    let data_dir = TempDirectory::new("mcp-config-data");
    let repo = TempDirectory::new("mcp-config-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let id = create_agent_session(&app, &repo.path, "max", "MCP config").await;

    let config_path = data_dir.path.join("mcp").join(format!("{id}.json"));
    assert!(config_path.is_file(), "the MCP config was not written");
    let config: Value = serde_json::from_slice(&fs::read(&config_path).expect("read MCP config"))
        .expect("parse MCP config");
    let url = config["mcpServers"]["elesdee"]["url"]
        .as_str()
        .expect("MCP URL");
    assert_eq!(url, format!("http://127.0.0.1:4319/internal/mcp/{id}"));

    let kit = skill_kit(&data_dir.path, &id);
    assert!(
        kit.join(".claude-plugin").join("plugin.json").is_file(),
        "the Claude plugin manifest was not written"
    );
    assert_skill_set(&kit.join("skills"));
    assert!(
        fs::read_to_string(kit.join("skills/elesdee/SKILL.md"))
            .expect("read skill")
            .contains("ask_me")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = fs::metadata(kit.join("skills/elesdee/SKILL.md"))
            .expect("skill metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        let directory_mode = fs::metadata(kit.join("skills/elesdee"))
            .expect("skill directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
    }

    assert_clean_repository(&repo.path);

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!config_path.exists(), "the MCP config was not removed");
    assert!(!kit.exists(), "the skill pack was not removed");
}

#[tokio::test]
async fn codex_sessions_get_a_private_home_with_every_skill() {
    let data_dir = TempDirectory::new("codex-skill-data");
    let repo = TempDirectory::new("codex-skill-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf '%s' \"$CODEX_HOME\" > codex-home.txt; sleep 30".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let id = create_agent_session(&app, &repo.path, "codex", "Codex skills").await;

    let session = wait_for_session(&app, &id, |session| session["branch"].is_string()).await;
    let folder = PathBuf::from(session["folder"].as_str().expect("session folder"));
    let recorded = wait_for_file(&folder.join("codex-home.txt")).await;
    let kit = skill_kit(&data_dir.path, &id);
    assert_eq!(PathBuf::from(recorded), kit.join("codex"));
    assert_skill_set(&kit.join("codex").join("skills"));
    assert_clean_repository(&repo.path);

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!kit.exists(), "the Codex skill home was not removed");
}

#[tokio::test]
async fn opencode_sessions_get_the_full_skill_set() {
    let data_dir = TempDirectory::new("opencode-skill-data");
    let repo = TempDirectory::new("opencode-skill-repo");
    init_git_repo(&repo.path);
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let provider = state.add_provider(Provider {
        id: String::new(),
        name: "OpenCode test".to_owned(),
        vendor: "openrouter".to_owned(),
        auth: Auth::Subscription,
        server: None,
        api_style: None,
        sample: false,
        connected: true,
        plan: "team".to_owned(),
        rows: Vec::new(),
        minimum_balance: None,
        foot: String::new(),
        usage_read_at: None,
        usage_checked_at: None,
        usage_status: None,
        usage_source: None,
        limit_reported_at: None,
    });
    let config = state.add_config(AiConfig {
        id: String::new(),
        name: "OpenCode test (OpenCode 2)".to_owned(),
        provider_id: provider.id,
        client: Client::OpenCode2,
        model: None,
        effort: None,
        args: Vec::new(),
        env: Vec::new(),
        config: std::collections::BTreeMap::new(),
        created_at: String::new(),
        updated_at: String::new(),
    });
    let app = router(state);
    ensure_test_project(&app, &repo.path).await;
    let id = create_agent_session(&app, &repo.path, &config.id, "OpenCode skills").await;

    let kit = skill_kit(&data_dir.path, &id);
    assert_skill_set(&kit.join("skills"));
    let config_path = data_dir
        .path
        .join("mcp")
        .join(format!("opencode-{id}.json"));
    let config: Value = serde_json::from_slice(&fs::read(&config_path).expect("read config"))
        .expect("parse config");
    assert_eq!(config["mcp"]["elesdee"]["type"], "remote");
    let skills = config["skills"].as_array().expect("skills");
    assert_eq!(skills.len(), 1);
    assert_eq!(
        PathBuf::from(skills[0].as_str().expect("skills path")),
        kit.join("skills")
    );
    assert_clean_repository(&repo.path);

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!config_path.exists(), "the OpenCode config was not removed");
    assert!(!kit.exists(), "the skill pack was not removed");
}

#[tokio::test]
async fn start_session_tool_creates_a_pending_request_that_can_be_denied() {
    let data_dir = TempDirectory::new("request-data");
    let repo = TempDirectory::new("request-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, parent) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Coordinator",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{parent}");
    let parent_id = parent["id"].as_str().expect("parent id");

    let (status, asked) = call(
        &app,
        "POST",
        &format!("/internal/mcp/{parent_id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "start_session",
                "arguments": {
                    "projectId": "web",
                    "name": "Helper",
                    "firstPrompt": "Run the tests"
                }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    let text = asked["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let request_id = text
        .strip_prefix("Session request: ")
        .expect("request id")
        .to_owned();

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let requests = bootstrap["sessionRequests"]
        .as_array()
        .expect("session requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["id"], request_id);
    assert_eq!(requests[0]["parentSessionId"], parent_id);
    assert_eq!(requests[0]["projectName"], "web");
    assert_eq!(requests[0]["firstPrompt"], "Run the tests");

    let (status, denied) = call(
        &app,
        "POST",
        &format!("/api/session-requests/{request_id}/deny"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{denied}");
    assert_eq!(denied["status"], "denied");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap["sessionRequests"]
            .as_array()
            .expect("session requests")
            .len(),
        0
    );

    let (status, _) = close_session(&app, parent_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_session_tools_ask_answer_notify_and_report_done() {
    let data_dir = TempDirectory::new("mcp-data");
    let repo = TempDirectory::new("mcp-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "MCP session",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let mcp = format!("/internal/mcp/{id}");

    let (status, init) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "elesdee");

    let (status, tools) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tools}");
    let names = tools["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    for name in [
        "ask_me",
        "wait_for_answer",
        "withdraw_question",
        "notify_me",
        "report_done",
        "write_handoff_note",
        "report_limit",
        "list_sessions",
        "read_session",
        "send_message",
        "check_usage",
        "wait_for_session",
        "share_file",
        "register_demo_server",
    ] {
        assert!(names.contains(&name), "missing tool {name}");
    }

    let (status, asked) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "ask_me",
                "arguments": { "text": "Deploy now?", "options": ["Yes", "No"] }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    let text = asked["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let question_id = text
        .strip_prefix("Question asked: ")
        .expect("question id")
        .to_owned();

    let (status, questions) = call(&app, "GET", "/api/questions", None).await;
    assert_eq!(status, StatusCode::OK, "{questions}");
    assert_eq!(questions.as_array().expect("questions").len(), 1);
    assert_eq!(questions[0]["id"], question_id);
    assert_eq!(questions[0]["options"][1], "No");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let waiting = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    assert_eq!(waiting["waitingFor"], "question");

    let (status, answered) = call(
        &app,
        "POST",
        &format!("/api/questions/{question_id}/answer"),
        Some(json!({"answer": "Yes"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    assert_eq!(answered["status"], "answered");
    assert_eq!(answered["answer"], "Yes");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let cleared = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    assert!(cleared["waitingFor"].is_null());
    assert_eq!(
        bootstrap["questions"].as_array().expect("questions").len(),
        0
    );

    let (_, second) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "ask_me", "arguments": { "text": "And now?" } }
        })),
    )
    .await;
    let second_id = second["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text")
        .strip_prefix("Question asked: ")
        .expect("question id")
        .to_owned();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/questions/{second_id}/withdraw"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, waited) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "wait_for_answer",
                "arguments": { "questionId": second_id, "timeoutSeconds": 1 }
            }
        })),
    )
    .await;
    assert_eq!(
        waited["result"]["content"][0]["text"],
        "The question was withdrawn."
    );

    let (_, notified) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "notify_me", "arguments": { "message": "Build finished." } }
        })),
    )
    .await;
    assert_eq!(notified["result"]["isError"], false);

    let (_, done) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "report_done", "arguments": {} }
        })),
    )
    .await;
    assert_eq!(done["result"]["isError"], false);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let finished = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    assert_eq!(finished["workState"], "done");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_wait_for_session_returns_on_a_state_change_and_times_out() {
    let data_dir = TempDirectory::new("mcp-wait-data");
    let repo = TempDirectory::new("mcp-wait-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let waiter = create_agent(&app, &repo.path, "max", "Waiter").await;
    let worker = create_agent(&app, &repo.path, "max", "Worker").await;
    let waiter_mcp = format!(
        "/internal/mcp/{}",
        waiter["id"].as_str().expect("waiter id")
    );
    let worker_mcp = format!(
        "/internal/mcp/{}",
        worker["id"].as_str().expect("worker id")
    );

    let (status, timed) = call(
        &app,
        "POST",
        &waiter_mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "wait_for_session",
                "arguments": {
                    "sessionId": worker["id"],
                    "until": "done",
                    "timeoutSeconds": 1
                }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{timed}");
    assert_eq!(timed["result"]["isError"], false);
    let timed_state: Value = serde_json::from_str(
        timed["result"]["content"][0]["text"]
            .as_str()
            .expect("wait text"),
    )
    .expect("wait result");
    assert_eq!(timed_state["timedOut"], true);
    assert_eq!(timed_state["workState"], "working");

    let waiting_app = app.clone();
    let waiting_mcp = waiter_mcp.clone();
    let worker_id = worker["id"].as_str().expect("worker id").to_owned();
    let waiting = tokio::spawn(async move {
        call(
            &waiting_app,
            "POST",
            &waiting_mcp,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "wait_for_session",
                    "arguments": {
                        "sessionId": worker_id,
                        "until": "done",
                        "timeoutSeconds": 5
                    }
                }
            })),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (status, done) = call(
        &app,
        "POST",
        &worker_mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "report_done", "arguments": {} }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["result"]["isError"], false);

    let (status, waited) = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("the wait tool did not return")
        .expect("join the wait task");
    assert_eq!(status, StatusCode::OK, "{waited}");
    assert_eq!(waited["result"]["isError"], false);
    let state: Value = serde_json::from_str(
        waited["result"]["content"][0]["text"]
            .as_str()
            .expect("wait text"),
    )
    .expect("wait result");
    assert_eq!(state["timedOut"], false);
    assert_eq!(state["workState"], "done");
    assert_eq!(state["status"], "active");

    let (status, _) = close_session(&app, waiter["id"].as_str().expect("waiter id")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = close_session(&app, worker["id"].as_str().expect("worker id")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_wait_for_session_stops_when_the_caller_closes() {
    let data_dir = TempDirectory::new("mcp-wait-close-data");
    let repo = TempDirectory::new("mcp-wait-close-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let waiter = create_agent(&app, &repo.path, "max", "Closing waiter").await;
    let worker = create_agent(&app, &repo.path, "max", "Quiet worker").await;
    let waiter_id = waiter["id"].as_str().expect("waiter id").to_owned();
    let waiter_mcp = format!("/internal/mcp/{waiter_id}");
    let worker_id = worker["id"].as_str().expect("worker id").to_owned();
    let waiting_worker_id = worker_id.clone();

    let waiting_app = app.clone();
    let waiting = tokio::spawn(async move {
        call(
            &waiting_app,
            "POST",
            &waiter_mcp,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "wait_for_session",
                    "arguments": { "sessionId": waiting_worker_id, "timeoutSeconds": 20 }
                }
            })),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (status, _) = close_session(&app, &waiter_id).await;
    assert_eq!(status, StatusCode::OK);

    let (status, waited) = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("the wait tool did not stop when the caller closed")
        .expect("join the wait task");
    assert_eq!(status, StatusCode::OK, "{waited}");
    assert_eq!(waited["result"]["isError"], true);
    assert_eq!(
        waited["result"]["content"][0]["text"],
        "The caller session was closed."
    );

    let (status, _) = close_session(&app, &worker_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_share_file_copies_a_file_and_refuses_paths_outside() {
    let data_dir = TempDirectory::new("mcp-share-data");
    let repo = TempDirectory::new("mcp-share-repo");
    init_git_repo(&repo.path);
    let outside = TempDirectory::new("mcp-share-outside");
    fs::write(outside.path.join("outside.txt"), "outside\n").expect("write outside file");
    fs::write(repo.path.join("notes.txt"), "hello from the session\n").expect("write notes");
    fs::write(repo.path.join(".env"), "TOKEN=secret\n").expect("write env");
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id");
    let mcp = format!("/internal/mcp/{id}");

    let (status, shared) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "share_file",
                "arguments": { "path": "notes.txt", "caption": "Session notes" }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shared}");
    assert_eq!(shared["result"]["isError"], false);
    let file: Value = serde_json::from_str(
        shared["result"]["content"][0]["text"]
            .as_str()
            .expect("share text"),
    )
    .expect("shared file");
    assert_eq!(file["name"], "notes.txt");
    assert_eq!(file["path"], "notes.txt");
    assert_eq!(file["caption"], "Session notes");
    assert_eq!(file["contentType"], "text/plain; charset=utf-8");
    let file_id = file["id"].as_str().expect("shared file id");

    let (status, files) = call(&app, "GET", &format!("/api/sessions/{id}/shared"), None).await;
    assert_eq!(status, StatusCode::OK, "{files}");
    let listed = files.as_array().expect("shared files");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], file_id);

    let (status, headers, body) =
        get_raw(&app, &format!("/api/sessions/{id}/shared/{file_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CONTENT_TYPE).expect("content type"),
        "text/plain; charset=utf-8"
    );
    assert_eq!(body, b"hello from the session\n");

    let outside_relative = format!(
        "../{}/outside.txt",
        outside
            .path
            .file_name()
            .expect("outside name")
            .to_str()
            .expect("outside name text")
    );
    for path in [
        outside_relative,
        outside
            .path
            .join("outside.txt")
            .to_string_lossy()
            .into_owned(),
    ] {
        let (status, refused) = call(
            &app,
            "POST",
            &mcp,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "share_file",
                    "arguments": { "path": path }
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{refused}");
        assert_eq!(refused["result"]["isError"], true);
        assert_eq!(
            refused["result"]["content"][0]["text"],
            "The path must be a file inside the session folder."
        );
    }

    let (_, refused) = call(
        &app,
        "POST",
        &mcp,
        Some(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "share_file",
                "arguments": { "path": ".env" }
            }
        })),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["content"][0]["text"],
        "The file looks like a key or a secret file. It was not shared."
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path.join("outside.txt"), repo.path.join("link.txt"))
            .expect("create symlink");
        let (_, refused) = call(
            &app,
            "POST",
            &mcp,
            Some(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "share_file",
                    "arguments": { "path": "link.txt" }
                }
            })),
        )
        .await;
        assert_eq!(refused["result"]["isError"], true);
        assert_eq!(
            refused["result"]["content"][0]["text"],
            "The path must be a file inside the session folder."
        );
    }

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mcp_share_file_refuses_a_stored_key() {
    let data_dir = TempDirectory::new("mcp-share-key-data");
    let repo = TempDirectory::new("mcp-share-key-repo");
    init_git_repo(&repo.path);
    let secrets = Arc::new(MemorySecretStore::default());
    secrets
        .set_key("deepseek", "sk-test-hidden-value")
        .expect("store key");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets);
    fs::write(repo.path.join("notes.txt"), "key: sk-test-hidden-value\n").expect("write notes");
    let app = router(state);
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id");

    let (status, refused) = call(
        &app,
        "POST",
        &format!("/internal/mcp/{id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "share_file",
                "arguments": { "path": "notes.txt" }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refused}");
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["content"][0]["text"],
        "The file looks like a key or a secret file. It was not shared."
    );

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn closing_a_session_withdraws_its_questions() {
    let data_dir = TempDirectory::new("mcp-close-data");
    let repo = TempDirectory::new("mcp-close-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "MCP close",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let (status, asked) = call(
        &app,
        "POST",
        &format!("/internal/mcp/{id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "ask_me", "arguments": { "text": "Still there?" } }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    let (_, questions) = call(&app, "GET", "/api/questions", None).await;
    assert_eq!(questions.as_array().expect("questions").len(), 1);

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
    let (_, questions) = call(&app, "GET", "/api/questions", None).await;
    assert_eq!(questions.as_array().expect("questions").len(), 0);
}

#[tokio::test]
async fn session_environment_overrides_reach_the_process() {
    let data_dir = TempDirectory::new("env-data");
    let repo = TempDirectory::new("env-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf 'value:%s\\n' \"$E2E_VALUE\"; sleep 30".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Env overrides",
            "folder": repo.path.to_string_lossy(),
            "env": [{ "name": "E2E_VALUE", "value": "hello" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    let text = read_sse_until(response.into_body(), |text| text.contains("value:hello")).await;
    assert!(sse_data_text(&text).contains("value:hello"));

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn configs_can_be_created_listed_updated_and_deleted() {
    let app = router(AppState::ephemeral());
    let (status, created) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Fast Claude",
            "providerId": "max",
            "client": "Claude Code",
            "model": "claude-sonnet-4-5",
            "effort": "high",
            "args": ["--verbose"],
            "env": [{ "name": "E2E_VALUE", "value": "config" }],
            "config": { "permissions.defaultMode": "acceptEdits" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["name"], "Fast Claude");
    assert_eq!(created["providerId"], "max");
    assert_eq!(created["client"], "Claude Code");
    assert_eq!(created["model"], "claude-sonnet-4-5");
    assert_eq!(created["effort"], "high");
    assert_eq!(created["args"], json!(["--verbose"]));
    assert_eq!(created["env"][0]["name"], "E2E_VALUE");
    assert_eq!(created["config"]["permissions.defaultMode"], "acceptEdits");
    let id = created["id"].as_str().expect("config id").to_owned();

    let (status, filtered) = call(&app, "GET", "/api/configs?q=fast", None).await;
    assert_eq!(status, StatusCode::OK);
    let filtered = filtered.as_array().expect("configs");
    assert!(filtered.iter().any(|config| config["id"] == id.as_str()));

    let (_, all) = call(&app, "GET", "/api/configs", None).await;
    let codex = all
        .as_array()
        .expect("configs")
        .iter()
        .find(|config| config["id"] == "codex-codex")
        .expect("the seeded Codex config");
    assert_eq!(codex["name"], "ChatGPT Pro (Codex)");
    assert_eq!(codex["providerId"], "codex");

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/configs/{id}"),
        Some(json!({
            "name": "Faster Claude",
            "model": "claude-opus-4-1",
            "effort": "max",
            "env": [],
            "config": { "model": "from-config" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Faster Claude");
    assert_eq!(updated["model"], "claude-opus-4-1");
    assert_eq!(updated["effort"], "max");
    assert!(updated["env"].as_array().expect("env").is_empty());
    assert_eq!(updated["config"]["model"], "from-config");

    let (status, _) = call(&app, "DELETE", &format!("/api/configs/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "DELETE", &format!("/api/configs/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_providers_clients_and_values_are_validated() {
    let app = router(AppState::ephemeral());

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Orphan",
            "providerId": "missing",
            "client": "Claude Code"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Wrong client",
            "providerId": "google",
            "client": "Claude Code"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, created) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Valid",
            "providerId": "max",
            "client": "Claude Code"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().expect("config id");

    let (status, unnamed) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({ "providerId": "codex", "client": "Codex" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unnamed}");
    assert_eq!(unnamed["name"], "ChatGPT Pro (Codex)");

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/configs/{id}"),
        Some(json!({ "providerId": "missing" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Bad model",
            "providerId": "max",
            "client": "Claude Code",
            "model": "not a model"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Bad effort",
            "providerId": "max",
            "client": "Claude Code",
            "effort": "turbo"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Bad key",
            "providerId": "max",
            "client": "Claude Code",
            "config": { "bad key": "value" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Key in env",
            "providerId": "max",
            "client": "Claude Code",
            "env": [{ "name": "ANTHROPIC_API_KEY", "value": "secret" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Key in config",
            "providerId": "codex",
            "client": "Codex",
            "config": { "providers.openai.apiKey": "secret" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn configs_persist_across_restarts() {
    let data_dir = TempDirectory::new("config-persist");
    {
        let app = router(AppState::with_data_dir(data_dir.path.clone()));
        let (status, created) = call(
            &app,
            "POST",
            "/api/configs",
            Some(json!({
                "name": "Saved",
                "providerId": "codex",
                "client": "Codex",
                "model": "gpt-5.2",
                "effort": "high",
                "config": { "features.web": "true" }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
    }

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (_, configs) = call(&app, "GET", "/api/configs", None).await;
    let saved = configs
        .as_array()
        .expect("configs")
        .iter()
        .find(|config| config["name"] == "Saved")
        .expect("the stored config was reloaded");
    assert_eq!(saved["providerId"], "codex");
    assert_eq!(saved["client"], "Codex");
    assert_eq!(saved["model"], "gpt-5.2");
    assert_eq!(saved["effort"], "high");
    assert_eq!(saved["config"]["features.web"], "true");
}

#[tokio::test]
async fn session_from_a_preset_gets_its_model_and_environment() {
    let data_dir = TempDirectory::new("preset-session-data");
    let repo = TempDirectory::new("preset-session-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf 'value:%s\\n' \"$E2E_VALUE\"; sleep 30".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, preset) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Preset launch",
            "providerId": "max",
            "client": "Claude Code",
            "model": "claude-sonnet-4-5",
            "env": [{ "name": "E2E_VALUE", "value": "from-preset" }],
            "config": { "permissions.defaultMode": "acceptEdits" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preset}");
    let preset_id = preset["id"].as_str().expect("config id");

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": preset_id,
            "name": "From preset",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["configId"], preset_id);
    assert_eq!(session["providerId"], "max");
    assert_eq!(session["model"], "claude-sonnet-4-5");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    let text = read_sse_until(response.into_body(), |text| {
        text.contains("value:from-preset")
    })
    .await;
    assert!(sse_data_text(&text).contains("value:from-preset"));

    let (_, settings) = claude_settings(&data_dir.path, &id);
    assert_eq!(settings["model"], "claude-sonnet-4-5");
    assert_eq!(settings["permissions"]["defaultMode"], "acceptEdits");
    assert!(settings["hooks"]["PreToolUse"].is_array());

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn launch_environment_overrides_win_over_a_preset() {
    let data_dir = TempDirectory::new("preset-env-data");
    let repo = TempDirectory::new("preset-env-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf 'value:%s\\n' \"$E2E_VALUE\"; sleep 30".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, preset) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Preset environment",
            "providerId": "max",
            "client": "Claude Code",
            "env": [{ "name": "E2E_VALUE", "value": "from-preset" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preset}");
    let preset_id = preset["id"].as_str().expect("config id");

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": preset_id,
            "name": "Launch wins",
            "folder": repo.path.to_string_lossy(),
            "env": [{ "name": "E2E_VALUE", "value": "from-launch" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(
        session["envOverrides"],
        json!([{ "name": "E2E_VALUE", "value": "from-launch" }])
    );

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    let text = read_sse_until(response.into_body(), |text| {
        text.contains("value:from-launch")
    })
    .await;
    let output = sse_data_text(&text);
    assert!(output.contains("value:from-launch"));
    assert!(!output.contains("value:from-preset"));

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn terminal_sessions_reject_a_preset() {
    let folder = TempDirectory::new("preset-terminal");
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    ensure_test_project(&app, &folder.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "configId": "max-claude-code",
            "folder": folder.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn invalid_environment_names_are_rejected() {
    let data_dir = TempDirectory::new("env-bad-data");
    let repo = TempDirectory::new("env-bad-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    for (name, status) in [
        ("1BAD", StatusCode::BAD_REQUEST),
        ("BAD-NAME", StatusCode::BAD_REQUEST),
    ] {
        let (result, body) = call(
            &app,
            "POST",
            "/api/sessions",
            Some(json!({
                "projectId": "web",
                "kind": "terminal",
                "folder": repo.path.to_string_lossy(),
                "env": [{ "name": name, "value": "x" }]
            })),
        )
        .await;
        assert_eq!(result, status, "{body}");
    }
}

#[tokio::test]
async fn cors_preflight_and_allowed_origin_headers() {
    let app = router(AppState::ephemeral());
    let origins = [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ];

    for origin in origins {
        let request = request_builder("GET", "/api/bootstrap")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .expect("build request");
        let response = app.clone().oneshot(request).await.expect("call route");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some(origin)
        );
        assert_eq!(
            response
                .headers()
                .get(header::VARY)
                .and_then(|value| value.to_str().ok()),
            Some("Origin")
        );
    }

    let request = request_builder("OPTIONS", "/api/sessions")
        .header(header::ORIGIN, "tauri://localhost")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "content-type, x-el-esdee-filename",
        )
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("call route");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("tauri://localhost")
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .and_then(|value| value.to_str().ok()),
        Some("GET, POST, PATCH, DELETE, OPTIONS")
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|value| value.to_str().ok()),
        Some("content-type, x-el-esdee-filename")
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_MAX_AGE)
            .and_then(|value| value.to_str().ok()),
        Some("600")
    );

    let request = request_builder("OPTIONS", "/api/sessions")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("call route");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[tokio::test]
async fn project_creation_returns_the_new_project() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("project-success");

    let (status, project) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "New App", "folder": folder.path.to_string_lossy() })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["id"], "new-app");
    assert_eq!(project["name"], "New App");
    assert_eq!(
        project["folder"],
        folder
            .path
            .canonicalize()
            .expect("canonical folder")
            .to_string_lossy()
            .as_ref()
    );

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let projects = bootstrap["projects"].as_array().expect("projects");
    assert_eq!(projects.len(), 1);
    assert!(projects.iter().any(|stored| stored["id"] == "new-app"));
}

#[tokio::test]
async fn project_creation_rejects_an_empty_name() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("project-empty-name");

    let (status, body) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "   ", "folder": folder.path.to_string_lossy() })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn project_creation_rejects_a_missing_folder() {
    let app = router(AppState::ephemeral());
    let parent = TempDirectory::new("project-missing-folder");
    let missing = parent.path.join("not-here");

    let (status, body) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "Missing", "folder": missing.to_string_lossy() })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn project_creation_rejects_a_duplicate_folder() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("project-duplicate-folder");
    let body = json!({ "name": "First", "folder": folder.path.to_string_lossy() });

    let (status, project) = call(&app, "POST", "/api/projects", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{project}");

    let mut duplicate = body;
    duplicate["name"] = Value::String("Second".to_owned());
    let (status, body) = call(&app, "POST", "/api/projects", Some(duplicate)).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn project_creation_appends_a_suffix_for_a_duplicate_name() {
    let app = router(AppState::ephemeral());
    let first_folder = TempDirectory::new("project-duplicate-name-a");
    let second_folder = TempDirectory::new("project-duplicate-name-b");

    let (status, first) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "New App", "folder": first_folder.path.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");

    let (status, second) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({ "name": "New App", "folder": second_folder.path.to_string_lossy() })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["id"], "new-app-2");
}

#[tokio::test]
async fn project_list_filters_by_name_and_folder() {
    let app = router(AppState::ephemeral());
    let root = TempDirectory::new("project-list");
    let alpha_folder = root.path.join("alpha");
    let beta_folder = root.path.join("beta");
    fs::create_dir_all(&alpha_folder).expect("create alpha folder");
    fs::create_dir_all(&beta_folder).expect("create beta folder");

    let (status, first) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "Zeta App",
            "folder": alpha_folder.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "Alpha Tool",
            "folder": beta_folder.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");

    let (status, all) = call(&app, "GET", "/api/projects", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let names = all
        .as_array()
        .expect("projects")
        .iter()
        .map(|project| project["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Alpha Tool", "Zeta App"]);

    let (status, filtered) = call(&app, "GET", "/api/projects?q=APP", None).await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    assert_eq!(filtered.as_array().expect("projects").len(), 1);
    assert_eq!(filtered[0]["id"], "zeta-app");

    let (status, by_folder) = call(&app, "GET", "/api/projects?q=beta", None).await;
    assert_eq!(status, StatusCode::OK, "{by_folder}");
    assert_eq!(by_folder.as_array().expect("projects").len(), 1);
    assert_eq!(by_folder[0]["id"], "alpha-tool");

    let (status, empty) = call(&app, "GET", "/api/projects?q=", None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty.as_array().expect("projects").len(), 2);

    let (status, miss) = call(&app, "GET", "/api/projects?q=missing", None).await;
    assert_eq!(status, StatusCode::OK, "{miss}");
    assert!(miss.as_array().expect("projects").is_empty());
}

#[tokio::test]
async fn project_defaults_can_be_set_and_cleared() {
    let repo = TempDirectory::new("project-defaults");
    init_git_repo(&repo.path);
    assert!(
        run_git(&repo.path, &["checkout", "-b", "feature"])
            .status
            .success(),
        "create feature branch"
    );
    assert!(run_git(&repo.path, &["checkout", "main"]).status.success());

    let app = router(AppState::ephemeral());
    ensure_test_project(&app, &repo.path).await;

    let (status, updated) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({
            "defaultConfigId": "max-claude-code",
            "defaultSourceBranch": "feature"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["defaultConfigId"], "max-claude-code");
    assert_eq!(updated["defaultSourceBranch"], "feature");

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{bootstrap}");
    assert_eq!(
        bootstrap["projects"][0]["defaultConfigId"],
        "max-claude-code"
    );
    assert_eq!(bootstrap["projects"][0]["defaultSourceBranch"], "feature");

    let (status, cleared) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultConfigId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["defaultConfigId"], Value::Null);
    assert_eq!(
        cleared["defaultSourceBranch"], "feature",
        "an absent field keeps its value"
    );

    let (status, cleared) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultSourceBranch": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["defaultConfigId"], Value::Null);
    assert_eq!(
        cleared["defaultSourceBranch"],
        Value::Null,
        "an empty branch clears the default"
    );
}

#[tokio::test]
async fn project_defaults_reject_an_unknown_account() {
    let repo = TempDirectory::new("project-defaults-account");
    let app = router(AppState::ephemeral());
    ensure_test_project(&app, &repo.path).await;

    let (status, body) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultConfigId": "missing" })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn project_defaults_reject_an_unsafe_or_missing_branch() {
    let repo = TempDirectory::new("project-defaults-branch");
    init_git_repo(&repo.path);
    let app = router(AppState::ephemeral());
    ensure_test_project(&app, &repo.path).await;

    let (status, body) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultSourceBranch": "bad branch" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultSourceBranch": "missing-branch" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    assert!(run_git(&repo.path, &["tag", "release"]).status.success());
    for revision in ["HEAD", "release"] {
        let (status, body) = call(
            &app,
            "PATCH",
            "/api/projects/web",
            Some(json!({ "defaultSourceBranch": revision })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{revision} must not pass as a branch: {body}"
        );
    }

    let (status, body) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultSourceBranch": "main@{1}" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let plain = TempDirectory::new("project-defaults-plain");
    let plain_app = router(AppState::ephemeral());
    ensure_test_project(&plain_app, &plain.path).await;
    let (status, body) = call(
        &plain_app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultSourceBranch": "main" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn project_defaults_reject_an_unknown_project() {
    let app = router(AppState::ephemeral());

    let (status, body) = call(
        &app,
        "PATCH",
        "/api/projects/missing",
        Some(json!({ "defaultConfigId": null })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn project_defaults_are_saved_and_reloaded() {
    let data_dir = TempDirectory::new("project-defaults-save");
    let repo = TempDirectory::new("project-defaults-save-repo");
    init_git_repo(&repo.path);

    {
        let app = router(AppState::with_data_dir(data_dir.path.clone()));
        ensure_test_project(&app, &repo.path).await;
        let (status, body) = call(
            &app,
            "PATCH",
            "/api/projects/web",
            Some(json!({
                "defaultConfigId": "max-claude-code",
                "defaultSourceBranch": "main"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, body) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["projects"][0]["defaultConfigId"], "max-claude-code");
    assert_eq!(body["projects"][0]["defaultSourceBranch"], "main");
}

#[tokio::test]
async fn old_project_state_loads_without_defaults() {
    let data_dir = TempDirectory::new("project-old-state");
    let folder = TempDirectory::new("project-old-state-folder");
    let stored = json!({
        "version": 1,
        "projects": [{
            "id": "web",
            "name": "web",
            "folder": folder.path.to_string_lossy()
        }],
        "sessions": []
    });
    fs::write(data_dir.path.join("state.json"), stored.to_string()).expect("write old state");

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, body) = call(&app, "GET", "/api/bootstrap", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["projects"][0]["defaultConfigId"], Value::Null);
    assert_eq!(body["projects"][0]["defaultSourceBranch"], Value::Null);
}

#[tokio::test]
async fn method_not_allowed_returns_a_json_error() {
    let app = router(AppState::ephemeral());

    for method in ["GET", "PUT"] {
        let request = request_builder(method, "/api/sessions")
            .body(Body::empty())
            .expect("build request");
        let (status, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}: {body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
    }
}

#[tokio::test]
async fn create_session_uses_the_requested_terminal_size() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf 'SIZE:'; stty size; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("create-size");
    ensure_test_project(&app, &folder.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": folder.path.to_string_lossy(),
            "cols": 100,
            "rows": 40
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let output = read_output_until(response.into_body(), |text| text.contains("40 100")).await;
    assert!(output.contains("40 100"), "pty size output: {output}");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": folder.path.to_string_lossy(),
            "cols": 10,
            "rows": 32
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn resize_applies_to_the_live_pty() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 1; stty size; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("resize-live");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/resize"),
        Some(json!({ "cols": 100, "rows": 40 })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let output = read_output_until(response.into_body(), |text| text.contains("40 100")).await;
    assert!(output.contains("40 100"), "pty size output: {output}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn resize_rejects_invalid_sizes() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("resize-invalid");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    for (cols, rows) in [(10, 32), (501, 32), (100, 4), (100, 301)] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/resize"),
            Some(json!({ "cols": cols, "rows": rows })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "cols={cols} rows={rows}: {body}"
        );
    }

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn resize_unknown_session_returns_not_found() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions/unknown/resize",
        Some(json!({ "cols": 100, "rows": 40 })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn resize_closed_session_returns_conflict() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("resize-closed");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/resize"),
        Some(json!({ "cols": 100, "rows": 40 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

async fn wait_for_done(app: &Router, id: &str) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let (status, body) = call(app, "GET", "/api/bootstrap", None).await;
        assert_eq!(status, StatusCode::OK);
        let done = body["sessions"]
            .as_array()
            .expect("sessions array")
            .iter()
            .find(|session| session["id"] == id)
            .is_some_and(|session| session["workState"] == "done");
        if done {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session {id} did not finish"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn rename_updates_the_display_name_only() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("rename");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let short = session["short"].clone();

    let (status, renamed) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "name": "  Renamed session  " })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Renamed session");
    assert_eq!(renamed["short"], short);
    assert_eq!(renamed["branch"], Value::Null);
    assert_eq!(renamed["status"], "active");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn rename_rejects_an_empty_name() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("rename-empty");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id");

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "name": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn rename_unknown_session_returns_not_found() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(
        &app,
        "PATCH",
        "/api/sessions/unknown",
        Some(json!({ "name": "Renamed" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn moving_a_running_session_keeps_its_folder_branch_and_process() {
    let data_dir = TempDirectory::new("move-data");
    let repo = TempDirectory::new("move-repo");
    init_git_repo(&repo.path);
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state.clone());
    let mut events = state.subscribe_events();
    ensure_test_project(&app, &repo.path).await;
    let other_folder = TempDirectory::new("move-other");
    let other = create_test_project(&app, "mobile", &other_folder.path).await;
    let other_id = other["id"].as_str().expect("project id").to_owned();

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Move me",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].clone();
    let folder = session["folder"].clone();
    let short = session["short"].clone();

    while events.try_recv().is_ok() {}

    let (status, moved) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "projectId": other_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["projectId"], other_id);
    assert_eq!(moved["folder"], folder, "the worktree stays");
    assert_eq!(moved["branch"], branch, "the branch stays");
    assert_eq!(moved["short"], short);
    assert_eq!(moved["status"], "active");

    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(event) = events.recv().await
                && event.session.id == id
                && event.session.project_id == other_id
            {
                return event;
            }
        }
    })
    .await
    .expect("the move event did not arrive");
    assert_eq!(event.event_type, "session.updated");

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    let state_json: Value = serde_json::from_str(&state_text).expect("state json");
    let stored = state_json["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|session| session["session"]["id"] == id)
        })
        .expect("stored session");
    assert_eq!(stored["session"]["projectId"], other_id);

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "still running" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn moving_a_session_to_an_unknown_project_is_refused() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("move-unknown");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, body) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "projectId": "missing" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let stored = bootstrap["sessions"]
        .as_array()
        .and_then(|sessions| sessions.iter().find(|item| item["id"] == id))
        .expect("session");
    assert_eq!(stored["projectId"], "web");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn moving_a_closed_session_keeps_its_log() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf keep-me; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("move-closed");
    let other_folder = TempDirectory::new("move-closed-other");
    ensure_test_project(&app, &folder.path).await;
    let other = create_test_project(&app, "docs", &other_folder.path).await;
    let other_id = other["id"].as_str().expect("project id").to_owned();
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let short = session["short"].clone();
    wait_for_session(&app, &id, |session| {
        session["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("keep-me"))
    })
    .await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let preview = closed["preview"].clone();

    let (status, moved) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "projectId": other_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["status"], "closed");
    assert_eq!(moved["projectId"], other_id);
    assert_eq!(moved["short"], short);
    assert_eq!(moved["preview"], preview, "the close note stays");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("keep-me")).await;
    assert!(text.contains("keep-me"), "the log stays: {text}");
}

#[tokio::test]
async fn reopen_restarts_a_closed_shell() {
    let app = router(AppState::ephemeral());
    let folder = TempDirectory::new("reopen-shell");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    tokio::time::sleep(Duration::from_millis(250)).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/input"),
        Some(json!({ "data": "exit\r" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    wait_for_done(&app, &id).await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, reopened) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["workState"], "working");
    assert!(reopened["waitingFor"].is_null());
    assert_eq!(reopened["name"], closed["name"]);
    assert_eq!(reopened["short"], closed["short"]);
    assert_eq!(reopened["branch"], closed["branch"]);

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn reopen_keeps_output_sequence_numbers() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf first; exit 0".to_owned(),
    ]));
    let folder = TempDirectory::new("reopen-sequence");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    read_sse_until(response.into_body(), |text| text.contains("first")).await;
    wait_for_done(&app, &id).await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, reopened) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=1"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("\"seq\":2")).await;
    let events = sse_values(&text);
    assert!(
        events
            .iter()
            .any(|event| event["seq"] == 2 && event["data"] == "first")
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn reopen_active_session_returns_conflict() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("reopen-active");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn reopen_with_a_missing_folder_returns_unprocessable() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("reopen-missing-folder");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    fs::remove_dir_all(folder.path.canonicalize().expect("canonical folder"))
        .expect("remove folder");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn delete_closed_session_removes_it() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("delete-closed");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !bootstrap["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["id"] == id)
    );
}

#[tokio::test]
async fn delete_active_session_returns_conflict() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("delete-active");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn delete_unknown_session_returns_not_found() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(&app, "DELETE", "/api/sessions/unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn persistence_keeps_projects_sessions_and_logs() {
    let data_dir = TempDirectory::new("persist-data");
    let project_folder = TempDirectory::new("persist-project");
    let session_folder = TempDirectory::new("persist-session");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf hello; sleep 30".to_owned(),
    ];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));

    let (status, project) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "Persisted Project",
            "folder": project_folder.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "persisted-project",
            "kind": "terminal",
            "folder": session_folder.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    read_sse_until(response.into_body(), |text| text.contains("hello")).await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    drop(app);

    let state_path = data_dir.path.join("state.json");
    let state_text = fs::read_to_string(&state_path).expect("state file");
    let state: Value = serde_json::from_str(&state_text).expect("state json");
    assert_eq!(state["version"], 2);
    assert_eq!(state["providers"].as_array().expect("providers").len(), 4);
    assert_eq!(state["configs"].as_array().expect("configs").len(), 4);
    assert!(
        state["projects"]
            .as_array()
            .expect("projects")
            .iter()
            .any(|project| project["id"] == "persisted-project")
    );
    assert_eq!(state["sessions"].as_array().expect("sessions").len(), 1);
    assert_eq!(state["sessions"][0]["session"]["id"], id);
    assert_eq!(state["sessions"][0]["session"]["status"], "closed");
    assert!(
        !state["sessions"][0]["output"]
            .as_array()
            .expect("output")
            .is_empty()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory_mode = fs::metadata(&data_dir.path)
            .expect("data dir")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&state_path)
            .expect("state file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        bootstrap["providers"].as_array().expect("providers").len(),
        4
    );
    assert_eq!(bootstrap["configs"].as_array().expect("configs").len(), 4);
    assert!(
        bootstrap["projects"]
            .as_array()
            .expect("projects")
            .iter()
            .any(|project| project["id"] == "persisted-project")
    );
    let restored = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .cloned()
        .expect("restored session");
    assert_eq!(restored["status"], "closed");
    assert_eq!(restored["projectId"], "persisted-project");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = restarted
        .clone()
        .oneshot(request)
        .await
        .expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("hello")).await;
    assert!(sse_values(&text).iter().any(|event| event["seq"] == 1));
}

#[tokio::test]
async fn active_session_resumes_on_startup() {
    let data_dir = TempDirectory::new("persist-active-data");
    let folder = TempDirectory::new("persist-active-folder");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf first; sleep 30".to_owned(),
    ];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["status"], "active");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    read_sse_until(response.into_body(), |text| text.contains("first")).await;
    // A state save keeps the output log and its next sequence number.
    let (status, renamed) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id}"),
        Some(json!({ "name": "Recovered" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    drop(app);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let restored =
        wait_for_session(&restarted, &id, |session| session["workState"] == "working").await;

    assert_eq!(restored["status"], "active");
    assert!(restored["waitingFor"].is_null());
    assert_ne!(
        restored["preview"],
        "Paused. The app restarted. Resume to continue."
    );

    // The restarted process continues the same output log from the next sequence.
    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=1"))
        .body(Body::empty())
        .expect("build output request");
    let response = restarted
        .clone()
        .oneshot(request)
        .await
        .expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("\"seq\":2")).await;
    assert!(
        sse_values(&text)
            .iter()
            .any(|event| event["seq"] == 2 && event["data"] == "first")
    );

    let (status, _) = close_session(&restarted, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn restart_keeps_a_codex_preset_model_and_config() {
    let data_dir = TempDirectory::new("recover-codex-config-data");
    let repo = TempDirectory::new("recover-codex-config-repo");
    init_git_repo(&repo.path);
    let args_file = data_dir.path.join("args.txt");
    let script = format!(
        "printf '%s\\n' \"$@\" > {}; sleep 30",
        args_file.to_string_lossy()
    );
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        script,
        "stub".to_owned(),
    ];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, preset) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "name": "Recovered Codex",
            "providerId": "codex",
            "client": "Codex",
            "model": "gpt-5.2",
            "effort": "high"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preset}");
    let preset_id = preset["id"].as_str().expect("config id");

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": preset_id,
            "name": "Recovered Codex",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    wait_for_args_file(&args_file).await;
    drop(app);
    let _ = fs::remove_file(&args_file);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    wait_for_args_file(&args_file).await;
    let arguments = fs::read_to_string(&args_file).expect("read arguments");
    assert!(
        arguments.contains("model=\"gpt-5.2\""),
        "the restarted Codex session lost its model: {arguments}"
    );
    assert!(
        arguments.contains("model_reasoning_effort=\"high\""),
        "the restarted Codex session lost its config: {arguments}"
    );

    let _ = close_session(&restarted, &id).await;
}

async fn wait_for_args_file(path: &Path) {
    for _ in 0..200 {
        if path.is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("{} was not written", path.display());
}

#[tokio::test]
async fn active_agent_session_keeps_its_conversation_id_across_restart() {
    let data_dir = TempDirectory::new("persist-agent-data");
    let repo = TempDirectory::new("persist-agent-repo");
    init_git_repo(&repo.path);
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Recovered agent",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let conversation = session["agentSessionId"]
        .as_str()
        .expect("conversation id")
        .to_owned();
    assert!(!conversation.is_empty());
    drop(app);

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    let state: Value = serde_json::from_str(&state_text).expect("state json");
    let stored = state["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|stored| stored["session"]["id"] == id)
        .expect("stored session");
    assert_eq!(stored["session"]["agentSessionId"], conversation);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let restored =
        wait_for_session(&restarted, &id, |session| session["workState"] == "working").await;

    assert_eq!(restored["status"], "active");
    assert_eq!(restored["agentSessionId"], conversation);
    assert_eq!(restored["preview"], "Resumed.");
    assert!(
        data_dir
            .path
            .join("mcp")
            .join(format!("{id}.json"))
            .exists()
    );

    let (status, _) = close_session(&restarted, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn active_session_with_a_missing_folder_restores_as_paused() {
    let data_dir = TempDirectory::new("persist-missing-folder-data");
    let root = TempDirectory::new("persist-missing-folder-root");
    let folder = root.path.join("session");
    fs::create_dir_all(&folder).expect("create session folder");
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &folder).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    drop(app);

    fs::remove_dir_all(&folder).expect("remove session folder");

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{bootstrap}");
    let restored = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("restored session");

    assert_eq!(restored["status"], "active");
    assert_eq!(restored["workState"], "paused");
    assert!(
        restored["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("folder is missing")),
        "{}",
        restored["preview"]
    );
}

#[tokio::test]
async fn active_session_with_a_missing_client_restores_as_paused() {
    let data_dir = TempDirectory::new("persist-missing-client-data");
    let repo = TempDirectory::new("persist-missing-client-repo");
    init_git_repo(&repo.path);
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Missing client",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    drop(app);

    let restarted = router(AppState::with_test_agent_bin_and_data_dir(
        data_dir.path.join("not-an-agent"),
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{bootstrap}");
    let restored = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("restored session");

    assert_eq!(restored["status"], "active");
    assert_eq!(restored["workState"], "paused");
    assert!(
        restored["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("is not installed")),
        "{}",
        restored["preview"]
    );
}

#[tokio::test]
async fn reopen_after_restart_continues_the_output_sequence() {
    let data_dir = TempDirectory::new("persist-sequence-data");
    let folder = TempDirectory::new("persist-sequence-folder");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf first; exit 0".to_owned(),
    ];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    read_sse_until(response.into_body(), |text| text.contains("first")).await;
    wait_for_done(&app, &id).await;
    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    drop(app);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, reopened) = call(
        &restarted,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=1"))
        .body(Body::empty())
        .expect("build output request");
    let response = restarted
        .clone()
        .oneshot(request)
        .await
        .expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let text = read_sse_until(response.into_body(), |text| text.contains("\"seq\":2")).await;
    assert!(
        sse_values(&text)
            .iter()
            .any(|event| event["seq"] == 2 && event["data"] == "first")
    );

    let (status, closed) = close_session(&restarted, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn rename_and_delete_persist_across_restarts() {
    let data_dir = TempDirectory::new("persist-menu-data");
    let folder_a = TempDirectory::new("persist-menu-a");
    let folder_b = TempDirectory::new("persist-menu-b");
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session_a = create_terminal(&app, &folder_a.path).await;
    let session_b = create_terminal(&app, &folder_b.path).await;
    let id_a = session_a["id"].as_str().expect("session id").to_owned();
    let id_b = session_b["id"].as_str().expect("session id").to_owned();

    let (status, renamed) = call(
        &app,
        "PATCH",
        &format!("/api/sessions/{id_a}"),
        Some(json!({ "name": "Renamed A" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let (status, closed) = close_session(&app, &id_a).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let (status, closed) = close_session(&app, &id_b).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id_b}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    drop(app);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = bootstrap["sessions"].as_array().expect("sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], id_a);
    assert_eq!(sessions[0]["name"], "Renamed A");
    assert!(!sessions.iter().any(|session| session["id"] == id_b));
}

#[tokio::test]
async fn graceful_shutdown_keeps_logs_and_resumes_sessions() {
    let data_dir = TempDirectory::new("persist-shutdown-data");
    let folder_a = TempDirectory::new("persist-shutdown-a");
    let folder_b = TempDirectory::new("persist-shutdown-b");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf alpha; sleep 30".to_owned(),
    ];
    let state = AppState::with_test_command_and_data_dir(command.clone(), data_dir.path.clone());
    let app = router(state.clone());

    let session_a = create_terminal(&app, &folder_a.path).await;
    let id_a = session_a["id"].as_str().expect("session id").to_owned();
    let request = request_builder("GET", &format!("/api/sessions/{id_a}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    read_sse_until(response.into_body(), |text| text.contains("alpha")).await;
    let (status, closed) = close_session(&app, &id_a).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let session_b = create_terminal(&app, &folder_b.path).await;
    let id_b = session_b["id"].as_str().expect("session id").to_owned();
    let request = request_builder("GET", &format!("/api/sessions/{id_b}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    read_sse_until(response.into_body(), |text| text.contains("alpha")).await;

    assert!(state.shutdown_all());
    drop(app);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = bootstrap["sessions"].as_array().expect("sessions");
    let restored_a = sessions
        .iter()
        .find(|session| session["id"] == id_a)
        .expect("session a");
    let restored_b = sessions
        .iter()
        .find(|session| session["id"] == id_b)
        .expect("session b");

    assert_eq!(restored_a["status"], "closed");
    assert_eq!(restored_a["preview"], "alpha");
    assert_eq!(restored_b["status"], "active");
    assert_eq!(restored_b["workState"], "working");
    assert_ne!(
        restored_b["preview"],
        "Paused. The app restarted. Resume to continue."
    );

    for id in [&id_a, &id_b] {
        let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
            .body(Body::empty())
            .expect("build output request");
        let response = restarted
            .clone()
            .oneshot(request)
            .await
            .expect("open output");
        assert_eq!(response.status(), StatusCode::OK);
        let text = read_sse_until(response.into_body(), |text| text.contains("alpha")).await;
        assert!(!sse_values(&text).is_empty());
    }
}

#[tokio::test]
async fn corrupt_state_falls_back_to_sample_accounts() {
    let data_dir = TempDirectory::new("persist-corrupt-data");
    let state_path = data_dir.path.join("state.json");
    fs::write(&state_path, b"{ this is not json").expect("write corrupt state");

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        bootstrap["projects"]
            .as_array()
            .expect("projects")
            .is_empty()
    );
    assert_eq!(
        bootstrap["providers"].as_array().expect("providers").len(),
        4
    );
    assert_eq!(bootstrap["configs"].as_array().expect("configs").len(), 4);
    assert_eq!(bootstrap["sessions"].as_array().expect("sessions").len(), 0);
    assert!(data_dir.path.join("state.bad.json").exists());
    assert!(!state_path.exists());
}

#[tokio::test]
async fn dirty_agent_close_commits_and_removes_the_worktree() {
    let data_dir = TempDirectory::new("worktree-dirty-data");
    let repo = TempDirectory::new("worktree-dirty-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Dirty Work",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write worktree file");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["preview"], "Closed. The worktree was removed.");
    assert!(!worktree.exists());
    assert!(git_ref_exists(&repo.path, &format!("refs/heads/{branch}")));
    assert_eq!(
        git_stdout(&repo.path, &["log", "-1", "--format=%s", &branch]),
        "elesdee: close Dirty Work"
    );
    assert_eq!(
        git_stdout(&repo.path, &["show", &format!("{branch}:new.txt")]),
        "hello"
    );
}

#[tokio::test]
async fn close_puts_the_handoff_note_in_the_last_commit_message() {
    let data_dir = TempDirectory::new("close-note-data");
    let repo = TempDirectory::new("close-note-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Close Note",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));

    let note = "Goal: ship the close note.\nDone: the parser.\nNext: run the checks.";
    write_handoff_note(&app, &id, note).await;
    fs::write(worktree.join("new.txt"), "hello\n").expect("write worktree file");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["preview"], "Closed. The worktree was removed.");
    assert_eq!(closed["handoffNote"], note);
    assert!(!worktree.exists());

    let message = git_stdout(&repo.path, &["log", "-1", "--format=%B", &branch]);
    assert!(
        message.starts_with("elesdee: close Close Note"),
        "{message}"
    );
    assert!(message.contains(note), "{message}");
    assert_eq!(
        git_stdout(&repo.path, &["show", "--name-only", "--format=", &branch]),
        "new.txt",
        "the note became a file in the repository"
    );
}

#[tokio::test]
async fn close_on_a_clean_tree_keeps_the_note_on_the_last_commit() {
    let data_dir = TempDirectory::new("close-clean-note-data");
    let repo = TempDirectory::new("close-clean-note-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Clean Note",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();

    let note = "Goal: carry the note on a clean tree.";
    write_handoff_note(&app, &id, note).await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["handoffNote"], note);

    let message = git_stdout(&repo.path, &["log", "-1", "--format=%B", &branch]);
    assert!(
        message.starts_with("elesdee: close Clean Note"),
        "{message}"
    );
    assert!(message.contains(note), "{message}");
    assert_eq!(
        git_stdout(&repo.path, &["show", "--name-only", "--format=", &branch]),
        "",
        "the note commit changed a file"
    );
}

#[tokio::test]
async fn push_failure_keeps_the_worktree() {
    let data_dir = TempDirectory::new("worktree-push-fail-data");
    let repo = TempDirectory::new("worktree-push-fail-repo");
    init_git_repo(&repo.path);
    let missing_remote = repo.path.join("missing-remote.git");
    add_origin(&repo.path, &missing_remote);

    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Push Failure",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write worktree file");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(worktree.exists());
    assert!(
        closed["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("push failed"))
    );

    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(!worktree.exists());
    assert!(git_ref_exists(
        &repo.path,
        "refs/heads/elesdee/web/push-failure"
    ));
}

#[tokio::test]
async fn push_success_removes_the_worktree_and_keeps_the_remote_branch() {
    let data_dir = TempDirectory::new("worktree-push-data");
    let repo = TempDirectory::new("worktree-push-repo");
    let remote_dir = TempDirectory::new("worktree-push-remote");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_origin(&repo.path, &remote);

    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Push Success",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write worktree file");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!worktree.exists());
    assert!(remote_ref_exists(&remote, &format!("refs/heads/{branch}")));
}

#[tokio::test]
async fn reopen_recreates_the_worktree_and_repo_root_survives_reload() {
    let data_dir = TempDirectory::new("worktree-reopen-data");
    let repo = TempDirectory::new("worktree-reopen-repo");
    init_git_repo(&repo.path);
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Reopen Work",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let branch = session["branch"].as_str().expect("branch").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!worktree.exists());
    drop(app);

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    let state: Value = serde_json::from_str(&state_text).expect("state json");
    assert_eq!(
        state["sessions"][0]["repoRoot"],
        repo.path
            .canonicalize()
            .expect("canonical repo")
            .to_string_lossy()
            .as_ref()
    );

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, reopened) = call(
        &restarted,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["branch"], branch);
    assert!(worktree.is_dir());

    let (status, closed) = close_session(&restarted, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn non_git_folder_is_rejected_for_agent_sessions() {
    let data_dir = TempDirectory::new("worktree-nongit-data");
    let folder = TempDirectory::new("worktree-nongit-folder");
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &folder.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "folder": folder.path.to_string_lossy()
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"], "Agent sessions need a Git repository.");
}

#[tokio::test]
async fn close_and_reopen_are_serialized_during_slow_git() {
    let data_dir = TempDirectory::new("race-data");
    let repo = TempDirectory::new("race-repo");
    init_git_repo(&repo.path);
    set_hook(&repo.path, "pre-commit", "#!/bin/sh\nsleep 2\n");
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Race",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write file");

    let close_request = request_builder("POST", &format!("/api/sessions/{id}/close"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("build close request");
    let close_app = app.clone();
    let close_task = tokio::spawn(async move {
        close_app
            .oneshot(close_request)
            .await
            .expect("close request")
            .status()
    });

    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("closing"))
    );

    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("closing"))
    );

    let close_status = tokio::time::timeout(TIMEOUT, close_task)
        .await
        .expect("close did not finish")
        .expect("close task failed");
    assert_eq!(close_status, StatusCode::OK);
    assert!(!worktree.exists());
}

#[tokio::test]
async fn slow_git_times_out_and_keeps_the_worktree() {
    let data_dir = TempDirectory::new("git-timeout-data");
    let repo = TempDirectory::new("git-timeout-repo");
    init_git_repo(&repo.path);
    set_hook(&repo.path, "pre-commit", "#!/bin/sh\nsleep 5\n");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_git_timeout(Duration::from_millis(500), Duration::from_millis(500));
    let app = router(state);
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Timeout",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write file");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(worktree.exists());
    assert!(
        closed["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("timed out"))
    );
}

#[tokio::test]
async fn branch_name_reuse_after_delete_gets_a_new_suffix() {
    let data_dir = TempDirectory::new("branch-reuse-data");
    let repo = TempDirectory::new("branch-reuse-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let body = json!({
        "projectId": "web",
        "kind": "agent",
        "providerId": "max",
            "client": "Claude Code",
        "name": "Reuse",
        "folder": repo.path.to_string_lossy()
    });

    let (status, first) = call(&app, "POST", "/api/sessions", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first_id = first["id"].as_str().expect("session id");
    assert_eq!(first["branch"], "elesdee/web/reuse");
    let (status, closed) = close_session(&app, first_id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let (status, delete_body) =
        call(&app, "DELETE", &format!("/api/sessions/{first_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{delete_body}");
    assert!(git_ref_exists(&repo.path, "refs/heads/elesdee/web/reuse"));

    let (status, second) = call(&app, "POST", "/api/sessions", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["short"], "reuse-2");
    assert_eq!(second["branch"], "elesdee/web/reuse-2");
    let second_id = second["id"].as_str().expect("session id");
    let (status, closed) = close_session(&app, second_id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn close_pushes_to_a_single_non_origin_remote() {
    let data_dir = TempDirectory::new("upstream-data");
    let repo = TempDirectory::new("upstream-repo");
    let remote_dir = TempDirectory::new("upstream-remote");
    init_git_repo(&repo.path);
    let remote = remote_dir.path.join("remote.git");
    init_bare_repo(&remote);
    add_remote_named(&repo.path, "upstream", &remote);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Upstream",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let branch = session["branch"].as_str().expect("branch");
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write file");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!worktree.exists());
    assert!(remote_ref_exists(&remote, &format!("refs/heads/{branch}")));
}

#[tokio::test]
async fn spawn_failure_rolls_back_the_worktree_and_branch() {
    let data_dir = TempDirectory::new("spawn-fail-data");
    let repo = TempDirectory::new("spawn-fail-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/definitely/not/a/real/command".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Spawn Fail",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(!git_ref_exists(
        &repo.path,
        "refs/heads/elesdee/web/spawn-fail"
    ));
    let worktrees = data_dir.path.join("worktrees");
    assert!(
        !worktrees.exists()
            || fs::read_dir(worktrees)
                .expect("worktrees directory")
                .next()
                .is_none()
    );
}

#[tokio::test]
async fn reopen_replaces_a_stale_worktree_directory() {
    let data_dir = TempDirectory::new("stale-data");
    let repo = TempDirectory::new("stale-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Stale",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    fs::create_dir_all(&worktree).expect("create stale directory");

    let (status, reopened) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert!(worktree.join(".git").is_file());

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn refused_worktree_removal_keeps_it_with_a_preview() {
    let data_dir = TempDirectory::new("refused-data");
    let repo = TempDirectory::new("refused-repo");
    init_git_repo(&repo.path);
    set_hook(
        &repo.path,
        "pre-commit",
        "#!/bin/sh\nprintf late > late.txt\n",
    );
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Refused",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::write(worktree.join("new.txt"), "hello\n").expect("write file");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(worktree.exists());
    assert!(worktree.join("late.txt").exists());
    assert!(
        closed["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("could not be removed"))
    );
}

#[tokio::test]
async fn close_with_a_missing_worktree_sets_a_truthful_preview() {
    let data_dir = TempDirectory::new("missing-worktree-data");
    let repo = TempDirectory::new("missing-worktree-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "providerId": "max",
            "client": "Claude Code",
            "name": "Missing Worktree",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let worktree = PathBuf::from(session["folder"].as_str().expect("folder"));
    fs::remove_dir_all(&worktree).expect("remove worktree");

    let (status, closed) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(
        closed["preview"],
        "Closed. The worktree folder was already missing."
    );
}

#[tokio::test]
async fn output_updates_the_preview_with_ansi_stripped() {
    let app = router(AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf '\\033[31mHello\\033[0m  world\\n'; sleep 30".to_owned(),
    ]));
    let folder = TempDirectory::new("preview-ansi");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    wait_for_preview(&app, &id, "Hello world").await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn rapid_output_updates_the_preview_once_per_throttle_window() {
    let state = AppState::with_test_command(vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        concat!(
            "sleep 0.3; printf 'first\\n'; sleep 0.05; printf 'second\\n'; ",
            "sleep 0.05; printf 'third\\n'; sleep 0.05; printf 'fourth\\n'; sleep 30"
        )
        .to_owned(),
    ])
    .with_test_preview_throttle(Duration::from_millis(500));
    let app = router(state);
    let folder = TempDirectory::new("preview-throttle");

    let request = request_builder("GET", "/api/events")
        .body(Body::empty())
        .expect("build events request");
    let events = app.clone().oneshot(request).await.expect("open events");
    assert_eq!(events.status(), StatusCode::OK);

    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let text = read_events_for(events.into_body(), Duration::from_millis(900)).await;
    let values = sse_values(&text);
    let previews = values
        .iter()
        .filter(|event| {
            event["type"] == "session.updated"
                && event["session"]["id"].as_str() == Some(id.as_str())
        })
        .filter_map(|event| event["session"]["preview"].as_str())
        .filter(|preview| ["first", "second", "third", "fourth"].contains(preview))
        .collect::<Vec<_>>();

    assert_eq!(previews.len(), 1, "previews: {previews:?}");
    assert_eq!(previews[0], "first");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn preview_is_saved_and_restored() {
    let data_dir = TempDirectory::new("preview-persist-data");
    let folder = TempDirectory::new("preview-persist-folder");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        "printf 'Saved preview\\n'; sleep 30".to_owned(),
    ];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    wait_for_preview(&app, &id, "Saved preview").await;

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    drop(app);

    let restarted = router(AppState::with_test_command_and_data_dir(
        command,
        data_dir.path.clone(),
    ));
    let (status, bootstrap) = call(&restarted, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let restored = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("restored session");
    assert_eq!(restored["preview"], "Saved preview");
}

#[tokio::test]
async fn claude_agent_writes_private_hook_settings_and_rejects_bad_tokens() {
    let data_dir = TempDirectory::new("claude-settings-data");
    let repo = TempDirectory::new("claude-settings-repo");
    init_git_repo(&repo.path);
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state.clone());
    let session = create_claude_agent(&app, &repo.path, "Hook Settings").await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (settings_path, settings) = claude_settings(&data_dir.path, &id);
    assert!(settings_path.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&settings_path)
            .expect("settings metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "settings file mode");
        let directory_mode = fs::metadata(settings_path.parent().expect("settings directory"))
            .expect("settings directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700, "settings directory mode");
    }

    let token = settings_token(&settings);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        !bootstrap.to_string().contains(&token),
        "hook token leaked into bootstrap"
    );
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(
        !state_text.contains(&token),
        "hook token leaked into state.json"
    );
    for event in [
        "PreToolUse",
        "PostToolUse",
        "Notification",
        "Stop",
        "UserPromptSubmit",
        "MessageDisplay",
    ] {
        let hook = &settings["hooks"][event][0]["hooks"][0];
        assert_eq!(hook["type"], "http", "{event}");
        assert!(
            hook["url"]
                .as_str()
                .expect("hook url")
                .contains(&format!("/internal/hooks/claude/{id}?token={token}")),
            "{event} url"
        );
    }

    let matcher = "Bash|Edit|Write|MultiEdit|NotebookEdit|WebFetch";
    for event in ["PreToolUse", "PostToolUse"] {
        assert_eq!(settings["hooks"][event][0]["matcher"], matcher, "{event}");
    }
    for event in ["Notification", "Stop", "UserPromptSubmit", "MessageDisplay"] {
        assert!(
            settings["hooks"][event][0].get("matcher").is_none(),
            "{event} must stay unmatcher"
        );
    }

    let body = json!({"hook_event_name": "UnknownEvent"});
    let (status, response) = send_hook(&app, &id, Some(&token), body.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{response}");
    let (status, response) = send_hook(&app, &id, None, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    let (status, response) = send_hook(&app, &id, Some("wrong-token"), body).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");

    let codex = create_agent(&app, &repo.path, "codex", "Codex hooks").await;
    let codex_id = codex["id"].as_str().expect("codex session id");
    let (status, response) = send_hook(
        &app,
        codex_id,
        Some(&token),
        json!({"hook_event_name": "UserPromptSubmit"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");

    let entry = state.session(&id).expect("session entry");
    let arguments = entry
        .stored_command()
        .expect("stored command")
        .get_argv()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        !arguments.iter().any(|argument| argument == "--settings"),
        "test command must not gain --settings: {arguments:?}"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!settings_path.exists(), "settings file survived close");
}

#[tokio::test]
async fn pre_tool_use_allow_answer_returns_and_clears_on_post_tool_use() {
    let data_dir = TempDirectory::new("claude-allow-data");
    let repo = TempDirectory::new("claude-allow-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Allow Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "session_id": "claude-session",
            "cwd": repo.path.to_string_lossy(),
            "tool_name": "Bash",
            "tool_input": {"command": "echo token=abc\nnext"},
            "tool_use_id": "toolu-allow"
        }),
    );
    let answer = async {
        let pending = wait_for_pending(&app, &id).await;
        assert_eq!(pending["requestId"], "toolu-allow");
        assert_eq!(pending["tool"], "Bash");
        assert_eq!(pending["summary"], "echo token=abc next");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-allow", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

        let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
        let current = bootstrap_session(&bootstrap, &id);
        assert_eq!(current["waitingFor"], "permission");
        assert_eq!(current["pendingPermission"]["requestId"], "toolu-allow");
    };

    let ((hook_status, hook_body), ()) = tokio::join!(hook, answer);
    assert_eq!(hook_status, StatusCode::OK, "{hook_body}");
    let output = &hook_body["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "PreToolUse");
    assert_eq!(output["permissionDecision"], "allow");
    assert_eq!(
        output["permissionDecisionReason"],
        "The user allowed this action."
    );

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "PostToolUse", "tool_use_id": "toolu-allow"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["pendingPermission"], Value::Null);
    assert_eq!(current["waitingFor"], Value::Null);
    assert_eq!(current["workState"], "working");
    assert_eq!(current["status"], "active");
}

#[tokio::test]
async fn permission_cards_skip_read_only_question_and_plan_tools() {
    let data_dir = TempDirectory::new("claude-filter-data");
    let repo = TempDirectory::new("claude-filter-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Filter Tools").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    for (index, tool) in ["Read", "Glob", "Grep", "AskUserQuestion", "ExitPlanMode"]
        .into_iter()
        .enumerate()
    {
        let (status, body) = send_hook(
            &app,
            &id,
            Some(&token),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": tool,
                "tool_input": {"file_path": "/etc/passwd"},
                "tool_use_id": format!("toolu-filter-{index}")
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{tool}: {body}");

        let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
        let current = bootstrap_session(&bootstrap, &id);
        assert_eq!(current["pendingPermission"], Value::Null, "{tool}");
        assert_eq!(current["waitingFor"], Value::Null, "{tool}");
    }

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo allowed"},
            "tool_use_id": "toolu-filter-bash"
        }),
    );
    let answer = async {
        let pending = wait_for_pending(&app, &id).await;
        assert_eq!(pending["requestId"], "toolu-filter-bash");
        assert_eq!(pending["tool"], "Bash");
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-filter-bash", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    };

    let ((status, body), ()) = tokio::join!(hook, answer);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hookSpecificOutput"]["permissionDecision"], "allow");
    close_session(&app, &id).await;
}

#[tokio::test]
async fn pre_tool_use_deny_allows_only_one_answer() {
    let data_dir = TempDirectory::new("claude-deny-data");
    let repo = TempDirectory::new("claude-deny-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Deny Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": {"file_path": "/tmp/example.txt", "old_string": "do-not-store-this"},
            "tool_use_id": "toolu-deny"
        }),
    );
    let answer = async {
        let pending = wait_for_pending(&app, &id).await;
        assert_eq!(pending["tool"], "Edit");
        assert_eq!(pending["summary"], "/tmp/example.txt");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "other-id", "decision": "deny"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-deny", "decision": "deny"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-deny", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains("already answered")),
            "{body}"
        );
    };

    let ((hook_status, hook_body), ()) = tokio::join!(hook, answer);
    assert_eq!(hook_status, StatusCode::OK, "{hook_body}");
    assert_eq!(
        hook_body["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(
        hook_body["hookSpecificOutput"]["permissionDecisionReason"],
        "The user denied this action."
    );

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        !bootstrap.to_string().contains("do-not-store-this"),
        "raw tool input leaked into session state"
    );

    let (status, body) = call(
        &app,
        "POST",
        "/api/sessions/does-not-exist/permission",
        Some(json!({"requestId": "toolu-deny", "decision": "deny"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn unanswered_permission_expires_and_is_denied() {
    let data_dir = TempDirectory::new("claude-timeout-data");
    let repo = TempDirectory::new("claude-timeout-repo");
    init_git_repo(&repo.path);
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_permission_timeout(Duration::from_millis(150)),
    );
    let session = create_claude_agent(&app, &repo.path, "Timeout Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo hello"},
            "tool_use_id": "toolu-timeout"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        body["hookSpecificOutput"]["permissionDecisionReason"],
        "No answer from the user. The request expired."
    );

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["pendingPermission"], Value::Null);
    assert_eq!(current["waitingFor"], Value::Null);

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/permission"),
        Some(json!({"requestId": "toolu-timeout", "decision": "allow"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn closing_a_session_denies_a_pending_permission_and_removes_settings() {
    let data_dir = TempDirectory::new("claude-close-data");
    let repo = TempDirectory::new("claude-close-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Close Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (settings_path, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo close"},
            "tool_use_id": "toolu-close"
        }),
    );
    let close = async {
        wait_for_pending(&app, &id).await;
        let (status, closed) = close_session(&app, &id).await;
        assert_eq!(status, StatusCode::OK, "{closed}");
    };

    let ((hook_status, hook_body), ()) = tokio::join!(hook, close);
    assert_eq!(hook_status, StatusCode::OK, "{hook_body}");
    assert_eq!(
        hook_body["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(
        hook_body["hookSpecificOutput"]["permissionDecisionReason"],
        "The session was closed."
    );

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["status"], "closed");
    assert_eq!(current["pendingPermission"], Value::Null);
    assert!(!settings_path.exists(), "settings file survived close");

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "MessageDisplay", "message": "late"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn agent_exit_denies_a_pending_permission() {
    let data_dir = TempDirectory::new("claude-exit-data");
    let repo = TempDirectory::new("claude-exit-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 1".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Exit Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (settings_path, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo exit"},
            "tool_use_id": "toolu-exit"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["hookSpecificOutput"]["permissionDecisionReason"],
        "The agent exited."
    );

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["status"], "active");
    assert_eq!(current["workState"], "done");
    assert_eq!(current["pendingPermission"], Value::Null);
    assert_eq!(current["waitingFor"], Value::Null);
    assert!(
        !settings_path.exists(),
        "settings file survived the agent exit"
    );

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "UserPromptSubmit"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn claude_notifications_messages_and_stop_update_session_state() {
    let data_dir = TempDirectory::new("claude-state-data");
    let repo = TempDirectory::new("claude-state-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Hook State").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["waitingFor"], "permission");
    assert_eq!(current["pendingPermission"], Value::Null);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "MessageDisplay", "message": "Building the app..."}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap_session(&bootstrap, &id)["preview"],
        "Building the app..."
    );

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Stop",
            "last_assistant_message": "Build finished.\nAll tests pass."
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["workState"], "done");
    assert_eq!(current["waitingFor"], Value::Null);
    assert_eq!(current["preview"], "All tests pass.");

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "UserPromptSubmit"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap_session(&bootstrap, &id)["workState"], "working");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn claude_idle_notifications_set_and_clear_the_input_waiting_state() {
    let data_dir = TempDirectory::new("claude-idle-data");
    let repo = TempDirectory::new("claude-idle-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Idle State").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["workState"], "waiting");
    assert_eq!(current["waitingFor"], "input");

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "PostToolUse"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["workState"], "working");
    assert_eq!(current["waitingFor"], Value::Null);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "UserPromptSubmit"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(current["workState"], "working");
    assert_eq!(current["waitingFor"], Value::Null);

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let current = bootstrap_session(&bootstrap, &id);
    assert_eq!(
        current["waitingFor"], "permission",
        "an idle notice must not hide a permission card"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn dropped_hook_connection_clears_the_pending_permission() {
    let data_dir = TempDirectory::new("claude-cancel-data");
    let repo = TempDirectory::new("claude-cancel-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Cancel Tool").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let request = request_builder(
        "POST",
        &format!("/internal/hooks/claude/{id}?token={token}"),
    )
    .header(header::CONTENT_TYPE, "application/json")
    .body(Body::from(
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo cancel"},
            "tool_use_id": "toolu-cancel"
        })
        .to_string(),
    ))
    .expect("build hook request");
    let hook_app = app.clone();
    let hook = tokio::spawn(async move { send(&hook_app, request).await });

    wait_for_pending(&app, &id).await;
    hook.abort();
    assert!(hook.await.is_err(), "hook task should be cancelled");

    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
        if bootstrap_session(&bootstrap, &id)["pendingPermission"] == Value::Null {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "pending permission survived a dropped hook"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/permission"),
        Some(json!({"requestId": "toolu-cancel", "decision": "allow"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn reopen_recreates_claude_hook_settings_with_a_new_token() {
    let data_dir = TempDirectory::new("claude-reopen-data");
    let repo = TempDirectory::new("claude-reopen-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Reopen Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (settings_path, settings) = claude_settings(&data_dir.path, &id);
    let first_token = settings_token(&settings);

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!settings_path.exists());

    let (status, reopened) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert!(settings_path.is_file(), "settings file was not recreated");

    let (_, settings) = claude_settings(&data_dir.path, &id);
    let second_token = settings_token(&settings);
    assert_ne!(first_token, second_token);

    let body = json!({"hook_event_name": "UserPromptSubmit"});
    let (status, response) = send_hook(&app, &id, Some(&first_token), body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    let (status, response) = send_hook(&app, &id, Some(&second_token), body).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{response}");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn startup_sweeps_stale_claude_settings_files() {
    let data_dir = TempDirectory::new("claude-sweep-data");
    let settings_dir = data_dir.path.join("claude-settings");
    fs::create_dir_all(&settings_dir).expect("create settings directory");
    let stale = settings_dir.join("orphan.json");
    let other = settings_dir.join("note.txt");
    fs::write(&stale, "{}").expect("write stale settings");
    fs::write(&other, "keep").expect("write other file");

    let _state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 0".to_owned()],
        data_dir.path.clone(),
    );

    assert!(!stale.exists(), "stale settings file survived startup");
    assert!(other.exists(), "unrelated file was removed");
}

#[tokio::test]
async fn hook_route_rejects_bad_hosts_origins_and_input() {
    let data_dir = TempDirectory::new("claude-guard-data");
    let repo = TempDirectory::new("claude-guard-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Guard").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);
    let uri = format!("/internal/hooks/claude/{id}?token={token}");
    let payload = json!({"hook_event_name": "UnknownEvent"}).to_string();

    let request = Request::builder()
        .method("POST")
        .uri(&uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let request = Request::builder()
        .method("POST")
        .uri(&uri)
        .header(header::HOST, "evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let request = request_builder("POST", &uri)
        .header(header::ORIGIN, "http://evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let request = request_builder("POST", &uri)
        .header(header::ORIGIN, "http://localhost:5173")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.clone()))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let request = request_builder("POST", &uri)
        .body(Body::from("{}"))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");

    let request = request_builder("POST", &uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))
        .expect("build request");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Invalid hook input.");

    let terminal = create_terminal(&app, &repo.path).await;
    let terminal_id = terminal["id"].as_str().expect("terminal id");
    let (status, body) = send_hook(
        &app,
        terminal_id,
        Some(&token),
        json!({"hook_event_name": "Stop"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    close_session(&app, terminal_id).await;
    close_session(&app, &id).await;
}

#[tokio::test]
async fn delete_removes_a_stale_claude_settings_file() {
    let data_dir = TempDirectory::new("claude-delete-data");
    let repo = TempDirectory::new("claude-delete-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Delete Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (settings_path, _) = claude_settings(&data_dir.path, &id);

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!settings_path.exists());

    fs::write(&settings_path, "{}").expect("restore stale settings file");
    let (status, body) = call(&app, "DELETE", &format!("/api/sessions/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(
        !settings_path.exists(),
        "stale settings file survived delete"
    );
}

#[tokio::test]
async fn shutdown_removes_claude_hook_settings() {
    let data_dir = TempDirectory::new("claude-shutdown-data");
    let repo = TempDirectory::new("claude-shutdown-repo");
    init_git_repo(&repo.path);
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state.clone());
    let session = create_claude_agent(&app, &repo.path, "Shutdown Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (settings_path, _) = claude_settings(&data_dir.path, &id);

    let finished = tokio::task::spawn_blocking(move || state.shutdown_all())
        .await
        .expect("shutdown task");
    assert!(finished, "shutdown timed out");
    assert!(!settings_path.exists(), "settings file survived shutdown");
}

#[tokio::test]
async fn second_pre_tool_use_asks_while_the_first_is_held() {
    let data_dir = TempDirectory::new("claude-parallel-data");
    let repo = TempDirectory::new("claude-parallel-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Parallel Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let first = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo first"},
            "tool_use_id": "toolu-first"
        }),
    );
    let second = async {
        wait_for_pending(&app, &id).await;
        let (status, body) = send_hook(
            &app,
            &id,
            Some(&token),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "echo second"},
                "tool_use_id": "toolu-second"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["hookSpecificOutput"]["permissionDecision"], "ask");

        let pending = wait_for_pending(&app, &id).await;
        assert_eq!(pending["requestId"], "toolu-first");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-first", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    };

    let ((first_status, first_body), ()) = tokio::join!(first, second);
    assert_eq!(first_status, StatusCode::OK, "{first_body}");
    assert_eq!(
        first_body["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );

    let (status, body) = send_hook(
        &app,
        &id,
        Some(&token),
        json!({"hook_event_name": "PostToolUse", "tool_use_id": "toolu-first"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    close_session(&app, &id).await;
}

#[tokio::test]
async fn hook_summary_strips_invisible_format_characters() {
    let data_dir = TempDirectory::new("claude-format-data");
    let repo = TempDirectory::new("claude-format-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Format Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);
    let command = "echo safe\u{202E}gpj.exe\u{200B}\u{FEFF}\u{E000} --flag";

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "tool_use_id": "toolu-format"
        }),
    );
    let answer = async {
        let pending = wait_for_pending(&app, &id).await;
        let summary = pending["summary"].as_str().expect("summary");
        assert_eq!(summary, "echo safe gpj.exe --flag");
        for character in ['\u{202E}', '\u{200B}', '\u{FEFF}', '\u{E000}'] {
            assert!(!summary.contains(character), "summary kept {character:?}");
        }
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-format", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    };

    let ((status, body), ()) = tokio::join!(hook, answer);
    assert_eq!(status, StatusCode::OK, "{body}");
    close_session(&app, &id).await;
}

#[tokio::test]
async fn pending_permission_is_not_written_to_state() {
    let data_dir = TempDirectory::new("claude-persist-data");
    let repo = TempDirectory::new("claude-persist-repo");
    init_git_repo(&repo.path);
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state.clone());
    let session = create_claude_agent(&app, &repo.path, "Persist Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let hook = send_hook(
        &app,
        &id,
        Some(&token),
        json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo persist"},
            "tool_use_id": "toolu-persist"
        }),
    );
    let inspect = async {
        wait_for_pending(&app, &id).await;
        state.save();

        let text = fs::read_to_string(data_dir.path.join("state.json")).expect("read state");
        let persisted: Value = serde_json::from_str(&text).expect("parse state");
        let persisted_session = persisted["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .find(|entry| entry["session"]["id"] == id)
            .expect("persisted session");
        assert_eq!(
            persisted_session["session"]["pendingPermission"],
            Value::Null
        );
        assert_eq!(persisted_session["session"]["waitingFor"], Value::Null);
        assert!(!text.contains("toolu-persist"));

        let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
        let current = bootstrap_session(&bootstrap, &id);
        assert_eq!(current["pendingPermission"]["requestId"], "toolu-persist");
        assert_eq!(current["waitingFor"], "permission");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/permission"),
            Some(json!({"requestId": "toolu-persist", "decision": "allow"})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    };

    let ((status, body), ()) = tokio::join!(hook, inspect);
    assert_eq!(status, StatusCode::OK, "{body}");
    close_session(&app, &id).await;
}

#[tokio::test]
async fn concurrent_answers_accept_exactly_one() {
    let data_dir = TempDirectory::new("claude-concurrent-data");
    let repo = TempDirectory::new("claude-concurrent-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_claude_agent(&app, &repo.path, "Concurrent Hooks").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);

    let hook_app = app.clone();
    let hook_id = id.clone();
    let hook_token = token.clone();
    let hook = tokio::spawn(async move {
        send_hook(
            &hook_app,
            &hook_id,
            Some(&hook_token),
            json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "echo concurrent"},
                "tool_use_id": "toolu-concurrent"
            }),
        )
        .await
    });
    wait_for_pending(&app, &id).await;

    let permission_uri = format!("/api/sessions/{id}/permission");
    let allow = call(
        &app,
        "POST",
        &permission_uri,
        Some(json!({"requestId": "toolu-concurrent", "decision": "allow"})),
    );
    let deny = call(
        &app,
        "POST",
        &permission_uri,
        Some(json!({"requestId": "toolu-concurrent", "decision": "deny"})),
    );
    let ((allow_status, allow_body), (deny_status, deny_body)) = tokio::join!(allow, deny);

    let accepted = usize::from(allow_status == StatusCode::NO_CONTENT)
        + usize::from(deny_status == StatusCode::NO_CONTENT);
    assert_eq!(accepted, 1, "allow: {allow_body}, deny: {deny_body}");
    let rejected = if allow_status == StatusCode::NO_CONTENT {
        assert_eq!(deny_status, StatusCode::CONFLICT, "{deny_body}");
        deny_body
    } else {
        assert_eq!(allow_status, StatusCode::CONFLICT, "{allow_body}");
        allow_body
    };
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|error| error.contains("already answered")),
        "{rejected}"
    );

    let (hook_status, hook_body) = hook.await.expect("hook task");
    assert_eq!(hook_status, StatusCode::OK, "{hook_body}");
    let expected = if allow_status == StatusCode::NO_CONTENT {
        "allow"
    } else {
        "deny"
    };
    assert_eq!(
        hook_body["hookSpecificOutput"]["permissionDecision"],
        expected
    );
    close_session(&app, &id).await;
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn write_installer(directory: &Path, succeed: bool) -> PathBuf {
    let path = directory.join(if succeed {
        "install-ok.sh"
    } else {
        "install-fail.sh"
    });
    let body = if succeed {
        "#!/bin/sh\nset -eu\nmkdir -p \"$2/bin\"\nprintf '#!/bin/sh\\nprintf \"9.9.9\\\\n\"\\n' > \"$2/bin/$3\"\nchmod 755 \"$2/bin/$3\"\n"
    } else {
        "#!/bin/sh\nset -eu\nmkdir -p \"$2/bin\"\nprintf 'partial' > \"$2/bin/$3\"\necho 'the download failed' >&2\nexit 1\n"
    };
    fs::write(&path, body).expect("write fake installer");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make fake installer executable");
    }
    path
}

async fn poll_install(app: &Router, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let (status, body) = call(app, "GET", &format!("/api/clients/install/{id}"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["state"] != "running" {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the install did not finish");
}

#[tokio::test]
async fn client_install_verifies_the_version_and_leaves_no_partial_copy() {
    let data_dir = TempDirectory::new("client-install");
    let ok_installer = write_installer(&data_dir.path, true);
    let fail_installer = write_installer(&data_dir.path, false);
    let app = router(AppState::with_data_dir(data_dir.path.clone()));

    let guard = EnvGuard::set("ELESDEE_INSTALL_BIN", &ok_installer);
    let (status, started) = call(&app, "POST", "/api/clients/codex/install", Some(json!({}))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["state"], "running");
    assert_eq!(started["client"], "Codex");
    let id = started["id"].as_str().expect("install id").to_owned();
    let finished = poll_install(&app, &id).await;
    assert_eq!(finished["state"], "done", "{finished}");
    assert_eq!(finished["version"], "9.9.9");
    let installed = PathBuf::from(finished["path"].as_str().expect("install path"));
    assert!(installed.is_file(), "{}", installed.display());
    assert!(installed.starts_with(data_dir.path.join("clients/codex")));
    drop(guard);

    let guard = EnvGuard::set("ELESDEE_INSTALL_BIN", &fail_installer);
    let (status, started) = call(
        &app,
        "POST",
        "/api/clients/opencode-2/install",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let id = started["id"].as_str().expect("install id").to_owned();
    let failed = poll_install(&app, &id).await;
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(
        failed["error"]
            .as_str()
            .is_some_and(|error| error.contains("the download failed")),
        "{failed}"
    );
    assert!(!data_dir.path.join("clients/opencode-2").exists());
    drop(guard);

    let (status, unknown) = call(
        &app,
        "POST",
        "/api/clients/unknown/install",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{unknown}");
    let (status, unknown) = call(&app, "GET", "/api/clients/install/missing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{unknown}");

    let (status, health) = call(&app, "GET", "/api/health", None).await;
    assert_eq!(status, StatusCode::OK, "{health}");
    let agents = health["agents"].as_array().expect("agents");
    assert_eq!(agents.len(), 4);
    assert!(agents.iter().all(|agent| agent.get("path").is_some()));
}

#[tokio::test]
async fn health_lists_antigravity() {
    let app = router(AppState::ephemeral());
    let (status, body) = call(&app, "GET", "/api/health", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names = body["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .map(|agent| agent["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 4);
    assert!(names.contains(&"Antigravity"));
}

#[tokio::test]
async fn google_account_is_stored_without_its_key_and_validated() {
    let data_dir = TempDirectory::new("google-account-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);

    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Google Gemini",
            "vendor": "google",
            "auth": "key",
            "key": "AIza-test-key-123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    let id = provider["id"].as_str().expect("provider id").to_owned();
    assert_eq!(provider["supportedClients"], json!(["Antigravity"]));
    assert_eq!(provider["vendor"], "google");
    assert_eq!(provider["auth"], "key");
    assert_eq!(provider["sample"], false);
    assert_eq!(provider["rows"].as_array().expect("rows").len(), 0);
    assert_eq!(provider["foot"], "No official usage source.");
    assert!(provider.get("key").is_none());
    assert_eq!(
        secrets.get_key(&id).expect("read stored key"),
        Some("AIza-test-key-123".to_owned())
    );

    let (status, config) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({ "providerId": id, "client": "Antigravity" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    assert_eq!(config["model"], "gemini-3.8-flash-medium");
    assert_eq!(config["name"], "Google Gemini (Antigravity)");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap["providers"].as_array().expect("providers").len(),
        5
    );
    assert_eq!(bootstrap["configs"].as_array().expect("configs").len(), 5);
    assert!(!bootstrap.to_string().contains("AIza-test-key-123"));
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("AIza-test-key-123"));
    assert!(state_text.contains("Antigravity"));

    let (status, duplicate) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Google Gemini",
            "vendor": "google",
            "auth": "key",
            "key": "AIza-second-key"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{duplicate}");
    assert_eq!(
        duplicate["id"].as_str().expect("duplicate id"),
        format!("{id}-2")
    );

    let invalid = [
        (
            json!({"name": "Unknown vendor", "vendor": "nope", "auth": "key", "key": "k"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "Wrong auth", "vendor": "google", "auth": "subscription", "key": "k"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "No key", "vendor": "google", "auth": "key", "key": "  "}),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (request, expected) in invalid {
        let (status, body) = call(&app, "POST", "/api/providers", Some(request)).await;
        assert_eq!(status, expected, "{body}");
    }
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(
        bootstrap["providers"].as_array().expect("providers").len(),
        6
    );

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": id,
            "client": "Antigravity",
            "model": "x".repeat(121)
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn provider_list_matches_the_supported_table() {
    let app = router(AppState::ephemeral());
    let (status, options) = call(&app, "GET", "/api/provider-options", None).await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let options = options.as_array().expect("provider options");
    let ids = options
        .iter()
        .map(|provider| provider["id"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    for id in [
        "anthropic",
        "openai",
        "google",
        "deepseek",
        "kimi",
        "zai",
        "openrouter",
        "ollama",
        "lmstudio",
        "custom",
    ] {
        assert!(ids.contains(&id), "missing provider {id}: {ids:?}");
    }
    let custom = options
        .iter()
        .find(|provider| provider["id"] == "custom")
        .expect("custom provider");
    assert_eq!(custom["custom"], true);

    let client_table = [
        ("anthropic", json!(["Claude Code", "OpenCode 2"])),
        ("openai", json!(["Codex", "OpenCode 2"])),
        ("google", json!(["Antigravity"])),
        ("deepseek", json!(["Claude Code", "Codex", "OpenCode 2"])),
        ("openrouter", json!(["OpenCode 2", "Codex"])),
    ];
    for (id, clients) in client_table {
        let provider = options
            .iter()
            .find(|provider| provider["id"] == id)
            .expect("provider option");
        assert_eq!(provider["supportedClients"], clients, "{id}");
    }
    let claude = options
        .iter()
        .find(|provider| provider["id"] == "anthropic")
        .expect("anthropic");
    assert_eq!(
        claude["efforts"]["Claude Code"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(
        claude["efforts"]["OpenCode 2"],
        json!(["low", "medium", "high"])
    );
}

#[tokio::test]
async fn deepseek_account_keeps_its_key_out_of_the_state() {
    let data_dir = TempDirectory::new("deepseek-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);

    let (status, account) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "DeepSeek work",
            "vendor": "deepseek",
            "auth": "key",
            "key": "sk-deepseek-secret"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(
        account["supportedClients"],
        json!(["Claude Code", "Codex", "OpenCode 2"])
    );
    assert_eq!(account["vendor"], "deepseek");
    assert!(account.get("key").is_none(), "the response leaked the key");
    let id = account["id"].as_str().expect("provider id");
    assert_eq!(
        secrets.get_key(id).expect("read key").as_deref(),
        Some("sk-deepseek-secret")
    );

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let text = bootstrap.to_string();
    assert!(
        !text.contains("sk-deepseek-secret"),
        "bootstrap leaked the key"
    );
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("read state file");
    assert!(
        !state_text.contains("sk-deepseek-secret"),
        "state leaked the key"
    );
}

#[tokio::test]
async fn custom_provider_account_validates_style_client_and_server() {
    let app = router(AppState::ephemeral());
    let valid = json!({
        "name": "Gateway",
        "vendor": "custom",
        "auth": "key",
        "apiStyle": "openai",
        "key": "sk-gateway",
        "server": "https://gateway.example/v1"
    });
    let (status, account) = call(&app, "POST", "/api/providers", Some(valid)).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["supportedClients"], json!(["Codex", "OpenCode 2"]));
    assert_eq!(account["apiStyle"], "openai");
    assert_eq!(account["server"], "https://gateway.example/v1");
    let provider_id = account["id"].as_str().expect("provider id");

    let (status, config) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Codex",
            "model": "gpt-test"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    assert_eq!(config["model"], "gpt-test");

    let (status, body) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({"providerId": provider_id, "client": "Claude Code"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let invalid = [
        (
            json!({"name": "No style", "vendor": "custom", "auth": "key", "key": "k", "server": "https://example.com"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "No server", "vendor": "custom", "auth": "key", "apiStyle": "openai", "key": "k"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "Bad server", "vendor": "custom", "auth": "key", "apiStyle": "openai", "key": "k", "server": "ftp://example.com"}),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (request, expected) in invalid {
        let (status, body) = call(&app, "POST", "/api/providers", Some(request)).await;
        assert_eq!(status, expected, "{body}");
    }
}

#[tokio::test]
async fn account_list_filters_and_never_includes_keys() {
    let data_dir = TempDirectory::new("account-list-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    let key = "AIza-account-list-secret";
    create_google_account(&app, "Google Gemini", key).await;

    let (status, all) = call(&app, "GET", "/api/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let accounts = all.as_array().expect("providers");
    assert_eq!(accounts.len(), 5);
    let names = accounts
        .iter()
        .map(|account| account["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "Anthropic Max 20×",
            "ChatGPT Pro",
            "DeepSeek API",
            "Google Gemini",
            "LiteLLM gateway"
        ]
    );
    assert!(
        !all.to_string().contains(key),
        "the key leaked into the list"
    );
    for account in accounts {
        assert!(account.get("key").is_none(), "an account carried a key");
    }

    for (filter, expected) in [
        ("chatgpt", "ChatGPT Pro"),
        ("google", "Google Gemini"),
        ("team", "LiteLLM gateway"),
        ("anthropic", "Anthropic Max 20×"),
    ] {
        let (status, filtered) =
            call(&app, "GET", &format!("/api/providers?q={filter}"), None).await;
        assert_eq!(status, StatusCode::OK, "{filter}: {filtered}");
        let matches = filtered.as_array().expect("providers");
        assert_eq!(matches.len(), 1, "{filter}: {filtered}");
        assert_eq!(matches[0]["name"], expected, "{filter}");
    }

    let (status, empty) = call(&app, "GET", "/api/providers?q=", None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty.as_array().expect("providers").len(), 5);

    let (status, miss) = call(&app, "GET", "/api/providers?q=missing", None).await;
    assert_eq!(status, StatusCode::OK, "{miss}");
    assert!(miss.as_array().expect("providers").is_empty());
}

#[tokio::test]
async fn account_metadata_restores_and_a_missing_key_blocks_the_launch() {
    let data_dir = TempDirectory::new("google-persist-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    let (status, account) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Persistent Google",
            "vendor": "google",
            "auth": "key",
            "key": "AIza-persist-key"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let id = account["id"].as_str().expect("provider id").to_owned();
    let (status, config) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({ "providerId": id, "client": "Antigravity" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    let config_id = config["id"].as_str().expect("config id").to_owned();
    drop(app);

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("AIza-persist-key"));

    let restored_secrets = Arc::new(MemorySecretStore::default());
    let restored = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(restored_secrets.clone());
    let restored_app = router(restored);
    let (_, bootstrap) = call(&restored_app, "GET", "/api/bootstrap", None).await;
    let restored_account = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"].as_str() == Some(id.as_str()))
        .expect("restored provider");
    assert_eq!(restored_account["supportedClients"], json!(["Antigravity"]));
    assert_eq!(restored_account["sample"], false);
    let restored_config = bootstrap["configs"]
        .as_array()
        .expect("configs")
        .iter()
        .find(|config| config["id"].as_str() == Some(config_id.as_str()))
        .expect("restored config");
    assert_eq!(restored_config["model"], "gemini-3.8-flash-medium");
    assert_eq!(
        restored_secrets.get_key(&id).expect("read restored key"),
        None
    );

    let repo = TempDirectory::new("google-missing-key-repo");
    init_git_repo(&repo.path);
    ensure_test_project(&restored_app, &repo.path).await;
    let (status, body) = call(
        &restored_app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": id,
            "name": "Missing Key",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"], "The account key is not available.");
    let (_, bootstrap) = call(&restored_app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap["sessions"].as_array().expect("sessions").len(), 0);
    let homes = data_dir.path.join("homes");
    assert!(!homes.exists() || fs::read_dir(&homes).expect("read homes").next().is_none());
}

#[tokio::test]
async fn antigravity_session_uses_a_private_home_and_key_environment() {
    let data_dir = TempDirectory::new("google-launch-data");
    let repo = TempDirectory::new("google-launch-repo");
    init_git_repo(&repo.path);
    let key = "AIza-launch-key";
    let script = format!(
        "printf 'HOME=%s\\n' \"$HOME\"; if [ \"$GEMINI_API_KEY\" = \"{key}\" ]; then echo KEY_MATCH; else echo KEY_MISSING; fi; sleep 30"
    );
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), script],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    ensure_test_project(&app, &repo.path).await;
    let account = create_google_account(&app, "Launch Google", key).await;
    let account_id = account["id"].as_str().expect("account id");

    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": account_id,
            "name": "Antigravity Work",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["client"], "Antigravity");
    assert_eq!(session["providerId"], account_id);
    assert_eq!(session["shortcuts"].as_array().expect("shortcuts").len(), 0);
    assert_eq!(session["pendingPermission"], Value::Null);
    let (status, body) = send_hook(
        &app,
        &id,
        Some("not-a-hook-token"),
        json!({"hook_event_name": "PreToolUse"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        secrets.get_key(account_id).expect("read key"),
        Some(key.to_owned())
    );

    let home = data_dir.path.join("homes").join(&id);
    let settings = home.join(".gemini/antigravity-cli/settings.json");
    assert!(home.is_dir(), "session home was not created");
    assert_eq!(
        fs::read(&settings).expect("read settings"),
        br#"{"model":"gemini-3.8-flash-medium","modelProvider":"gemini"}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = fs::metadata(&settings)
            .expect("settings metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        let home_mode = fs::metadata(&home)
            .expect("home metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(home_mode, 0o700);
    }
    assert!(
        !data_dir
            .path
            .join("claude-settings")
            .join(format!("{id}.json"))
            .exists(),
        "Antigravity must not get Claude hook settings"
    );

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    assert_eq!(response.status(), StatusCode::OK);
    let output = read_output_until(response.into_body(), |text| {
        text.contains("KEY_MATCH") && text.contains("HOME=")
    })
    .await;
    assert!(
        output.contains(&format!("HOME={}", home.to_string_lossy())),
        "{output}"
    );
    assert!(!output.contains("KEY_MISSING"), "{output}");
    assert!(!output.contains(key), "the key leaked into the output log");

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains(key), "the key leaked into state.json");

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!home.exists(), "session home survived close");
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains(key), "the key leaked into state.json");
}

#[tokio::test]
async fn reopen_recreates_the_antigravity_home() {
    let data_dir = TempDirectory::new("google-reopen-data");
    let repo = TempDirectory::new("google-reopen-repo");
    init_git_repo(&repo.path);
    let key = "AIza-reopen-key";
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    ensure_test_project(&app, &repo.path).await;
    let account = create_google_account(&app, "Reopen Google", key).await;
    let account_id = account["id"].as_str().expect("account id");
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": account_id,
            "name": "Reopen Antigravity",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id").to_owned();
    let home = data_dir.path.join("homes").join(&id);
    let settings = home.join(".gemini/antigravity-cli/settings.json");
    assert!(settings.is_file());

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!home.exists());

    let (status, reopened) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/reopen"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert!(settings.is_file(), "settings were not recreated on reopen");
    assert_eq!(
        fs::read(&settings).expect("read settings"),
        br#"{"model":"gemini-3.8-flash-medium","modelProvider":"gemini"}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&settings)
            .expect("settings metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert!(!home.exists(), "reopened home survived close");
}

#[tokio::test]
async fn session_files_list_read_and_write_round_trip() {
    let folder = TempDirectory::new("files-roundtrip");
    fs::write(folder.path.join("note.txt"), "hello\n").expect("write note");
    fs::create_dir_all(folder.path.join("src")).expect("create src");
    fs::write(folder.path.join("src/lib.rs"), "fn main() {}\n").expect("write source");

    let app = router(AppState::ephemeral());
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, listing) = call(&app, "GET", &format!("/api/sessions/{id}/files"), None).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    let entries = listing["entries"].as_array().expect("entries");
    let names = entries
        .iter()
        .map(|entry| entry["name"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(names, ["src", "note.txt"]);
    assert_eq!(entries[0]["directory"], true);

    let (status, nested) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/files?path=src"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(nested["path"], "src");
    assert_eq!(nested["entries"][0]["path"], "src/lib.rs");

    let (status, file) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/file?path=note.txt"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{file}");
    assert_eq!(file["content"], "hello\n");
    assert_eq!(file["size"], 6);

    let (status, written) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": "note.txt", "content": "changed\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{written}");
    assert_eq!(written["content"], "changed\n");
    assert_eq!(
        fs::read_to_string(folder.path.join("note.txt")).expect("read note"),
        "changed\n"
    );

    let (status, created) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": "src/new.txt", "content": "new\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(
        fs::read_to_string(folder.path.join("src/new.txt")).expect("read new file"),
        "new\n"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn file_endpoints_reject_unsafe_and_oversized_content() {
    let folder = TempDirectory::new("files-guards");
    let outside = TempDirectory::new("files-outside");
    fs::write(folder.path.join("note.txt"), "hello\n").expect("write note");
    fs::create_dir_all(folder.path.join("src")).expect("create src");
    fs::write(folder.path.join("blob.bin"), [0_u8, 1, 2]).expect("write binary");
    fs::write(
        folder.path.join("big.txt"),
        vec![b'a'; MAX_FILE_BYTES as usize + 1],
    )
    .expect("write large file");
    fs::write(outside.path.join("secret.txt"), "secret\n").expect("write secret");
    fs::create_dir_all(folder.path.join(".git")).expect("create git folder");
    fs::write(folder.path.join(".git").join("config"), "[remote]\n").expect("write git config");

    let app = router(AppState::ephemeral());
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    for path in ["../secret.txt", "/etc/passwd", "src/.."] {
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/sessions/{id}/file?path={path}"),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{path} must be rejected: {body}"
        );
    }

    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/file?path=src"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/file?path=.git/config"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": ".git/config", "content": "changed\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        fs::read_to_string(folder.path.join(".git").join("config")).expect("read git config"),
        "[remote]\n"
    );

    #[cfg(unix)]
    {
        fs::hard_link(folder.path.join("note.txt"), folder.path.join("linked.txt"))
            .expect("hard link");
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/sessions/{id}/file?path=linked.txt"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }

    for route in ["file", "files"] {
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/sessions/{id}/{route}?path=note.txt/x"),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "a file used as a folder must not be a server error: {body}"
        );
    }

    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/file?path=blob.bin"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/sessions/{id}/file?path=big.txt"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");

    let oversized = "a".repeat(MAX_FILE_BYTES as usize + 1);
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": "note.txt", "content": oversized })),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(
        fs::read_to_string(folder.path.join("note.txt")).expect("read note"),
        "hello\n"
    );

    let beyond_axum_limit = "a".repeat(3 * 1024 * 1024);
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": "note.txt", "content": beyond_axum_limit })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "a request body over the framework limit must be 413: {body}"
    );

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/file"),
        Some(json!({ "path": "blob.bin", "content": "a\u{0}b" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            outside.path.join("secret.txt"),
            folder.path.join("escape.txt"),
        )
        .expect("create symlink");
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/sessions/{id}/file?path=escape.txt"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{id}/file"),
            Some(json!({ "path": "escape.txt", "content": "overwritten\n" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            fs::read_to_string(outside.path.join("secret.txt")).expect("read secret"),
            "secret\n"
        );
    }

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn terminal_changes_report_tracked_and_untracked_files() {
    let repo = TempDirectory::new("changes-repo");
    init_git_repo(&repo.path);
    fs::write(repo.path.join("README.md"), "initial\nmore\n").expect("change readme");
    fs::write(repo.path.join("new.txt"), "one\ntwo\n").expect("add file");

    let app = router(AppState::ephemeral());
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, changes) = call(&app, "GET", &format!("/api/sessions/{id}/changes"), None).await;
    assert_eq!(status, StatusCode::OK, "{changes}");
    assert_eq!(changes["git"], true);
    let files = changes["files"].as_array().expect("changed files");
    assert_eq!(files.len(), 2, "{changes}");

    let readme = files
        .iter()
        .find(|file| file["path"] == "README.md")
        .expect("README change");
    assert_eq!(readme["status"], "modified");
    assert_eq!(readme["additions"], 1);
    assert!(
        readme["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+more"),
        "{readme}"
    );

    let new_file = files
        .iter()
        .find(|file| file["path"] == "new.txt")
        .expect("untracked change");
    assert_eq!(new_file["status"], "untracked");
    assert_eq!(new_file["additions"], 2);
    assert!(
        new_file["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+++ b/new.txt"),
        "{new_file}"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let plain = TempDirectory::new("changes-plain");
    fs::write(plain.path.join("note.txt"), "hello\n").expect("write note");
    let session = create_terminal(&app, &plain.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (status, changes) = call(&app, "GET", &format!("/api/sessions/{id}/changes"), None).await;
    assert_eq!(status, StatusCode::OK, "{changes}");
    assert_eq!(changes["git"], false);
    assert!(changes["files"].as_array().expect("files").is_empty());
    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn large_diffs_and_nested_untracked_files_are_listed() {
    let repo = TempDirectory::new("changes-large-repo");
    init_git_repo(&repo.path);
    let large = (0..4000)
        .map(|line| format!("changed line {line} with enough text to fill the pipe\n"))
        .collect::<String>();
    fs::write(repo.path.join("README.md"), large).expect("write large change");
    fs::create_dir_all(repo.path.join("nested").join("deeper")).expect("create nested folder");
    fs::write(repo.path.join("nested").join("a.txt"), "alpha\n").expect("write a");
    fs::write(
        repo.path.join("nested").join("deeper").join("b.txt"),
        "beta\n",
    )
    .expect("write b");

    let app = router(AppState::ephemeral());
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, changes) = call(&app, "GET", &format!("/api/sessions/{id}/changes"), None).await;
    assert_eq!(status, StatusCode::OK, "{changes}");
    assert_eq!(changes["git"], true);
    assert!(changes["error"].is_null(), "git timed out: {changes}");
    let files = changes["files"].as_array().expect("changed files");
    let paths = files
        .iter()
        .map(|file| file["path"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"nested/a.txt"), "{paths:?}");
    assert!(paths.contains(&"nested/deeper/b.txt"), "{paths:?}");
    let readme = files
        .iter()
        .find(|file| file["path"] == "README.md")
        .expect("README change");
    assert!(
        readme["diff"].as_str().unwrap_or_default().len() > 100_000,
        "the diff was truncated before the file cap"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

#[tokio::test]
async fn agent_changes_include_commits_since_the_branch_point() {
    let data_dir = TempDirectory::new("changes-agent-data");
    let repo = TempDirectory::new("changes-agent-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let session = create_agent(&app, &repo.path, "max", "Changes agent").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let worktree = data_dir.path.join("worktrees").join(&id);
    assert!(worktree.is_dir());

    fs::write(worktree.join("committed.txt"), "committed\n").expect("write committed file");
    assert!(run_git(&worktree, &["add", "."]).status.success());
    let commit = run_git(&worktree, &["commit", "-m", "agent commit"]);
    assert!(commit.status.success(), "commit failed: {commit:?}");
    fs::write(worktree.join("README.md"), "initial\nwork\n").expect("change readme");

    let (status, changes) = call(&app, "GET", &format!("/api/sessions/{id}/changes"), None).await;
    assert_eq!(status, StatusCode::OK, "{changes}");
    assert_eq!(changes["git"], true);
    assert!(changes["base"].is_string(), "{changes}");

    let files = changes["files"].as_array().expect("changed files");
    let committed = files
        .iter()
        .find(|file| file["path"] == "committed.txt")
        .expect("committed change");
    assert_eq!(committed["status"], "added");
    assert_eq!(committed["additions"], 1);

    let readme = files
        .iter()
        .find(|file| file["path"] == "README.md")
        .expect("working change");
    assert_eq!(readme["status"], "modified");
    assert!(
        readme["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+work"),
        "{readme}"
    );

    let (status, closed) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
}

async fn create_google_account(app: &Router, name: &str, key: &str) -> Value {
    let (status, provider) = call(
        app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": name,
            "vendor": "google",
            "auth": "key",
            "key": key
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create Google provider: {provider}");
    let (status, config) = call(
        app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider["id"],
            "client": "Antigravity"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create Google config: {config}");
    provider
}

async fn create_agent(app: &Router, repo: &Path, account: &str, name: &str) -> Value {
    ensure_test_project(app, repo).await;
    let config_id = config_for_account(app, account).await;
    let (status, session) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": config_id,
            "name": name,
            "folder": repo.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create agent session: {session}");
    session
}

async fn create_claude_agent(app: &Router, repo: &Path, name: &str) -> Value {
    create_agent(app, repo, "max", name).await
}

fn claude_settings(data_dir: &Path, id: &str) -> (PathBuf, Value) {
    let path = data_dir.join("claude-settings").join(format!("{id}.json"));
    let text = fs::read_to_string(&path).expect("read settings file");
    let settings = serde_json::from_str(&text).expect("parse settings file");
    (path, settings)
}

fn settings_token(settings: &Value) -> String {
    let url = settings["hooks"]["PreToolUse"][0]["hooks"][0]["url"]
        .as_str()
        .expect("hook url");
    url.split_once("token=")
        .expect("token in hook url")
        .1
        .to_owned()
}

async fn send_hook(
    app: &Router,
    id: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let uri = match token {
        Some(token) => format!("/internal/hooks/claude/{id}?token={token}"),
        None => format!("/internal/hooks/claude/{id}"),
    };
    call(app, "POST", &uri, Some(body)).await
}

async fn send_statusline(
    app: &Router,
    id: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let uri = match token {
        Some(token) => format!("/internal/statusline/claude/{id}?token={token}"),
        None => format!("/internal/statusline/claude/{id}"),
    };
    call(app, "POST", &uri, Some(body)).await
}

async fn wait_for_pending(app: &Router, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let (status, bootstrap) = call(app, "GET", "/api/bootstrap", None).await;
        assert_eq!(status, StatusCode::OK);
        let current = bootstrap_session(&bootstrap, id);
        if current["pendingPermission"].is_object() {
            return current["pendingPermission"].clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no pending permission for {id}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn bootstrap_session<'a>(bootstrap: &'a Value, id: &str) -> &'a Value {
    bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session in bootstrap")
}

fn run_git(dir: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .output()
        .expect("run git")
}

fn git_stdout(dir: &Path, arguments: &[&str]) -> String {
    let output = run_git(dir, arguments);
    assert!(output.status.success(), "git {arguments:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git_ref_exists(repo: &Path, reference: &str) -> bool {
    run_git(repo, &["rev-parse", "--verify", reference])
        .status
        .success()
}

fn init_git_repo(repo: &Path) {
    let output = run_git(repo, &["init"]);
    assert!(output.status.success(), "git init failed");
    fs::write(repo.join("README.md"), "initial\n").expect("write README");
    assert!(run_git(repo, &["add", "."]).status.success());
    let commit = run_git(
        repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "initial",
        ],
    );
    assert!(commit.status.success(), "git initial commit failed");
    assert!(
        run_git(repo, &["config", "user.name", "Test"])
            .status
            .success()
    );
    assert!(
        run_git(repo, &["config", "user.email", "test@example.com"])
            .status
            .success()
    );
}

fn init_bare_repo(path: &Path) {
    let output = Command::new("git")
        .args(["init", "--bare"])
        .arg(path)
        .output()
        .expect("run git init --bare");
    assert!(output.status.success(), "git init --bare failed");
}

fn add_origin(repo: &Path, remote: &Path) {
    add_remote_named(repo, "origin", remote);
}

fn add_remote_named(repo: &Path, name: &str, remote: &Path) {
    let remote = remote.to_string_lossy();
    let output = run_git(repo, &["remote", "add", name, remote.as_ref()]);
    assert!(output.status.success(), "git remote add failed");
}

fn set_hook(repo: &Path, name: &str, script: &str) {
    let hooks = repo.join(".git").join("hooks");
    fs::create_dir_all(&hooks).expect("create hooks directory");
    let path = hooks.join(name);
    fs::write(&path, script).expect("write hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make hook executable");
    }
}

fn remote_ref_exists(remote: &Path, reference: &str) -> bool {
    Command::new("git")
        .arg("--git-dir")
        .arg(remote)
        .args(["rev-parse", "--verify", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn child_pid(text: &str) -> Option<u32> {
    let mut remaining = text;
    while let Some(position) = remaining.find("CHILD:") {
        let after = &remaining[position + "CHILD:".len()..];
        let digits = after
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        if let Ok(pid) = digits.parse::<u32>() {
            return Some(pid);
        }
        remaining = after;
    }
    None
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    unsafe { libc::kill(pid, 0) == 0 }
}

#[derive(Clone)]
struct BalanceServer {
    calls: Arc<AtomicUsize>,
    auth: Arc<Mutex<Option<String>>>,
    body: Value,
    status: StatusCode,
}

async fn balance_handler(
    AxumState(server): AxumState<BalanceServer>,
    headers: axum::http::HeaderMap,
) -> Response {
    server.calls.fetch_add(1, Ordering::SeqCst);
    *server.auth.lock().expect("auth") = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    (server.status, axum::Json(server.body.clone())).into_response()
}

async fn start_balance_server(
    body: Value,
    status: StatusCode,
) -> (String, Arc<AtomicUsize>, Arc<Mutex<Option<String>>>) {
    let server = BalanceServer {
        calls: Arc::new(AtomicUsize::new(0)),
        auth: Arc::new(Mutex::new(None)),
        body,
        status,
    };
    let calls = Arc::clone(&server.calls);
    let auth = Arc::clone(&server.auth);
    let app = Router::new()
        .route("/user/balance", get(balance_handler))
        .with_state(server);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind balance server");
    let address = listener.local_addr().expect("balance server address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), calls, auth)
}

async fn create_deepseek_account(app: &Router, name: &str, key: &str) -> String {
    let (status, account) = call(
        app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": name,
            "vendor": "deepseek",
            "auth": "key",
            "key": key
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let provider_id = account["id"].as_str().expect("provider id").to_owned();
    let (status, config) = call(
        app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Claude Code"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    provider_id
}

#[tokio::test]
async fn refresh_without_an_official_source_returns_no_usage_data() {
    let app = router(AppState::ephemeral());
    let (status, account) = call(
        &app,
        "POST",
        "/api/providers/max/usage/refresh",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["usageStatus"], Value::Null);
    assert_eq!(account["usageReadAt"], Value::Null);
    assert_eq!(account["usageCheckedAt"], Value::Null);
    assert_eq!(account["foot"], "No official usage source.");
    assert_eq!(account["rows"].as_array().expect("rows").len(), 0);
}

#[tokio::test]
async fn deepseek_balance_is_read_from_the_official_endpoint_and_coalesced() {
    let body = json!({
        "is_available": true,
        "balance_infos": [{
            "currency": "USD",
            "total_balance": "110.00",
            "granted_balance": "10.00",
            "topped_up_balance": "100.00"
        }]
    });
    let (base, calls, auth) = start_balance_server(body, StatusCode::OK).await;
    let data_dir = TempDirectory::new("usage-deepseek");
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_usage_base(base),
    );
    let key = "sk-deepseek-secret";
    let id = create_deepseek_account(&app, "DeepSeek usage", key).await;
    let uri = format!("/api/providers/{id}/usage/refresh");

    let (first, second) = tokio::join!(
        call(&app, "POST", &uri, Some(json!({}))),
        call(&app, "POST", &uri, Some(json!({})))
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "concurrent reads must share one request"
    );
    assert_eq!(
        auth.lock().expect("auth").as_deref(),
        Some("Bearer sk-deepseek-secret")
    );

    let account = &first.1;
    assert_eq!(account["usageStatus"], "fresh");
    assert_eq!(account["usageSource"], "deepseek");
    assert!(account["usageReadAt"].as_str().is_some());
    assert_eq!(account["rows"][0]["window"], "balance");
    assert_eq!(account["rows"][0]["value"], "USD 110.00");
    assert!(
        !account.to_string().contains(key),
        "the key leaked into the API response"
    );
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        !bootstrap.to_string().contains(key),
        "the key leaked into bootstrap"
    );
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains(key), "the key leaked into state.json");
}

#[tokio::test]
async fn a_failed_balance_read_marks_the_account_unknown_and_keeps_the_key_private() {
    let (base, calls, auth) =
        start_balance_server(json!({}), StatusCode::INTERNAL_SERVER_ERROR).await;
    let data_dir = TempDirectory::new("usage-deepseek-fail");
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_usage_base(base),
    );
    let key = "sk-deepseek-failure";
    let id = create_deepseek_account(&app, "DeepSeek failure", key).await;

    let (status, response) = call(
        &app,
        "POST",
        &format!("/api/providers/{id}/usage/refresh"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{response}");
    assert!(!response.to_string().contains(key));

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let account = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == id)
        .expect("account");
    assert_eq!(account["usageStatus"], "unknown");
    assert_eq!(account["usageSource"], "deepseek");
    assert!(account["usageCheckedAt"].as_str().is_some());
    assert!(account["usageReadAt"].is_null());
    assert_eq!(account["rows"].as_array().expect("rows").len(), 0);
    assert!(!bootstrap.to_string().contains(key));
    assert_eq!(
        auth.lock().expect("auth").as_deref(),
        Some("Bearer sk-deepseek-failure")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains(key));
}

#[tokio::test]
async fn a_failed_read_keeps_the_last_values_and_marks_them_stale() {
    let (base, _, _) = start_balance_server(json!({}), StatusCode::OK).await;
    let data_dir = TempDirectory::new("usage-deepseek-stale");
    let app = router(
        AppState::with_test_command_and_data_dir(
            vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            data_dir.path.clone(),
        )
        .with_test_usage_base(base),
    );
    let key = "sk-deepseek-stale";
    let id = create_deepseek_account(&app, "DeepSeek stale", key).await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{id}/usage"),
        Some(json!({ "rows": [{ "label": "Manual", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, before) = call(&app, "GET", "/api/bootstrap", None).await;
    let read_at = before["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == id)
        .expect("account")["usageReadAt"]
        .clone();
    assert!(read_at.as_str().is_some());

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{id}/usage/refresh"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let account = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == id)
        .expect("account");
    assert_eq!(account["usageStatus"], "stale");
    assert_eq!(account["usageReadAt"], read_at);
    assert!(account["usageCheckedAt"].as_str().is_some());
    assert_eq!(account["rows"][0]["used"], 20);
    assert!(!bootstrap.to_string().contains(key));
}

#[tokio::test]
async fn claude_status_line_updates_usage_and_switches_the_session() {
    let data_dir = TempDirectory::new("usage-statusline-data");
    let repo = TempDirectory::new("usage-statusline-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let backup = create_test_account(&app, "Status backup", "sk-status-backup").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{backup}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let session = create_claude_agent(&app, &repo.path, "Status line").await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let (_, settings) = claude_settings(&data_dir.path, &id);
    let token = settings_token(&settings);
    let command = settings["statusLine"]["command"]
        .as_str()
        .expect("status line command");
    assert_eq!(settings["statusLine"]["type"], "command");
    assert!(
        command.contains(&format!("/internal/statusline/claude/{id}?token={token}")),
        "{command}"
    );

    let payload = json!({
        "rate_limits": {
            "five_hour": { "used_percentage": 95, "resets_at": 1738425600 },
            "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
        }
    });
    let (status, _) = send_statusline(&app, &id, Some("wrong-token"), payload.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_statusline(&app, &id, None, payload.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, response) = send_statusline(&app, &id, Some(&token), payload).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{response}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        !bootstrap.to_string().contains(&token),
        "the status line token leaked into bootstrap"
    );
    let max = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == "max")
        .expect("max account");
    assert_eq!(max["usageStatus"], "fresh");
    assert_eq!(max["usageSource"], "statusline");
    assert!(max["usageReadAt"].as_str().is_some());
    assert_eq!(max["foot"], "");
    assert_eq!(max["rows"][0]["window"], "five_hour");
    assert_eq!(max["rows"][0]["used"], 95);
    assert_eq!(max["rows"][0]["resetsAt"], 1738425600);
    assert_eq!(max["rows"][1]["window"], "seven_day");
    assert_eq!(max["rows"][1]["used"], 41);

    let switched = wait_for_session(&app, &id, |session| session["providerId"] == backup).await;
    assert_eq!(switched["status"], "active");
    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn edit_account_without_a_key_change_keeps_the_saved_key() {
    let data_dir = TempDirectory::new("account-edit-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    let id = create_test_account_for(&app, "Edit me", "deepseek", "sk-edit-secret").await;

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/providers/{id}"),
        Some(json!({
            "name": "Edited account",
            "vendor": "deepseek",
            "auth": "key"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Edited account");
    assert_eq!(updated["vendor"], "deepseek");
    assert_eq!(updated["id"], id);
    assert!(updated.get("key").is_none(), "the response leaked the key");
    assert_eq!(
        secrets.get_key(&id).expect("read key").as_deref(),
        Some("sk-edit-secret"),
        "a blank key field must keep the saved key"
    );

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(
        !state_text.contains("sk-edit-secret"),
        "state leaked the key"
    );
    assert!(state_text.contains("Edited account"));
}

#[tokio::test]
async fn edit_account_with_a_key_change_replaces_the_key() {
    let data_dir = TempDirectory::new("account-rekey-data");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    let id = create_test_account_for(&app, "Rekey", "anthropic", "sk-old").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{id}/usage"),
        Some(json!({ "rows": [{ "label": "5-hour", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/api/providers/{id}"),
        Some(json!({
            "name": "Rekey",
            "vendor": "anthropic",
            "auth": "key",
            "key": "sk-new"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert!(updated.get("key").is_none(), "the response leaked the key");
    assert_eq!(
        secrets.get_key(&id).expect("read key").as_deref(),
        Some("sk-new")
    );
    assert_eq!(
        updated["rows"].as_array().expect("rows").len(),
        1,
        "a key change keeps the stored usage"
    );
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("sk-new"), "state leaked the key");
    assert!(!state_text.contains("sk-old"), "state leaked the old key");
}

#[tokio::test]
async fn editing_a_sample_account_stops_it_being_a_sample() {
    let data_dir = TempDirectory::new("account-sample-data");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let seed = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == "max")
        .cloned()
        .expect("seed provider");
    assert_eq!(seed["sample"], true);

    let (status, updated) = call(
        &app,
        "PATCH",
        "/api/providers/max",
        Some(json!({
            "name": "My Claude",
            "vendor": "anthropic",
            "auth": "subscription"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["sample"], false,
        "an edited sample is a real provider"
    );
    assert_eq!(updated["name"], "My Claude");
    drop(app);

    let reloaded = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    let (_, bootstrap) = call(&reloaded, "GET", "/api/bootstrap", None).await;
    let account = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|account| account["id"] == "max")
        .expect("edited provider");
    assert_eq!(account["name"], "My Claude");
    assert_eq!(account["sample"], false, "the sample mark must not return");
}

#[tokio::test]
async fn removing_a_provider_is_refused_until_its_configs_are_gone() {
    let data_dir = TempDirectory::new("account-remove-data");
    let folder = TempDirectory::new("account-remove-project");
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    ensure_test_project(&app, &folder.path).await;
    let id = create_test_account(&app, "Remove me", "sk-remove-secret").await;
    let config_id = config_for_account(&app, &id).await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/providers/{id}/usage"),
        Some(json!({ "rows": [{ "label": "5-hour", "used": 20 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, project) = call(
        &app,
        "PATCH",
        "/api/projects/web",
        Some(json!({ "defaultConfigId": config_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["defaultConfigId"], config_id);

    let (status, refused) = call(&app, "DELETE", &format!("/api/providers/{id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("AI config"),
        "{refused}"
    );
    assert_eq!(
        secrets.get_key(&id).expect("read key").as_deref(),
        Some("sk-remove-secret"),
        "a refused removal must keep the key"
    );

    let (status, _) = call(&app, "DELETE", &format!("/api/configs/{config_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _removed) = call(&app, "DELETE", &format!("/api/providers/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(secrets.get_key(&id).expect("read key"), None);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        !bootstrap["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .any(|account| account["id"] == id),
        "the provider is still in bootstrap"
    );
    let web = bootstrap["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .find(|project| project["id"] == "web")
        .expect("project");
    assert!(
        web["defaultConfigId"].is_null(),
        "removing a config clears the project default"
    );
    let (_, configs) = call(&app, "GET", "/api/configs?q=", None).await;
    assert!(
        !configs
            .as_array()
            .expect("configs")
            .iter()
            .any(|config| config["providerId"] == id),
        "a config still points at the removed provider"
    );
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("sk-remove-secret"));
}

#[tokio::test]
async fn removing_a_config_used_by_an_active_session_is_refused() {
    let data_dir = TempDirectory::new("account-busy-data");
    let repo = TempDirectory::new("account-busy-repo");
    init_git_repo(&repo.path);
    let secrets = Arc::new(MemorySecretStore::default());
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_secret_store(secrets.clone());
    let app = router(state);
    ensure_test_project(&app, &repo.path).await;
    let account_id = create_test_account(&app, "Busy account", "sk-busy-secret").await;
    let config_id = config_for_account(&app, &account_id).await;
    let session_id = create_agent_session(&app, &repo.path, &account_id, "Busy session").await;

    let (status, refused) = call(&app, "DELETE", &format!("/api/configs/{config_id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Busy session"),
        "the refusal must name the session: {refused}"
    );
    assert_eq!(
        secrets.get_key(&account_id).expect("read key").as_deref(),
        Some("sk-busy-secret"),
        "a refused removal must keep the key"
    );

    let (status, _) = close_session(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "DELETE", &format!("/api/configs/{config_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _removed) = call(
        &app,
        "DELETE",
        &format!("/api/providers/{account_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(secrets.get_key(&account_id).expect("read key"), None);
}

async fn start_browser_page_server() -> String {
    async fn page_a() -> ([(header::HeaderName, &'static str); 1], &'static str) {
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<html><head><title>Page A</title></head><body><h1>Alpha text</h1></body></html>",
        )
    }
    async fn page_b() -> ([(header::HeaderName, &'static str); 1], &'static str) {
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<html><head><title>Page B</title></head><body><h1>Beta text</h1></body></html>",
        )
    }
    let app = Router::new()
        .route("/a", get(page_a))
        .route("/b", get(page_b));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind page server");
    let address = listener.local_addr().expect("page server address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}")
}

async fn start_browser_download_server(
    good_archive: Vec<u8>,
    serve_good: Arc<AtomicBool>,
) -> String {
    let good_archive = Arc::new(good_archive);
    let app = Router::new().route(
        "/chrome.zip",
        get(move || {
            let archive = Arc::clone(&good_archive);
            let serve_good = Arc::clone(&serve_good);
            async move {
                if serve_good.load(Ordering::SeqCst) {
                    (
                        [(header::CONTENT_TYPE, "application/zip")],
                        archive.as_ref().clone(),
                    )
                        .into_response()
                } else {
                    (
                        [(header::CONTENT_TYPE, "application/zip")],
                        b"a different archive".to_vec(),
                    )
                        .into_response()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind download server");
    let address = listener.local_addr().expect("download server address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}")
}

fn fake_browser_archive(platform: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    writer
        .start_file(
            format!(
                "chrome-{platform}/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
            ),
            options,
        )
        .expect("add the browser program");
    writer
        .write_all(b"#!/bin/sh\nexit 0\n")
        .expect("write the browser program");
    writer.finish().expect("finish the archive").into_inner()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn wait_for_browser_setup(app: &Router, id: &str, wanted: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (status, body) = call(app, "GET", &format!("/api/sessions/{id}/browser"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let setup = body["setup"].clone();
        if setup["phase"] == wanted {
            return setup;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "setup did not reach {wanted}: {setup}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn only_install_folders(root: &Path) -> Vec<String> {
    fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

async fn mcp_tool(app: &Router, id: &str, name: &str, arguments: Value) -> Value {
    let (status, body) = call(
        app,
        "POST",
        &format!("/internal/mcp/{id}"),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn tool_text(body: &Value) -> &str {
    body["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text")
}

#[tokio::test]
async fn mcp_browser_tools_are_scoped_to_the_calling_session() {
    let base = start_browser_page_server().await;
    let folder = TempDirectory::new("browser-scope");
    let app = router(AppState::ephemeral());
    let first = create_terminal(&app, &folder.path).await;
    let second = create_terminal(&app, &folder.path).await;
    let first_id = first["id"].as_str().expect("first id");
    let second_id = second["id"].as_str().expect("second id");

    let refused = mcp_tool(&app, second_id, "browser_read", json!({})).await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        tool_text(&refused),
        "No page is open in this session's browser."
    );

    let opened = mcp_tool(
        &app,
        first_id,
        "browser_open",
        json!({ "url": format!("{base}/a") }),
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    assert!(tool_text(&opened).contains("Page A"), "{opened}");
    assert!(tool_text(&opened).contains("Alpha text"), "{opened}");

    let read_first = mcp_tool(&app, first_id, "browser_read", json!({})).await;
    assert!(tool_text(&read_first).contains("Alpha text"));
    let read_second = mcp_tool(&app, second_id, "browser_read", json!({})).await;
    assert_eq!(read_second["result"]["isError"], true);

    let opened = mcp_tool(
        &app,
        second_id,
        "browser_open",
        json!({ "url": format!("{base}/b") }),
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    assert!(tool_text(&opened).contains("Beta text"), "{opened}");
    let read_first = mcp_tool(&app, first_id, "browser_read", json!({})).await;
    assert!(tool_text(&read_first).contains("Alpha text"));
    let read_second = mcp_tool(&app, second_id, "browser_read", json!({})).await;
    assert!(tool_text(&read_second).contains("Beta text"));

    let (status, first_state) = call(
        &app,
        "GET",
        &format!("/api/sessions/{first_id}/browser"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first_state}");
    let (status, second_state) = call(
        &app,
        "GET",
        &format!("/api/sessions/{second_id}/browser"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second_state}");
    assert_eq!(first_state["tabs"].as_array().expect("first tabs").len(), 1);
    assert_eq!(
        second_state["tabs"].as_array().expect("second tabs").len(),
        1
    );
    assert_eq!(first_state["tabs"][0]["title"], "Page A");
    assert_eq!(second_state["tabs"][0]["title"], "Page B");
    let first_tab = first_state["tabs"][0]["id"].as_str().expect("first tab id");
    assert_ne!(first_tab, second_state["tabs"][0]["id"]);

    let (status, refused) = call(
        &app,
        "POST",
        &format!("/api/sessions/{second_id}/browser/tabs/{first_tab}/activate"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let (status, closed) = close_session(&app, first_id).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    let refused = mcp_tool(
        &app,
        first_id,
        "browser_open",
        json!({ "url": format!("{base}/a") }),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(tool_text(&refused), "The session is closed.");
    let (status, closed_state) = call(
        &app,
        "GET",
        &format!("/api/sessions/{first_id}/browser"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed_state}");
    assert_eq!(closed_state["started"], false);
    assert!(
        closed_state["tabs"]
            .as_array()
            .expect("closed tabs")
            .is_empty()
    );
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/open"),
        Some(json!({ "url": format!("{base}/a") })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, _) = close_session(&app, second_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn browser_live_input_and_logs_stay_inside_one_session() {
    let base = start_browser_page_server().await;
    let folder = TempDirectory::new("browser-live-input");
    let app = router(AppState::ephemeral());
    let first = create_terminal(&app, &folder.path).await;
    let second = create_terminal(&app, &folder.path).await;
    let first_id = first["id"].as_str().expect("first id");
    let second_id = second["id"].as_str().expect("second id");

    for (id, page) in [(first_id, "a"), (second_id, "b")] {
        let opened = mcp_tool(
            &app,
            id,
            "browser_open",
            json!({ "url": format!("{base}/{page}") }),
        )
        .await;
        assert_eq!(opened["result"]["isError"], false, "{opened}");
    }

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/click"),
        Some(json!({ "x": 0.5, "y": 0.25 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first_console = mcp_tool(&app, first_id, "browser_console", json!({})).await;
    assert_eq!(first_console["result"]["isError"], false, "{first_console}");
    assert!(tool_text(&first_console).contains("Live click at 0.500, 0.250"));
    let second_console = mcp_tool(&app, second_id, "browser_console", json!({})).await;
    assert!(
        !tool_text(&second_console).contains("Live click"),
        "{second_console}"
    );

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{second_id}/browser/input/type"),
        Some(json!({ "text": "hello there" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let read_second = mcp_tool(&app, second_id, "browser_read", json!({})).await;
    assert!(
        tool_text(&read_second).contains("hello there"),
        "{read_second}"
    );
    let read_first = mcp_tool(&app, first_id, "browser_read", json!({})).await;
    assert!(
        !tool_text(&read_first).contains("hello there"),
        "{read_first}"
    );
    let second_console = mcp_tool(&app, second_id, "browser_console", json!({})).await;
    assert!(tool_text(&second_console).contains("Live text was typed"));
    let first_console = mcp_tool(&app, first_id, "browser_console", json!({})).await;
    assert!(!tool_text(&first_console).contains("Live text was typed"));

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/key"),
        Some(json!({ "key": "Enter" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first_console = mcp_tool(&app, first_id, "browser_console", json!({})).await;
    assert!(tool_text(&first_console).contains("Live key Enter"));

    // Each modifier bit reaches the session's own page with the key.
    for modifiers in [1, 2, 4, 8] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{first_id}/browser/input/key"),
            Some(json!({ "key": "a", "modifiers": modifiers })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let console = mcp_tool(&app, first_id, "browser_console", json!({})).await;
        assert!(
            tool_text(&console).contains(&format!("Live key a with modifiers {modifiers}")),
            "{console}"
        );
    }
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/key"),
        Some(json!({ "key": "7", "modifiers": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first_console = mcp_tool(&app, first_id, "browser_console", json!({})).await;
    assert!(tool_text(&first_console).contains("Live key 7 with modifiers 3"));
    let second_console = mcp_tool(&app, second_id, "browser_console", json!({})).await;
    assert!(
        !tool_text(&second_console).contains("Live key a"),
        "{second_console}"
    );
    assert!(
        !tool_text(&second_console).contains("Live key 7"),
        "{second_console}"
    );

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/click"),
        Some(json!({ "x": 1.5, "y": 0.5 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    for key in ["F13", "!"] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/sessions/{first_id}/browser/input/key"),
            Some(json!({ "key": key })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key}: {body}");
    }

    let (status, _) = close_session(&app, first_id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/click"),
        Some(json!({ "x": 0.5, "y": 0.5 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/browser/input/key"),
        Some(json!({ "key": "a", "modifiers": 2 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let refused = mcp_tool(&app, first_id, "browser_console", json!({})).await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(tool_text(&refused), "The session is closed.");
    let refused = mcp_tool(&app, first_id, "browser_network", json!({ "limit": 5 })).await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(tool_text(&refused), "The session is closed.");

    let (status, _) = close_session(&app, second_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn browser_logs_redact_secrets_and_return_short_lists() {
    const SECRET: &str = "sk-browser-secret-abc123";
    let base = start_browser_page_server().await;
    let folder = TempDirectory::new("browser-logs");
    let app = router(AppState::ephemeral());
    let (status, account) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Browser secret",
            "vendor": "custom",
            "auth": "key",
            "apiStyle": "openai",
            "key": SECRET,
            "server": "https://gateway.example/v1"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id");

    let opened = mcp_tool(
        &app,
        id,
        "browser_open",
        json!({ "url": format!("{base}/a?q={SECRET}&token=visible-secret") }),
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");

    let network = mcp_tool(&app, id, "browser_network", json!({})).await;
    assert_eq!(network["result"]["isError"], false, "{network}");
    let text = tool_text(&network);
    assert!(
        !text.contains(SECRET),
        "the network log leaked a key: {text}"
    );
    assert!(
        !text.contains("visible-secret"),
        "the network log leaked a token: {text}"
    );
    assert!(
        text.contains("q=[redacted]") || text.contains("[redacted]"),
        "{text}"
    );
    assert!(text.contains("token=[redacted]"), "{text}");
    assert!(text.contains("returned 200"), "{text}");

    let console = mcp_tool(&app, id, "browser_console", json!({})).await;
    assert_eq!(console["result"]["isError"], false, "{console}");
    assert!(tool_text(&console).contains("Loaded Page A"), "{console}");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/browser/open"),
        Some(json!({ "url": format!("{base}/b") })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let short = mcp_tool(&app, id, "browser_network", json!({ "limit": 1 })).await;
    let text = tool_text(&short);
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("/b"), "{text}");
    assert!(!text.contains("/a?"), "{text}");

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn demo_servers_register_list_and_stop() {
    let data_dir = TempDirectory::new("demo-data");
    let repo = TempDirectory::new("demo-repo");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state);
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let name = session["name"].as_str().expect("session name").to_owned();

    let (failed, text) = mcp_text(
        &app,
        &id,
        "register_demo_server",
        json!({ "name": "Preview", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{text}");
    assert!(text.contains("http://localhost:3000"), "{text}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    let servers = demo_servers(listed);
    assert_eq!(servers.len(), 1, "{listed}");
    assert_eq!(servers[0]["name"], "Preview");
    assert_eq!(servers[0]["port"], 3000);
    assert_eq!(servers[0]["address"], "http://localhost:3000");
    assert_eq!(servers[0]["state"], "active");
    let server_id = servers[0]["id"].as_str().expect("server id").to_owned();

    let (status, all) = call(&app, "GET", "/api/demo-servers", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all[0]["sessionId"], id);
    assert_eq!(all[0]["sessionName"], name);
    assert_eq!(all[0]["port"], 3000);

    let (status, updated) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/demo-servers/{server_id}/stop"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(demo_servers(&updated)[0]["state"], "stopped");

    let (_, all) = call(&app, "GET", "/api/demo-servers", None).await;
    assert!(all.as_array().expect("demo servers").is_empty());

    let (status, unknown) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/demo-servers/unknown/stop"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{unknown}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[cfg(unix)]
fn write_fake_tailscale(folder: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let binary = folder.join("tailscale");
    fs::write(&binary, format!("#!/bin/sh\n{body}\n")).expect("write fake tailscale");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .expect("make fake tailscale executable");
    binary
}

#[cfg(unix)]
#[tokio::test]
async fn demo_servers_register_with_the_tailnet_address() {
    let data_dir = TempDirectory::new("demo-tailnet-data");
    let repo = TempDirectory::new("demo-tailnet-repo");
    let binary = write_fake_tailscale(
        &data_dir.path,
        "printf '{\"BackendState\":\"Running\",\"Self\":{\"DNSName\":\"studio.tailnet.ts.net.\",\"TailscaleIPs\":[\"100.101.102.103\"]}}'\n",
    );
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_tailscale_binary(binary);
    let app = router(state);
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(
        &app,
        &id,
        "register_demo_server",
        json!({ "name": "Preview", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{text}");
    let tailnet = "https://studio.tailnet.ts.net:3000";
    assert!(text.contains("http://localhost:3000"), "{text}");
    assert!(text.contains(tailnet), "{text}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    let servers = demo_servers(listed);
    assert_eq!(servers[0]["address"], "http://localhost:3000");
    assert_eq!(servers[0]["tailscaleAddress"], tailnet);

    let (status, all) = call(&app, "GET", "/api/demo-servers", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all[0]["tailscaleAddress"], tailnet);

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[cfg(unix)]
async fn assert_demo_registers_without_tailscale(label: &str, binary: PathBuf) {
    let data_dir = TempDirectory::new(&format!("demo-{label}-data"));
    let repo = TempDirectory::new(&format!("demo-{label}-repo"));
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_tailscale_binary(binary);
    let app = router(state);
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(
        &app,
        &id,
        "register_demo_server",
        json!({ "name": "Preview", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{label}: {text}");
    assert!(text.contains("http://localhost:3000"), "{label}: {text}");
    assert!(!text.contains("Tailscale"), "{label}: {text}");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    let server = &demo_servers(listed)[0];
    assert_eq!(server["address"], "http://localhost:3000");
    assert!(
        server.get("tailscaleAddress").is_none(),
        "{label}: {server}"
    );

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
}

#[cfg(unix)]
#[tokio::test]
async fn demo_servers_register_when_tailscale_is_missing_or_fails() {
    assert_demo_registers_without_tailscale(
        "missing",
        PathBuf::from("/nonexistent/elesdee-test-tailscale"),
    )
    .await;

    let folder = TempDirectory::new("demo-tailscale-failing");
    let binary = write_fake_tailscale(&folder.path, "exit 1");
    assert_demo_registers_without_tailscale("failing", binary).await;
}

#[tokio::test]
async fn mcp_browser_refuses_a_non_http_address_and_shares_a_screenshot() {
    let base = start_browser_page_server().await;
    let data_dir = TempDirectory::new("browser-share-data");
    let folder = TempDirectory::new("browser-share-folder");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id");

    let refused = mcp_tool(
        &app,
        id,
        "browser_open",
        json!({ "url": "file:///etc/passwd" }),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        tool_text(&refused),
        "Only http and https addresses can be opened."
    );

    let opened = mcp_tool(
        &app,
        id,
        "browser_open",
        json!({ "url": format!("{base}/a") }),
    )
    .await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");

    let screenshot = mcp_tool(
        &app,
        id,
        "browser_screenshot",
        json!({ "caption": "The first page" }),
    )
    .await;
    assert_eq!(screenshot["result"]["isError"], false, "{screenshot}");
    let file: Value =
        serde_json::from_str(tool_text(&screenshot)).expect("shared screenshot record");
    assert_eq!(file["caption"], "The first page");
    assert_eq!(file["image"], true);
    assert_eq!(file["contentType"], "image/png");
    assert!(file["name"].as_str().unwrap_or_default().ends_with(".png"));
    let file_id = file["id"].as_str().expect("screenshot id");

    let (status, files) = call(&app, "GET", &format!("/api/sessions/{id}/shared"), None).await;
    assert_eq!(status, StatusCode::OK, "{files}");
    assert_eq!(files.as_array().expect("shared files").len(), 1);

    let (status, headers, body) =
        get_raw(&app, &format!("/api/sessions/{id}/shared/{file_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CONTENT_TYPE).expect("content type"),
        "image/png"
    );
    assert!(body.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn browser_setup_verifies_the_download_and_retries_after_a_failure() {
    let folder = TempDirectory::new("browser-install-folder");
    let data_dir = TempDirectory::new("browser-install-data");
    let platform = if cfg!(target_arch = "aarch64") {
        "mac-arm64"
    } else {
        "mac-x64"
    };
    let version = "0.0.1-test";
    let archive = fake_browser_archive(platform);
    let serve_good = Arc::new(AtomicBool::new(false));
    let base = start_browser_download_server(archive.clone(), Arc::clone(&serve_good)).await;
    let root = data_dir.path.join("chromium");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    )
    .with_test_browser_install(TestBrowserInstall {
        root: root.clone(),
        url: format!("{base}/chrome.zip"),
        version: version.to_owned(),
        platform: platform.to_owned(),
        sha256: sha256_hex(&archive),
    });
    let app = router(state);
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/browser/setup"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let setup = wait_for_browser_setup(&app, id, "failed").await;
    assert_eq!(setup["phase"], "failed");
    assert!(
        setup["error"]
            .as_str()
            .unwrap_or_default()
            .contains("checksum"),
        "{setup}"
    );
    assert!(
        !root.join(version).exists(),
        "a failed download left a folder"
    );
    assert!(
        only_install_folders(&root)
            .iter()
            .all(|name| !name.starts_with(".install-")),
        "a failed download left staging files"
    );

    // The session is still usable while the browser is missing.
    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{bootstrap}");
    assert!(
        bootstrap["sessions"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == id)),
        "{bootstrap}"
    );

    serve_good.store(true, Ordering::SeqCst);
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/browser/setup"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let setup = wait_for_browser_setup(&app, id, "ready").await;
    assert_eq!(setup["source"], "download", "{setup}");
    assert_eq!(setup["version"], version, "{setup}");
    let executable = root.join(version).join(format!(
        "chrome-{platform}/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
    ));
    assert!(executable.is_file(), "the browser program is missing");
    assert_eq!(
        fs::read_to_string(root.join("version")).expect("the version marker"),
        version
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&executable)
            .expect("the browser program")
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "the browser program is not executable");
    }

    // Deleting the private folder makes the next use start the download again.
    fs::remove_dir_all(&root).expect("remove the browser folder");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/browser/setup"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_for_browser_setup(&app, id, "ready").await;
    assert!(executable.is_file(), "the browser was not installed again");
}

#[tokio::test]
async fn demo_servers_enforce_their_limits_and_session_ownership() {
    let data_dir = TempDirectory::new("demo-limits-data");
    let repo = TempDirectory::new("demo-limits-repo");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state);
    let first = create_terminal(&app, &repo.path).await;
    let first_id = first["id"].as_str().expect("session id").to_owned();
    let second = create_terminal(&app, &repo.path).await;
    let second_id = second["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "   ", "port": 3000 }),
    )
    .await;
    assert!(failed, "{text}");

    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "Privileged", "port": 80 }),
    )
    .await;
    assert!(failed, "{text}");
    assert!(text.contains("1024"), "{text}");

    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "Service", "port": 4319 }),
    )
    .await;
    assert!(failed, "{text}");
    assert!(text.contains("elesdee"), "{text}");

    for port in 3000..3008 {
        let (failed, text) = mcp_text(
            &app,
            &first_id,
            "register_demo_server",
            json!({ "name": format!("Preview {port}"), "port": port }),
        )
        .await;
        assert!(!failed, "{text}");
    }
    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "One too many", "port": 3008 }),
    )
    .await;
    assert!(failed, "{text}");
    assert!(text.contains('8'), "{text}");

    let (failed, text) = mcp_text(
        &app,
        &second_id,
        "register_demo_server",
        json!({ "name": "Shared", "port": 3000 }),
    )
    .await;
    assert!(failed, "{text}");
    assert!(text.contains("Another session"), "{text}");

    // A repeated port in the same session updates its name and stays one entry.
    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "Renamed preview", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{text}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == first_id)
        .expect("session");
    let servers = demo_servers(listed);
    assert_eq!(servers.len(), 8);
    assert!(
        servers
            .iter()
            .any(|server| server["name"] == "Renamed preview" && server["port"] == 3000)
    );

    // A stopped record frees a slot for a new registration.
    let stopped_id = servers
        .iter()
        .find(|server| server["port"] == 3001)
        .and_then(|server| server["id"].as_str())
        .expect("server to stop")
        .to_owned();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/sessions/{first_id}/demo-servers/{stopped_id}/stop"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "Replacement", "port": 3008 }),
    )
    .await;
    assert!(!failed, "{text}");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == first_id)
        .expect("session");
    let servers = demo_servers(listed);
    assert_eq!(servers.len(), 8);
    assert!(servers.iter().all(|server| server["port"] != 3001));
    assert!(
        servers
            .iter()
            .any(|server| server["name"] == "Replacement" && server["port"] == 3008)
    );

    let (status, _) = close_session(&app, &first_id).await;
    assert_eq!(status, StatusCode::OK);
    let (failed, text) = mcp_text(
        &app,
        &first_id,
        "register_demo_server",
        json!({ "name": "After close", "port": 3100 }),
    )
    .await;
    assert!(failed, "{text}");
    assert!(text.contains("closed"), "{text}");
}

#[tokio::test]
async fn closing_or_pausing_a_session_stops_its_demo_servers() {
    let data_dir = TempDirectory::new("demo-stop-data");
    let repo = TempDirectory::new("demo-stop-repo");
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    );
    let app = router(state.clone());
    let session = create_terminal(&app, &repo.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (failed, text) = mcp_text(
        &app,
        &id,
        "register_demo_server",
        json!({ "name": "Preview", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{text}");

    let entry = state.session(&id).expect("session entry");
    entry.pause("Paused. Test.");
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    assert_eq!(demo_servers(listed)[0]["state"], "stopped");
    assert!(state.demo_server_by_port(3000).is_none());

    // The same port can be registered again after the pause.
    let (failed, text) = mcp_text(
        &app,
        &id,
        "register_demo_server",
        json!({ "name": "Preview again", "port": 3000 }),
    )
    .await;
    assert!(!failed, "{text}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == id)
        .expect("session");
    assert_eq!(demo_servers(listed)[0]["state"], "stopped");
}

async fn create_agent_with_config(app: &Router, repo: &Path, config_id: &str, name: &str) -> Value {
    ensure_test_project(app, repo).await;
    let (status, session) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": config_id,
            "name": name,
            "folder": repo.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create agent: {session}");
    session
}

#[tokio::test]
async fn one_provider_serves_two_configs_on_two_clients() {
    let data_dir = TempDirectory::new("reuse-data");
    let repo = TempDirectory::new("reuse-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "printf 'A=%s O=%s\\n' \"$ANTHROPIC_BASE_URL\" \"$OPENAI_BASE_URL\"; sleep 30"
                .to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Shared DeepSeek",
            "vendor": "deepseek",
            "auth": "key",
            "key": "sk-shared-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    let provider_id = provider["id"].as_str().expect("provider id").to_owned();

    let (status, claude) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Claude Code",
            "model": "deepseek-chat"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{claude}");
    let (status, codex) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Codex",
            "model": "deepseek-chat"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{codex}");
    assert_eq!(claude["providerId"], provider_id);
    assert_eq!(codex["providerId"], provider_id);
    assert_eq!(claude["client"], "Claude Code");
    assert_eq!(codex["client"], "Codex");

    let claude_id = claude["id"].as_str().expect("claude config id");
    let codex_id = codex["id"].as_str().expect("codex config id");
    let first = create_agent_with_config(&app, &repo.path, claude_id, "Claude reuse").await;
    let first_id = first["id"].as_str().expect("session id");
    assert_eq!(first["providerId"], provider_id);
    let request = request_builder("GET", &format!("/api/sessions/{first_id}/output?after=0"))
        .body(Body::empty())
        .expect("output request");
    let response = app.clone().oneshot(request).await.expect("output");
    let text = read_sse_until(response.into_body(), |text| {
        text.contains("A=https://api.deepseek.com/anthropic")
    })
    .await;
    assert!(sse_data_text(&text).contains("A=https://api.deepseek.com/anthropic"));
    assert!(!sse_data_text(&text).contains("sk-shared-secret"));

    let second = create_agent_with_config(&app, &repo.path, codex_id, "Codex reuse").await;
    let second_id = second["id"].as_str().expect("session id");
    assert_eq!(second["providerId"], provider_id);
    let request = request_builder("GET", &format!("/api/sessions/{second_id}/output?after=0"))
        .body(Body::empty())
        .expect("output request");
    let response = app.clone().oneshot(request).await.expect("output");
    let text = read_sse_until(response.into_body(), |text| {
        text.contains("O=https://api.deepseek.com")
    })
    .await;
    assert!(sse_data_text(&text).contains("O=https://api.deepseek.com"));

    let (status, _) = close_session(&app, first_id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = close_session(&app, second_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn model_effort_and_extra_arguments_reach_each_client() {
    let data_dir = TempDirectory::new("launch-flags-data");
    let repo = TempDirectory::new("launch-flags-repo");
    init_git_repo(&repo.path);
    let args_file = data_dir.path.join("args.txt");
    let script = format!(
        "printf '%s\\n' \"$@\" > {}; sleep 30",
        args_file.to_string_lossy()
    );
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            script,
            "stub".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;

    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Flags",
            "vendor": "deepseek",
            "auth": "key",
            "key": "sk-flags"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, config) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Codex",
            "model": "gpt-5.2",
            "effort": "high",
            "args": ["--foo", "bar"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    let config_id = config["id"].as_str().expect("config id");
    let session = create_agent_with_config(&app, &repo.path, config_id, "Codex flags").await;
    let id = session["id"].as_str().expect("session id");
    wait_for_args_file(&args_file).await;
    let arguments = fs::read_to_string(&args_file).expect("read arguments");
    let lines = arguments.lines().collect::<Vec<_>>();
    assert!(
        arguments.contains("model_reasoning_effort=\"high\""),
        "effort missing: {arguments}"
    );
    assert!(
        arguments.contains("model=\"gpt-5.2\""),
        "model missing: {arguments}"
    );
    assert_eq!(&lines[lines.len() - 2..], ["--foo", "bar"]);
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
    let _ = fs::remove_file(&args_file);

    let (status, claude) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "Claude Code",
            "model": "claude-sonnet-4-5",
            "effort": "max",
            "args": ["--verbose"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{claude}");
    let claude_id = claude["id"].as_str().expect("claude config id");
    let session = create_agent_with_config(&app, &repo.path, claude_id, "Claude flags").await;
    let id = session["id"].as_str().expect("session id");
    wait_for_args_file(&args_file).await;
    let arguments = fs::read_to_string(&args_file).expect("read arguments");
    assert!(
        arguments.contains("--model\nclaude-sonnet-4-5"),
        "{arguments}"
    );
    assert!(arguments.contains("--effort\nmax"), "{arguments}");
    assert!(arguments.trim_end().ends_with("--verbose"), "{arguments}");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
    let _ = fs::remove_file(&args_file);

    let (status, opencode) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": provider_id,
            "client": "OpenCode 2",
            "model": "deepseek-chat",
            "effort": "high"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opencode}");
    let opencode_id = opencode["id"].as_str().expect("opencode config id");
    let session = create_agent_with_config(&app, &repo.path, opencode_id, "OpenCode flags").await;
    let id = session["id"].as_str().expect("session id");
    let path = data_dir
        .path
        .join("mcp")
        .join(format!("opencode-{id}.json"));
    let config: Value =
        serde_json::from_slice(&fs::read(&path).expect("read opencode config")).expect("json");
    assert_eq!(config["model"], "deepseek-chat");
    assert_eq!(config["reasoningEffort"], "high");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);

    let (status, google) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Google flags",
            "vendor": "google",
            "auth": "key",
            "key": "sk-google-flags"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{google}");
    let google_id = google["id"].as_str().expect("google provider id");
    let (status, antigravity) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({
            "providerId": google_id,
            "client": "Antigravity",
            "model": "gemini-3.8-flash-medium",
            "effort": "high"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{antigravity}");
    let antigravity_id = antigravity["id"].as_str().expect("antigravity config id");
    let session =
        create_agent_with_config(&app, &repo.path, antigravity_id, "Antigravity flags").await;
    let id = session["id"].as_str().expect("session id");
    let path = data_dir
        .path
        .join("homes")
        .join(id)
        .join(".gemini")
        .join("antigravity-cli")
        .join("settings.json");
    let settings: Value =
        serde_json::from_slice(&fs::read(&path).expect("read antigravity settings")).expect("json");
    assert_eq!(settings["model"], "gemini-3.8-flash-medium");
    assert_eq!(settings["effort"], "high");
    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[derive(Clone)]
struct ModelsServer {
    calls: Arc<AtomicUsize>,
    status: StatusCode,
    body: Value,
}

async fn models_handler(AxumState(server): AxumState<ModelsServer>) -> Response {
    server.calls.fetch_add(1, Ordering::SeqCst);
    (server.status, axum::Json(server.body.clone())).into_response()
}

async fn start_models_server(status: StatusCode, body: Value) -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = ModelsServer {
        calls: Arc::clone(&calls),
        status,
        body,
    };
    let app = Router::new()
        .route("/v1/models", get(models_handler))
        .route("/models", get(models_handler))
        .with_state(server);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind models server");
    let address = listener.local_addr().expect("models address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}/v1"), calls)
}

#[tokio::test]
async fn provider_models_are_read_cached_coalesced_and_fall_back() {
    let (server, calls) = start_models_server(
        StatusCode::OK,
        json!({
            "data": [
                { "id": "gateway-large", "display_name": "Gateway Large" },
                { "id": "gateway-small" }
            ]
        }),
    )
    .await;
    let app = router(AppState::ephemeral());
    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Gateway",
            "vendor": "custom",
            "auth": "key",
            "apiStyle": "openai",
            "server": server,
            "key": "sk-model-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    let provider_id = provider["id"].as_str().expect("provider id").to_owned();

    let first = tokio::spawn({
        let app = app.clone();
        let provider_id = provider_id.clone();
        async move {
            call(
                &app,
                "GET",
                &format!("/api/providers/{provider_id}/models?client=Codex"),
                None,
            )
            .await
        }
    });
    let second = tokio::spawn({
        let app = app.clone();
        let provider_id = provider_id.clone();
        async move {
            call(
                &app,
                "GET",
                &format!("/api/providers/{provider_id}/models?client=Codex"),
                None,
            )
            .await
        }
    });
    let (first, second) = tokio::join!(first, second);
    let (status, first) = first.expect("first call");
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = second.expect("second call");
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the reads were not coalesced"
    );
    let models = first["models"].as_array().expect("models");
    assert!(models.iter().any(|model| model["id"] == "gateway-large"));
    assert_eq!(first["source"], "provider");
    assert_eq!(first["fallback"], false);
    assert!(!first.to_string().contains("sk-model-secret"));

    let (status, cached) = call(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/models?client=Codex"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cached}");
    assert_eq!(cached["source"], "cache");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the cache missed");

    let closed = {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        drop(listener);
        format!("http://{address}/v1")
    };
    let (status, fallback_provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Broken Gateway",
            "vendor": "deepseek",
            "auth": "key",
            "server": closed,
            "key": "sk-broken-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{fallback_provider}");
    let fallback_id = fallback_provider["id"].as_str().expect("id");
    let (status, fallback) = call(
        &app,
        "GET",
        &format!("/api/providers/{fallback_id}/models?client=Codex"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{fallback}");
    assert_eq!(fallback["fallback"], true);
    assert_eq!(fallback["source"], "known");
    assert!(
        fallback["models"]
            .as_array()
            .expect("models")
            .iter()
            .any(|model| model["id"] == "deepseek-chat")
    );
    assert!(!fallback.to_string().contains("sk-broken-secret"));
}

#[tokio::test]
async fn a_state_with_accounts_and_presets_migrates_to_providers_and_configs() {
    let data_dir = TempDirectory::new("migrate-state");
    let state = json!({
        "version": 1,
        "projects": [],
        "accounts": [
            {
                "id": "max",
                "name": "Max plan",
                "plan": "subscription",
                "client": "Claude Code",
                "provider": "anthropic",
                "auth": "subscription",
                "sample": true,
                "rows": [],
                "foot": "No official usage source.",
                "server": null,
                "model": null
            },
            {
                "id": "codex",
                "name": "Codex plan",
                "plan": "subscription",
                "client": "Codex",
                "provider": "openai",
                "auth": "subscription",
                "sample": true,
                "rows": [],
                "foot": "No official usage source.",
                "server": null,
                "model": null
            }
        ],
        "presets": [{
            "id": "fast",
            "name": "Fast",
            "accountId": "max",
            "model": "claude-sonnet-4-5",
            "env": [],
            "config": {},
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        }],
        "sessions": [
            {
                "session": {
                    "id": "s1", "name": "From account", "short": "s1",
                    "projectId": "web", "kind": "agent", "status": "closed",
                    "workState": null, "client": "Claude Code",
                    "accountId": "max", "allowedAccountIds": ["max"],
                    "branch": null, "folder": "/tmp", "preview": "Closed.",
                    "waitingFor": null, "startedAt": "2026-01-01T00:00:00Z",
                    "shortcuts": []
                },
                "cols": 80, "rows": 24, "output": [], "nextSeq": 1
            },
            {
                "session": {
                    "id": "s2", "name": "From preset", "short": "s2",
                    "projectId": "web", "kind": "agent", "status": "closed",
                    "workState": null, "client": "Claude Code",
                    "accountId": "max", "presetId": "fast",
                    "allowedAccountIds": ["max", "codex"],
                    "branch": null, "folder": "/tmp", "preview": "Closed.",
                    "waitingFor": null, "startedAt": "2026-01-01T00:00:00Z",
                    "shortcuts": []
                },
                "cols": 80, "rows": 24, "output": [], "nextSeq": 1
            },
            {
                "session": {
                    "id": "s3", "name": "Missing account", "short": "s3",
                    "projectId": "web", "kind": "agent", "status": "closed",
                    "workState": null, "client": "Claude Code",
                    "accountId": "ghost", "branch": null, "folder": "/tmp",
                    "preview": "Closed.", "waitingFor": null,
                    "startedAt": "2026-01-01T00:00:00Z", "shortcuts": []
                },
                "cols": 80, "rows": 24, "output": [], "nextSeq": 1
            }
        ]
    });
    fs::write(
        data_dir.path.join("state.json"),
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write state");

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK, "{bootstrap}");
    let providers = bootstrap["providers"].as_array().expect("providers");
    assert_eq!(providers.len(), 2);
    assert_eq!(
        providers[0]["supportedClients"],
        json!(["Claude Code", "OpenCode 2"])
    );
    let configs = bootstrap["configs"].as_array().expect("configs");
    assert_eq!(configs.len(), 3);
    assert!(
        configs
            .iter()
            .any(|config| config["id"] == "max" && config["name"] == "Max plan (Claude Code)")
    );
    let preset = configs
        .iter()
        .find(|config| config["id"] == "fast")
        .expect("preset config");
    assert_eq!(preset["name"], "Fast");
    assert_eq!(preset["providerId"], "max");
    assert_eq!(preset["model"], "claude-sonnet-4-5");

    let sessions = bootstrap["sessions"].as_array().expect("sessions");
    let from_account = sessions
        .iter()
        .find(|session| session["id"] == "s1")
        .expect("s1");
    assert_eq!(from_account["configId"], "max");
    assert_eq!(from_account["providerId"], "max");
    let from_preset = sessions
        .iter()
        .find(|session| session["id"] == "s2")
        .expect("s2");
    assert_eq!(from_preset["configId"], "fast");
    assert_eq!(
        from_preset["allowedConfigIds"],
        json!(["max", "codex", "fast"])
    );
    let missing = sessions
        .iter()
        .find(|session| session["id"] == "s3")
        .expect("s3");
    assert_eq!(missing["configId"], Value::Null);
    assert!(
        missing["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("Migration"))
    );

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    let reloaded: Value = serde_json::from_str(&state_text).expect("state json");
    assert_eq!(reloaded["version"], 2);
    assert!(reloaded.get("accounts").is_none());
    assert!(reloaded.get("presets").is_none());
}

const SIGN_IN_STUB: &str = r#"#!/bin/sh
case "$1 $2" in
  "auth status"|"login status")
    case "${FAKE_STATUS:-subscription}" in
      subscription) printf '{"loggedIn":true,"authMethod":"claude.ai"}'; exit 0 ;;
      oauth) printf '{"loggedIn":true,"authMethod":"oauth_token"}'; exit 0 ;;
      api_key) printf '{"loggedIn":true,"authMethod":"api_key"}'; exit 0 ;;
      slow) sleep 60; printf '{"loggedIn":true,"authMethod":"claude.ai"}'; exit 0 ;;
      *) printf '{"loggedIn":false,"authMethod":"none"}'; exit 1 ;;
    esac
    ;;
esac
printf 'Open the sign-in page: https://example.com/signin?state=test\n'
case "${FAKE_SIGNIN:-success}" in
  success) exit 0 ;;
  refuse) exit 1 ;;
  code)
    printf 'Enter this code: ABCD-EFGH\n'
    IFS= read -r line
    [ "$line" = "ABCD-EFGH" ] && exit 0
    exit 1
    ;;
  wait) sleep 60; exit 0 ;;
esac
exit 1
"#;

fn write_sign_in_stub(directory: &Path) -> PathBuf {
    let path = directory.join("fake-sign-in.sh");
    fs::write(&path, SIGN_IN_STUB).expect("write sign-in stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make the sign-in stub executable");
    }
    path
}

fn sign_in_app(
    directory: &Path,
    stub: &Path,
    env: &[(&str, &str)],
    timeout: Option<Duration>,
) -> Router {
    let state = AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        directory.to_path_buf(),
    )
    .with_test_sign_in(SignInOverrides {
        binary: Some(stub.to_path_buf()),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        timeout,
    });
    router(state)
}

async fn wait_for_sign_in(app: &Router, id: &str, expected: &[&str]) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (status, view) = call(app, "GET", &format!("/api/sign-in/{id}"), None).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        let state = view["state"].as_str().unwrap_or_default();
        if expected.contains(&state) {
            return view;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the sign-in did not reach {expected:?}: {view}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

async fn connected_provider_count(app: &Router, vendor: &str) -> usize {
    let (_, providers) = call(app, "GET", "/api/providers?q=", None).await;
    providers
        .as_array()
        .expect("providers")
        .iter()
        .filter(|provider| {
            provider["vendor"] == vendor
                && provider["sample"] == false
                && provider["connected"] == true
        })
        .count()
}

#[tokio::test]
async fn subscription_sign_in_connects_a_claude_provider() {
    let data_dir = TempDirectory::new("signin-claude-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "success")], None);

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["client"], "Claude Code");
    let id = started["id"].as_str().expect("sign-in id").to_owned();

    let view = wait_for_sign_in(&app, &id, &["success"]).await;
    assert_eq!(
        view["url"].as_str(),
        Some("https://example.com/signin?state=test")
    );
    let provider = view["provider"].as_object().expect("provider");
    assert_eq!(provider["vendor"], "anthropic");
    assert_eq!(provider["auth"], "subscription");
    assert_eq!(provider["connected"], true);
    assert_eq!(provider["sample"], false);
    assert_eq!(provider["name"], "Anthropic");
    assert!(
        provider["supportedClients"]
            .as_array()
            .is_some_and(|clients| !clients.is_empty())
    );
    let provider_id = provider["id"].as_str().expect("provider id");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == provider_id)
        .expect("listed provider");
    assert_eq!(listed["connected"], true);
    assert_eq!(connected_provider_count(&app, "anthropic").await, 1);

    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("state=test"));
    assert!(!state_text.contains("ABCD-EFGH"));
}

#[tokio::test]
async fn subscription_sign_in_refusal_leaves_no_connected_provider() {
    let data_dir = TempDirectory::new("signin-refuse-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "refuse")], None);

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic", "name": "Refused" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();

    let view = wait_for_sign_in(&app, &id, &["failed"]).await;
    assert_eq!(view["reason"], "clientError");
    assert!(view["provider"].is_null());
    assert_eq!(connected_provider_count(&app, "anthropic").await, 0);
}

#[tokio::test]
async fn subscription_sign_in_times_out_with_a_clear_reason() {
    let data_dir = TempDirectory::new("signin-timeout-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(
        &data_dir.path,
        &stub,
        &[("FAKE_SIGNIN", "wait")],
        Some(Duration::from_secs(1)),
    );

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "openai" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();

    let view = wait_for_sign_in(&app, &id, &["failed"]).await;
    assert_eq!(view["reason"], "timeout");
    assert!(view["provider"].is_null());
    assert_eq!(connected_provider_count(&app, "openai").await, 0);
}

#[tokio::test]
async fn subscription_sign_in_can_be_abandoned() {
    let data_dir = TempDirectory::new("signin-cancel-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "wait")], None);

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();
    wait_for_sign_in(&app, &id, &["waiting"]).await;

    let (status, _) = call(&app, "DELETE", &format!("/api/sign-in/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, view) = call(&app, "GET", &format!("/api/sign-in/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], "cancelled");
    assert!(view["provider"].is_null());
    assert_eq!(connected_provider_count(&app, "anthropic").await, 0);
}

#[tokio::test]
async fn a_one_time_code_completes_the_codex_device_sign_in() {
    let data_dir = TempDirectory::new("signin-code-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "code")], None);

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "openai", "mode": "code" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["client"], "Codex");
    assert_eq!(started["mode"], "code");
    let id = started["id"].as_str().expect("sign-in id").to_owned();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let waiting = loop {
        let (_, view) = call(&app, "GET", &format!("/api/sign-in/{id}"), None).await;
        if view["state"] == "waiting" && view["code"].is_string() {
            break view;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the device code was not shown: {view}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    };
    assert_eq!(waiting["code"], "ABCD-EFGH");

    let (status, submitted) = call(
        &app,
        "POST",
        &format!("/api/sign-in/{id}/code"),
        Some(json!({ "code": "ABCD-EFGH" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted}");

    let view = wait_for_sign_in(&app, &id, &["success"]).await;
    assert_eq!(view["provider"]["vendor"], "openai");
    assert_eq!(view["provider"]["connected"], true);
    assert_eq!(connected_provider_count(&app, "openai").await, 1);
}

#[tokio::test]
async fn a_key_login_is_not_accepted_as_a_subscription() {
    let data_dir = TempDirectory::new("signin-key-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(
        &data_dir.path,
        &stub,
        &[("FAKE_SIGNIN", "success"), ("FAKE_STATUS", "api_key")],
        None,
    );

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();

    let view = wait_for_sign_in(&app, &id, &["failed"]).await;
    assert_eq!(view["reason"], "notSubscription");
    assert!(view["provider"].is_null());
    assert_eq!(connected_provider_count(&app, "anthropic").await, 0);
}

#[tokio::test]
async fn signing_in_again_connects_an_existing_subscription_provider() {
    let data_dir = TempDirectory::new("signin-reconnect-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "success")], None);

    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Work Claude",
            "vendor": "anthropic",
            "auth": "subscription"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    assert_eq!(provider["connected"], false);
    let provider_id = provider["id"].as_str().expect("provider id").to_owned();

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic", "providerId": provider_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();
    let view = wait_for_sign_in(&app, &id, &["success"]).await;

    assert_eq!(view["provider"]["id"], provider_id);
    assert_eq!(view["provider"]["connected"], true);
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    let listed = bootstrap["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["id"] == provider_id)
        .expect("listed provider");
    assert_eq!(listed["connected"], true);
}

#[tokio::test]
async fn abandoning_while_the_credential_is_checked_connects_nothing() {
    let data_dir = TempDirectory::new("signin-verify-cancel-data");
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(
        &data_dir.path,
        &stub,
        &[("FAKE_SIGNIN", "success"), ("FAKE_STATUS", "slow")],
        None,
    );

    let (status, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let id = started["id"].as_str().expect("sign-in id").to_owned();
    wait_for_sign_in(&app, &id, &["verifying"]).await;

    let (status, _) = call(&app, "DELETE", &format!("/api/sign-in/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, view) = call(&app, "GET", &format!("/api/sign-in/{id}"), None).await;
    assert_eq!(view["state"], "cancelled");
    assert!(view["provider"].is_null());
    assert_eq!(connected_provider_count(&app, "anthropic").await, 0);
}

#[tokio::test]
async fn a_subscription_provider_never_stores_a_key() {
    let secrets = Arc::new(MemorySecretStore::default());
    let app = router(AppState::ephemeral().with_test_secret_store(secrets.clone()));

    let (status, provider) = call(
        &app,
        "POST",
        "/api/providers",
        Some(json!({
            "name": "Sign-in only",
            "vendor": "anthropic",
            "auth": "subscription",
            "key": "sk-should-not-be-stored"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    assert_eq!(provider["connected"], false);
    let provider_id = provider["id"].as_str().expect("provider id");
    assert_eq!(
        secrets.get_key(provider_id).expect("read key").as_deref(),
        None
    );
}

#[tokio::test]
async fn a_signed_in_provider_receives_status_line_usage() {
    let data_dir = TempDirectory::new("signin-usage-data");
    let repo = TempDirectory::new("signin-usage-repo");
    init_git_repo(&repo.path);
    let stub = write_sign_in_stub(&data_dir.path);
    let app = sign_in_app(&data_dir.path, &stub, &[("FAKE_SIGNIN", "success")], None);

    let (_, started) = call(
        &app,
        "POST",
        "/api/sign-in",
        Some(json!({ "vendor": "anthropic" })),
    )
    .await;
    let id = started["id"].as_str().expect("sign-in id").to_owned();
    let view = wait_for_sign_in(&app, &id, &["success"]).await;
    let provider_id = view["provider"]["id"]
        .as_str()
        .expect("provider id")
        .to_owned();

    let (status, config) = call(
        &app,
        "POST",
        "/api/configs",
        Some(json!({ "providerId": provider_id, "client": "Claude Code" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{config}");
    ensure_test_project(&app, &repo.path).await;
    let (status, session) = call(
        &app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "configId": config["id"],
            "name": "Signed in",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["providerId"], provider_id);
    let session_id = session["id"].as_str().expect("session id");

    let (_, settings) = claude_settings(&data_dir.path, session_id);
    let token = settings_token(&settings);
    let (status, body) = send_statusline(
        &app,
        session_id,
        Some(&token),
        json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 12, "resets_at": 1 },
                "seven_day": { "used_percentage": 34, "resets_at": 2 }
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, provider) = call(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{provider}");
    assert_eq!(provider["usageSource"], "statusline");
    let rows = provider["rows"].as_array().expect("usage rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["used"], 12);
    assert_eq!(rows[1]["used"], 34);

    let (status, _) = close_session(&app, session_id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn settings_default_off_and_survive_a_reload() {
    let data_dir = TempDirectory::new("settings-persist");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap["settings"]["startAtLogin"], false);
    assert_eq!(bootstrap["settings"]["keepAwake"], false);

    let (status, settings) = call(
        &app,
        "PATCH",
        "/api/settings",
        Some(json!({ "startAtLogin": true, "keepAwake": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(settings["startAtLogin"], true);
    assert_eq!(settings["keepAwake"], true);

    drop(app);
    let reloaded = router(AppState::with_data_dir(data_dir.path.clone()));
    let (_, bootstrap) = call(&reloaded, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap["settings"]["startAtLogin"], true);
    assert_eq!(bootstrap["settings"]["keepAwake"], true);

    let state = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(state.contains("\"startAtLogin\": true"));
    assert!(state.contains("\"keepAwake\": true"));
}

#[tokio::test]
async fn devices_are_empty_and_report_connection_state() {
    let data_dir = TempDirectory::new("devices-empty");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));

    let (status, devices) = call(&app, "GET", "/api/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(devices.as_array().expect("devices array").is_empty());

    let (status, _) = call(&app, "DELETE", "/api/devices/unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_paired_device_lists_its_connection_state() {
    let data_dir = TempDirectory::new("devices-paired");
    let state = json!({
        "version": 2,
        "projects": [],
        "providers": [],
        "configs": [],
        "questions": [],
        "sessionRequests": [],
        "pairedDevices": [{
            "endpointId": "phone-1",
            "name": "Pixel",
            "platform": "Android",
            "pairedAt": "2026-11-19T10:00:00Z",
            "lastSeen": "2026-11-19T10:05:00Z"
        }],
        "sessions": []
    });
    fs::write(
        data_dir.path.join("state.json"),
        serde_json::to_vec_pretty(&state).expect("state json"),
    )
    .expect("write state");

    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let (status, devices) = call(&app, "GET", "/api/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    let devices = devices.as_array().expect("devices array");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["name"], "Pixel");
    assert_eq!(devices[0]["platform"], "Android");
    assert_eq!(devices[0]["lastSeen"], "2026-11-19T10:05:00Z");
    assert_eq!(devices[0]["connected"], false);
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[tokio::test]
async fn an_upload_is_cleaned_stored_once_and_sent_to_the_session() {
    let data_dir = TempDirectory::new("upload");
    let app = router(AppState::with_test_command_and_data_dir(
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "while IFS= read -r line; do printf 'got:%s\\n' \"$line\"; done".to_owned(),
        ],
        data_dir.path.clone(),
    ));
    let folder = TempDirectory::new("upload-session");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let request = request_builder("GET", &format!("/api/sessions/{id}/output?after=0"))
        .body(Body::empty())
        .expect("build output request");
    let response = app.clone().oneshot(request).await.expect("open output");
    let output = response.into_body();

    let upload_uri = format!("/api/sessions/{id}/upload");
    let (status, uploaded) = post_upload(
        &app,
        &upload_uri,
        Some("../../shot.png"),
        "application/octet-stream",
        PNG_SIGNATURE.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    assert_eq!(uploaded["name"], "shot.png");
    let first_path = uploaded["path"].as_str().expect("upload path").to_owned();
    assert!(
        first_path.starts_with(
            data_dir
                .path
                .join("uploads")
                .join(&id)
                .to_str()
                .expect("data dir")
        ),
        "the upload was stored outside the data folder: {first_path}"
    );
    assert_eq!(fs::read(&first_path).expect("stored upload"), PNG_SIGNATURE);

    let (status, second) = post_upload(
        &app,
        &upload_uri,
        Some("shot.png"),
        "application/octet-stream",
        b"second".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(second["name"], "shot-1.png");
    assert_ne!(second["path"], uploaded["path"]);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&first_path)
            .expect("upload metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let text = read_output_until(output, |text| text.contains("Uploaded file:")).await;
    assert!(
        text.contains(&first_path),
        "the session did not get the upload path: {text}"
    );

    let (status, body) = post_upload(
        &app,
        &upload_uri,
        None,
        "application/octet-stream",
        b"no name".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = post_upload(
        &app,
        &upload_uri,
        Some("note.txt"),
        "application/json",
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");

    let (status, _) = close_session(&app, &id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post_upload(
        &app,
        &upload_uri,
        Some("late.txt"),
        "application/octet-stream",
        b"late".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn an_upload_over_the_cap_is_refused() {
    let data_dir = TempDirectory::new("upload-large");
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 5".to_owned()],
        data_dir.path.clone(),
    ));
    let folder = TempDirectory::new("upload-large-session");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();

    let (status, body) = post_upload(
        &app,
        &format!("/api/sessions/{id}/upload"),
        Some("big.bin"),
        "application/octet-stream",
        vec![0u8; elesdee_link::MAX_UPLOAD_BYTES + 1],
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
}

#[tokio::test]
async fn an_image_is_served_by_signature_and_confined_to_the_folder() {
    let data_dir = TempDirectory::new("image");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let folder = TempDirectory::new("image-session");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let root = PathBuf::from(session["folder"].as_str().expect("session folder"));

    let mut png = PNG_SIGNATURE.to_vec();
    png.extend_from_slice(b"image body");
    fs::write(root.join("shot.png"), &png).expect("write image");
    fs::write(root.join("notes.txt"), "plain text\n").expect("write text");
    fs::create_dir_all(root.join(".git")).expect("create git folder");
    fs::write(root.join(".git").join("config"), "[remote]\n").expect("write git config");

    let (status, headers, bytes) =
        get_raw(&app, &format!("/api/sessions/{id}/image?path=shot.png")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(bytes, png);

    let (status, _, body) =
        get_raw(&app, &format!("/api/sessions/{id}/image?path=notes.txt")).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(
        String::from_utf8_lossy(&body).contains("not a supported image"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );

    let (status, _, _) = get_raw(&app, &format!("/api/sessions/{id}/image?path=../shot.png")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = get_raw(&app, &format!("/api/sessions/{id}/image?path=.git/config")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = get_raw(&app, &format!("/api/sessions/{id}/image?path=missing.png")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[cfg(unix)]
#[tokio::test]
async fn an_image_symlink_out_of_the_folder_is_refused() {
    let data_dir = TempDirectory::new("image-symlink");
    let outside = TempDirectory::new("image-symlink-outside");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));
    let folder = TempDirectory::new("image-symlink-session");
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    let root = PathBuf::from(session["folder"].as_str().expect("session folder"));

    let mut png = PNG_SIGNATURE.to_vec();
    png.extend_from_slice(b"outside");
    fs::write(outside.path.join("shot.png"), &png).expect("write outside image");
    std::os::unix::fs::symlink(outside.path.join("shot.png"), root.join("link.png"))
        .expect("create symlink");

    let (status, _, _) = get_raw(&app, &format!("/api/sessions/{id}/image?path=link.png")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn diagnostics_export_writes_a_redacted_bundle() {
    let data_dir = TempDirectory::new("diagnostics-export");
    let out_dir = TempDirectory::new("diagnostics-output");
    let app = router(AppState::with_data_dir(data_dir.path.clone()));

    let (status, body) = call(
        &app,
        "POST",
        "/api/diagnostics/export",
        Some(json!({ "folder": out_dir.path.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let path = body["path"].as_str().expect("bundle path");
    let text = fs::read_to_string(path).expect("read bundle");
    assert!(text.contains("\"app\""));
    assert!(text.contains("\"counts\""));
    assert!(!text.contains("sk-"));

    let (status, _) = call(
        &app,
        "POST",
        "/api/diagnostics/export",
        Some(json!({ "folder": "relative/path" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
