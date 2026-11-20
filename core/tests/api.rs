use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use elesdee_core::api::router;
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

    let accounts = body["accounts"].as_array().expect("accounts array");
    assert_eq!(accounts.len(), 4);

    let max = accounts
        .iter()
        .find(|account| account["name"] == "Anthropic Max 20×")
        .expect("Anthropic Max account");
    assert_eq!(max["client"], "Claude Code");
    assert_eq!(max["auth"], "subscription");

    let codex = accounts
        .iter()
        .find(|account| account["name"] == "ChatGPT Pro")
        .expect("ChatGPT Pro account");
    assert_eq!(codex["client"], "Codex");

    let deepseek = accounts
        .iter()
        .find(|account| account["name"] == "DeepSeek API")
        .expect("DeepSeek account");
    assert_eq!(deepseek["client"], "Claude Code");

    let litellm = accounts
        .iter()
        .find(|account| account["name"] == "LiteLLM gateway")
        .expect("LiteLLM account");
    assert_eq!(litellm["server"], "http://localhost:4000/v1");
    assert_eq!(litellm["model"], "gpt-5.2");

    for account in accounts {
        assert_eq!(
            account["rows"].as_array().expect("usage rows").len(),
            0,
            "seed usage rows are invented"
        );
        assert_eq!(account["foot"], "No official usage source.");
        assert_eq!(account["sample"], true, "seed accounts are samples");
    }

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
    assert_eq!(body["accounts"].as_array().expect("accounts").len(), 4);
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

    for path in ["/api/projects?q=", "/api/accounts?q="] {
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
        "accountId": "max",
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
        run_git(&repo.path, &["checkout", "-b", "feature"]).status.success(),
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
            "accountId": "max",
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
        run_git(&repo.path, &["checkout", "-b", "feature"]).status.success(),
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
            "accountId": "max",
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
            "accountId": "max",
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
    assert_eq!(git_ref_count(&repo.path), 1, "a failed launch left a branch");
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

    let (status, switched) = call(
        &app,
        "POST",
        &format!("/api/sessions/{id}/account"),
        Some(json!({ "accountId": second })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["accountId"], second);
    assert_eq!(switched["status"], "active");
    assert_eq!(switched["client"], "Claude Code");

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn usage_near_the_limit_switches_then_pauses_without_room() {
    let data_dir = TempDirectory::new("auto-data");
    let repo = TempDirectory::new("auto-repo");
    init_git_repo(&repo.path);
    let app = router(AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        data_dir.path.clone(),
    ));
    ensure_test_project(&app, &repo.path).await;
    let first = create_test_account(&app, "Auto A", "sk-auto-a").await;
    let second = create_test_account(&app, "Auto B", "sk-auto-b").await;

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/accounts/{second}/usage"),
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
        &format!("/api/accounts/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let switched = wait_for_session(&app, id, |session| {
        session["accountId"] == second
    })
    .await;
    assert_eq!(switched["status"], "active");
    assert_eq!(switched["workState"], "working");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/accounts/{second}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 95 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let paused = wait_for_session(&app, id, |session| {
        session["workState"] == "paused"
    })
    .await;
    assert_eq!(paused["status"], "active");
    assert!(
        paused["preview"]
            .as_str()
            .is_some_and(|preview| preview.contains("No account"))
    );

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/accounts/{first}/usage"),
        Some(json!({ "rows": [{ "label": "Weekly", "used": 30 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resumed = wait_for_session(&app, id, |session| {
        session["workState"] == "working"
    })
    .await;
    assert_eq!(resumed["accountId"], first);

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
}

async fn create_test_account(app: &Router, name: &str, key: &str) -> String {
    let (status, account) = call(
        app,
        "POST",
        "/api/accounts",
        Some(json!({
            "name": name,
            "providerId": "anthropic",
            "auth": "key",
            "key": key
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    account["id"].as_str().expect("account id").to_owned()
}

async fn wait_for_session(
    app: &Router,
    id: &str,
    ready: impl Fn(&Value) -> bool,
) -> Value {
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

#[tokio::test]
async fn claude_sessions_get_and_clean_up_their_mcp_config() {
    let data_dir = TempDirectory::new("mcp-config-data");
    let repo = TempDirectory::new("mcp-config-repo");
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
            "accountId": "max",
            "name": "MCP config",
            "folder": repo.path.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let id = session["id"].as_str().expect("session id");
    let config_path = data_dir.path.join("mcp").join(format!("{id}.json"));
    assert!(config_path.is_file(), "the MCP config was not written");
    let config: Value =
        serde_json::from_slice(&fs::read(&config_path).expect("read MCP config"))
            .expect("parse MCP config");
    let url = config["mcpServers"]["elesdee"]["url"]
        .as_str()
        .expect("MCP URL");
    assert_eq!(url, format!("http://127.0.0.1:4319/internal/mcp/{id}"));
    let skill = data_dir
        .path
        .join("plugin")
        .join("skills")
        .join("elesdee")
        .join("SKILL.md");
    assert!(skill.is_file(), "the elesdee skill was not written");
    assert!(
        fs::read_to_string(&skill)
            .expect("read skill")
            .contains("ask_me")
    );

    let (status, _) = close_session(&app, id).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!config_path.exists(), "the MCP config was not removed");
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
            "accountId": "max",
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
            "accountId": "max",
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
        "list_sessions",
        "read_session",
        "send_message",
        "check_usage",
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
            "accountId": "max",
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
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
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
        Some("content-type")
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
    assert_eq!(state["version"], 1);
    assert_eq!(state["accounts"].as_array().expect("accounts").len(), 4);
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
    assert_eq!(bootstrap["accounts"].as_array().expect("accounts").len(), 4);
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
async fn active_session_restores_closed_with_the_restart_note() {
    let data_dir = TempDirectory::new("persist-active-data");
    let folder = TempDirectory::new("persist-active-folder");
    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];
    let app = router(AppState::with_test_command_and_data_dir(
        command.clone(),
        data_dir.path.clone(),
    ));
    let session = create_terminal(&app, &folder.path).await;
    let id = session["id"].as_str().expect("session id").to_owned();
    assert_eq!(session["status"], "active");
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
        .cloned()
        .expect("restored session");

    assert_eq!(restored["status"], "active");
    assert_eq!(restored["workState"], "paused");
    assert!(restored["waitingFor"].is_null());
    assert_eq!(
        restored["preview"],
        "Paused. The app restarted. Resume to continue."
    );

    let (status, resumed) = call(
        &restarted,
        "POST",
        &format!("/api/sessions/{id}/resume"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["status"], "active");
    assert_eq!(resumed["workState"], "working");

    let (status, _) = close_session(&restarted, &id).await;
    assert_eq!(status, StatusCode::OK);
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
async fn graceful_shutdown_saves_the_restart_note_and_logs() {
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
    assert_eq!(restored_b["workState"], "paused");
    assert_eq!(
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
    assert_eq!(bootstrap["accounts"].as_array().expect("accounts").len(), 4);
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
        "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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
            "accountId": "max",
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

    let (status, account) = call(
        &app,
        "POST",
        "/api/accounts",
        Some(json!({
            "name": "Google Gemini",
            "providerId": "google",
            "client": "Antigravity",
            "auth": "key",
            "key": "AIza-test-key-123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let id = account["id"].as_str().expect("account id").to_owned();
    assert_eq!(account["client"], "Antigravity");
    assert_eq!(account["provider"], "google");
    assert_eq!(account["auth"], "key");
    assert_eq!(account["sample"], false);
    assert_eq!(account["model"], "gemini-3.8-flash-medium");
    assert_eq!(account["rows"].as_array().expect("rows").len(), 0);
    assert_eq!(account["foot"], "No official usage source.");
    assert!(account.get("key").is_none());
    assert_eq!(
        secrets.get_key(&id).expect("read stored key"),
        Some("AIza-test-key-123".to_owned())
    );

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap["accounts"].as_array().expect("accounts").len(), 5);
    assert!(!bootstrap.to_string().contains("AIza-test-key-123"));
    let state_text = fs::read_to_string(data_dir.path.join("state.json")).expect("state file");
    assert!(!state_text.contains("AIza-test-key-123"));
    assert!(state_text.contains("Antigravity"));

    let (status, duplicate) = call(
        &app,
        "POST",
        "/api/accounts",
        Some(json!({
            "name": "Google Gemini",
            "providerId": "google",
            "client": "Antigravity",
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

    let (status, custom_model) = call(
        &app,
        "POST",
        "/api/accounts",
        Some(json!({
            "name": "Google Pro",
            "providerId": "google",
            "client": "Antigravity",
            "auth": "key",
            "key": "AIza-third-key",
            "model": "gemini-3.8-pro"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{custom_model}");
    assert_eq!(custom_model["model"], "gemini-3.8-pro");

    let invalid = [
        (
            json!({"name": "", "providerId": "google", "client": "Antigravity", "auth": "key", "key": "k"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "Wrong provider", "providerId": "anthropic", "client": "Antigravity", "auth": "key", "key": "k"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "Wrong client", "providerId": "google", "client": "Claude Code", "auth": "key", "key": "k"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "Wrong auth", "providerId": "google", "client": "Antigravity", "auth": "subscription", "key": "k"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "No key", "providerId": "google", "client": "Antigravity", "auth": "key", "key": "  "}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "Long model", "providerId": "google", "client": "Antigravity", "auth": "key", "key": "k", "model": "x".repeat(121)}),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (request, expected) in invalid {
        let (status, body) = call(&app, "POST", "/api/accounts", Some(request)).await;
        assert_eq!(status, expected, "{body}");
    }
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(bootstrap["accounts"].as_array().expect("accounts").len(), 7);
}

#[tokio::test]
async fn provider_list_matches_the_supported_table() {
    let app = router(AppState::ephemeral());
    let (status, providers) = call(&app, "GET", "/api/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{providers}");
    let providers = providers.as_array().expect("providers");
    let ids = providers
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
    let custom = providers
        .iter()
        .find(|provider| provider["id"] == "custom")
        .expect("custom provider");
    assert_eq!(custom["custom"], true);
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
        "/api/accounts",
        Some(json!({
            "name": "DeepSeek work",
            "providerId": "deepseek",
            "auth": "key",
            "key": "sk-deepseek-secret"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["client"], "Claude Code");
    assert_eq!(account["provider"], "deepseek");
    assert!(account.get("key").is_none(), "the response leaked the key");
    let id = account["id"].as_str().expect("account id");
    assert_eq!(
        secrets.get_key(id).expect("read key").as_deref(),
        Some("sk-deepseek-secret")
    );

    let (status, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert_eq!(status, StatusCode::OK);
    let text = bootstrap.to_string();
    assert!(!text.contains("sk-deepseek-secret"), "bootstrap leaked the key");
    let state_text =
        fs::read_to_string(data_dir.path.join("state.json")).expect("read state file");
    assert!(!state_text.contains("sk-deepseek-secret"), "state leaked the key");
}

#[tokio::test]
async fn custom_provider_account_validates_style_client_and_server() {
    let app = router(AppState::ephemeral());
    let valid = json!({
        "name": "Gateway",
        "providerId": "custom",
        "auth": "key",
        "apiStyle": "openai",
        "client": "Codex",
        "key": "sk-gateway",
        "server": "https://gateway.example/v1",
        "model": "gpt-test"
    });
    let (status, account) = call(&app, "POST", "/api/accounts", Some(valid)).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["client"], "Codex");
    assert_eq!(account["apiStyle"], "openai");
    assert_eq!(account["server"], "https://gateway.example/v1");
    assert_eq!(account["model"], "gpt-test");

    let invalid = [
        (
            json!({"name": "No style", "providerId": "custom", "auth": "key", "key": "k", "server": "https://example.com"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "Wrong client", "providerId": "custom", "auth": "key", "apiStyle": "anthropic", "client": "Codex", "key": "k", "server": "https://example.com"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"name": "No server", "providerId": "custom", "auth": "key", "apiStyle": "openai", "client": "Codex", "key": "k"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "Bad server", "providerId": "custom", "auth": "key", "apiStyle": "openai", "client": "Codex", "key": "k", "server": "ftp://example.com"}),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (request, expected) in invalid {
        let (status, body) = call(&app, "POST", "/api/accounts", Some(request)).await;
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

    let (status, all) = call(&app, "GET", "/api/accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let accounts = all.as_array().expect("accounts");
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
        ("antigravity", "Google Gemini"),
        ("google", "Google Gemini"),
        ("team", "LiteLLM gateway"),
        ("anthropic", "Anthropic Max 20×"),
    ] {
        let (status, filtered) =
            call(&app, "GET", &format!("/api/accounts?q={filter}"), None).await;
        assert_eq!(status, StatusCode::OK, "{filter}: {filtered}");
        let matches = filtered.as_array().expect("accounts");
        assert_eq!(matches.len(), 1, "{filter}: {filtered}");
        assert_eq!(matches[0]["name"], expected, "{filter}");
    }

    let (status, empty) = call(&app, "GET", "/api/accounts?q=", None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty.as_array().expect("accounts").len(), 5);

    let (status, miss) = call(&app, "GET", "/api/accounts?q=missing", None).await;
    assert_eq!(status, StatusCode::OK, "{miss}");
    assert!(miss.as_array().expect("accounts").is_empty());
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
        "/api/accounts",
        Some(json!({
            "name": "Persistent Google",
            "providerId": "google",
            "client": "Antigravity",
            "auth": "key",
            "key": "AIza-persist-key"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let id = account["id"].as_str().expect("account id").to_owned();
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
    let restored_account = bootstrap["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .find(|account| account["id"].as_str() == Some(id.as_str()))
        .expect("restored account");
    assert_eq!(restored_account["client"], "Antigravity");
    assert_eq!(restored_account["sample"], false);
    assert_eq!(restored_account["model"], "gemini-3.8-flash-medium");
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
    assert_eq!(session["accountId"], account_id);
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
        br#"{"modelProvider":"gemini"}"#
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
        br#"{"modelProvider":"gemini"}"#
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

async fn create_google_account(app: &Router, name: &str, key: &str) -> Value {
    let (status, account) = call(
        app,
        "POST",
        "/api/accounts",
        Some(json!({
            "name": name,
            "providerId": "google",
            "client": "Antigravity",
            "auth": "key",
            "key": key
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create Google account: {account}");
    account
}

async fn create_agent(app: &Router, repo: &Path, account: &str, name: &str) -> Value {
    ensure_test_project(app, repo).await;
    let (status, session) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "agent",
            "accountId": account,
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
