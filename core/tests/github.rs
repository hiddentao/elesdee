use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State as AxumState;
use axum::http::{Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use elesdee_core::SecretStore;
use elesdee_core::api::router;
use elesdee_core::{AppState, MemorySecretStore};

const TEST_HOST: &str = "127.0.0.1:4319";
const TOKEN: &str = "gho_test_token_value";

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "elesdee-github-{label}-{}-{}",
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

#[derive(Clone, Copy)]
enum Mode {
    Success,
    Denied,
    Expired,
    Pending(usize),
}

struct FakeState {
    mode: Mode,
    polls: usize,
    repos: Vec<Value>,
}

struct FakeGitHub {
    api: String,
    login: String,
    state: Arc<Mutex<FakeState>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn start_fake(repos: Vec<Value>, mode: Mode) -> FakeGitHub {
    let state = Arc::new(Mutex::new(FakeState {
        mode,
        polls: 0,
        repos,
    }));
    let app = Router::new()
        .route("/login/device/code", post(device_code))
        .route("/login/oauth/access_token", post(access_token))
        .route("/user", get(user))
        .route("/user/repos", get(user_repos))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake GitHub");
    let address = listener.local_addr().expect("fake GitHub address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let base = format!("http://{address}");
    FakeGitHub {
        api: base.clone(),
        login: base,
        state,
    }
}

async fn device_code() -> impl IntoResponse {
    axum::Json(json!({
        "device_code": "device-code-1",
        "user_code": "ABCD-1234",
        "verification_uri": "http://example.invalid/device",
        "expires_in": 900,
        "interval": 1
    }))
}

async fn access_token(AxumState(state): AxumState<Arc<Mutex<FakeState>>>) -> impl IntoResponse {
    let mut state = lock(&state);
    state.polls += 1;
    match state.mode {
        Mode::Success => axum::Json(json!({
            "access_token": TOKEN,
            "token_type": "bearer",
            "scope": "repo"
        })),
        Mode::Denied => axum::Json(json!({ "error": "access_denied" })),
        Mode::Expired => axum::Json(json!({ "error": "expired_token" })),
        Mode::Pending(pending) => {
            if state.polls <= pending {
                axum::Json(json!({ "error": "authorization_pending" }))
            } else {
                axum::Json(json!({
                    "access_token": TOKEN,
                    "token_type": "bearer",
                    "scope": "repo"
                }))
            }
        }
    }
}

fn authorized(request: &Request<Body>) -> bool {
    let expected = format!("Bearer {TOKEN}");
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(expected.as_str())
}

async fn user(request: Request<Body>) -> impl IntoResponse {
    if !authorized(&request) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    axum::Json(json!({
        "login": "octocat",
        "name": "The Octocat",
        "avatar_url": "http://example.invalid/avatar.png"
    }))
    .into_response()
}

async fn user_repos(
    AxumState(state): AxumState<Arc<Mutex<FakeState>>>,
    request: Request<Body>,
) -> impl IntoResponse {
    if !authorized(&request) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    axum::Json(lock(&state).repos.clone()).into_response()
}

fn repo(id: u64, owner: &str, name: &str, clone_url: &str) -> Value {
    json!({
        "id": id,
        "full_name": format!("{owner}/{name}"),
        "name": name,
        "owner": { "login": owner },
        "private": false,
        "clone_url": clone_url,
        "html_url": format!("https://example.invalid/{owner}/{name}"),
        "description": "A test repository",
        "updated_at": "2026-01-01T00:00:00Z"
    })
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

fn test_state(
    fake: &FakeGitHub,
    directory: &TempDirectory,
    store: Arc<MemorySecretStore>,
) -> AppState {
    AppState::with_data_dir(directory.path.join("data"))
        .with_test_secret_store(store)
        .with_test_github(
            fake.api.clone(),
            fake.login.clone(),
            "test-client".to_owned(),
            directory.path.join("projects"),
        )
}

async fn start_and_wait(app: &Router) -> Value {
    let (status, view) = call(app, "POST", "/api/github/device", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "start device flow: {view}");
    let id = view["id"].as_str().expect("flow id").to_owned();
    for _ in 0..100 {
        let (status, view) = call(app, "GET", &format!("/api/github/device/{id}"), None).await;
        assert_eq!(status, StatusCode::OK, "read device flow");
        match view["state"].as_str() {
            Some("success" | "failed" | "cancelled") => return view,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    panic!("the device flow did not finish");
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn create_bare_repo(directory: &Path, name: &str) -> PathBuf {
    let source = directory.join(format!("{name}-source"));
    fs::create_dir_all(&source).expect("create source repository");
    git(&source, &["init", "-q", "-b", "main"]);
    git(&source, &["config", "user.name", "Test"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    fs::write(source.join("README.md"), "hello\n").expect("write readme");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "Initial commit"]);
    let bare = directory.join(format!("{name}.git"));
    git(
        directory,
        &[
            "clone",
            "--quiet",
            "--bare",
            source.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    bare
}

#[tokio::test]
async fn device_flow_stores_the_token_outside_state() {
    let directory = TempDirectory::new("flow");
    let fake = start_fake(Vec::new(), Mode::Success).await;
    let store = Arc::new(MemorySecretStore::default());
    let state = test_state(&fake, &directory, Arc::clone(&store));
    let app = router(state.clone());

    let view = start_and_wait(&app).await;
    assert_eq!(view["state"], "success");
    assert_eq!(view["login"], "octocat");
    assert!(
        !view.to_string().contains(TOKEN),
        "the token is never in a response"
    );
    assert_eq!(
        store.get_key("github:oauth").expect("read key"),
        Some(TOKEN.to_owned())
    );

    let stored = state.data_dir().expect("data directory").join("state.json");
    state.save();
    if stored.exists() {
        let text = fs::read_to_string(&stored).expect("read state");
        assert!(!text.contains(TOKEN), "the token is never in state");
    }

    let (status, status_view) = call(&app, "GET", "/api/github/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_view["connected"], true);
    assert_eq!(status_view["login"], "octocat");
    assert!(!status_view.to_string().contains(TOKEN));

    let (status, _) = call(&app, "POST", "/api/github/sign-out", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(store.get_key("github:oauth").expect("read key"), None);
    let (_, status_view) = call(&app, "GET", "/api/github/status", None).await;
    assert_eq!(status_view["connected"], false);

    let (status, repos) = call(&app, "GET", "/api/github/repos?q=", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{repos}");
}

#[tokio::test]
async fn a_refused_code_leaves_nothing_connected() {
    let directory = TempDirectory::new("denied");
    let fake = start_fake(Vec::new(), Mode::Denied).await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, Arc::clone(&store)));

    let view = start_and_wait(&app).await;
    assert_eq!(view["state"], "failed");
    assert_eq!(view["reason"], "denied");
    assert_eq!(store.get_key("github:oauth").expect("read key"), None);
    let (_, status_view) = call(&app, "GET", "/api/github/status", None).await;
    assert_eq!(status_view["connected"], false);
}

#[tokio::test]
async fn an_expired_code_reports_expiry() {
    let directory = TempDirectory::new("expired");
    let fake = start_fake(Vec::new(), Mode::Expired).await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, Arc::clone(&store)));

    let view = start_and_wait(&app).await;
    assert_eq!(view["state"], "failed");
    assert_eq!(view["reason"], "expired");
    assert_eq!(store.get_key("github:oauth").expect("read key"), None);
}

#[tokio::test]
async fn polling_waits_for_the_authorization() {
    let directory = TempDirectory::new("pending");
    let fake = start_fake(Vec::new(), Mode::Pending(2)).await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, Arc::clone(&store)));

    let view = start_and_wait(&app).await;
    assert_eq!(view["state"], "success");
    assert!(lock(&fake.state).polls >= 3);
}

#[tokio::test]
async fn repository_search_matches_name_and_owner_and_marks_added() {
    let directory = TempDirectory::new("repos");
    let bare = create_bare_repo(&directory.path, "web");
    let clone_url = format!("file://{}", bare.display());
    let fake = start_fake(
        vec![
            repo(1, "acme", "web", &clone_url),
            repo(2, "acme", "api", "https://example.invalid/acme/api.git"),
            repo(3, "other", "web", "https://example.invalid/other/web.git"),
        ],
        Mode::Success,
    )
    .await;
    let store = Arc::new(MemorySecretStore::default());
    let state = test_state(&fake, &directory, store);
    let app = router(state);

    start_and_wait(&app).await;

    let existing = directory.path.join("existing");
    git(
        &directory.path,
        &[
            "clone",
            "--quiet",
            bare.to_str().unwrap(),
            existing.to_str().unwrap(),
        ],
    );
    let (status, project) = call(
        &app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "Existing web",
            "folder": existing.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");

    let (status, repos) = call(&app, "GET", "/api/github/repos?q=acme", None).await;
    assert_eq!(status, StatusCode::OK);
    let repos = repos.as_array().expect("repository list");
    assert_eq!(repos.len(), 2, "{repos:?}");
    let web = repos
        .iter()
        .find(|repo| repo["fullName"] == "acme/web")
        .expect("web repository");
    assert_eq!(web["cloneUrl"], clone_url);
    assert_eq!(web["added"], true);
    assert_eq!(
        web["localFolder"],
        fs::canonicalize(&existing)
            .expect("canonical project folder")
            .to_string_lossy()
            .as_ref(),
        "the marked repository returns its own project folder"
    );
    let api = repos
        .iter()
        .find(|repo| repo["fullName"] == "acme/api")
        .expect("api repository");
    assert_eq!(api["added"], false);
    assert_eq!(api["localFolder"], serde_json::Value::Null);

    let (status, repos) = call(&app, "GET", "/api/github/repos?q=web", None).await;
    assert_eq!(status, StatusCode::OK);
    let repos = repos.as_array().expect("repository list");
    assert_eq!(repos.len(), 2, "{repos:?}");
    assert!(repos.iter().all(|repo| repo["name"] == "web"));
}

#[tokio::test]
async fn cloning_adds_the_folder_as_a_project() {
    let directory = TempDirectory::new("clone");
    let bare = create_bare_repo(&directory.path, "demo");
    let fake = start_fake(
        vec![repo(
            1,
            "acme",
            "demo",
            &format!("file://{}", bare.display()),
        )],
        Mode::Success,
    )
    .await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, store));
    start_and_wait(&app).await;

    let checkouts = directory.path.join("checkouts");
    let (status, project) = call(
        &app,
        "POST",
        "/api/github/clone",
        Some(json!({
            "fullName": "acme/demo",
            "name": "Demo project",
            "parentFolder": checkouts.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["name"], "Demo project");
    let folder = PathBuf::from(project["folder"].as_str().expect("folder"));
    assert!(folder.join("README.md").exists());
    let remote = git(&folder, &["remote", "get-url", "origin"]);
    assert!(!remote.contains(TOKEN), "the token is never in the remote");

    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        bootstrap["projects"]
            .as_array()
            .is_some_and(|projects| projects.iter().any(|item| item["id"] == project["id"])),
        "the project is listed: {bootstrap}"
    );
}

#[tokio::test]
async fn a_failed_clone_removes_the_partial_folder() {
    let directory = TempDirectory::new("clone-failed");
    let fake = start_fake(
        vec![repo(
            1,
            "acme",
            "broken",
            &format!("file://{}/missing.git", directory.path.display()),
        )],
        Mode::Success,
    )
    .await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, store));
    start_and_wait(&app).await;

    let checkouts = directory.path.join("checkouts");
    fs::create_dir_all(&checkouts).expect("create checkouts folder");
    let (status, body) = call(
        &app,
        "POST",
        "/api/github/clone",
        Some(json!({
            "fullName": "acme/broken",
            "parentFolder": checkouts.to_string_lossy()
        })),
    )
    .await;
    assert!(!status.is_success(), "{body}");
    assert!(
        !checkouts.join("broken").exists(),
        "the partial folder is removed"
    );
    let (_, bootstrap) = call(&app, "GET", "/api/bootstrap", None).await;
    assert!(
        bootstrap["projects"]
            .as_array()
            .is_some_and(|projects| projects.is_empty()),
        "no project is added"
    );
}

#[tokio::test]
async fn an_existing_folder_is_never_overwritten() {
    let directory = TempDirectory::new("clone-exists");
    let bare = create_bare_repo(&directory.path, "demo");
    let fake = start_fake(
        vec![repo(
            1,
            "acme",
            "demo",
            &format!("file://{}", bare.display()),
        )],
        Mode::Success,
    )
    .await;
    let store = Arc::new(MemorySecretStore::default());
    let app = router(test_state(&fake, &directory, store));
    start_and_wait(&app).await;

    let checkouts = directory.path.join("checkouts");
    let target = checkouts.join("demo");
    fs::create_dir_all(&target).expect("create existing folder");
    fs::write(target.join("keep.txt"), "keep\n").expect("write marker");

    let (status, body) = call(
        &app,
        "POST",
        "/api/github/clone",
        Some(json!({
            "fullName": "acme/demo",
            "parentFolder": checkouts.to_string_lossy()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).expect("read marker"),
        "keep\n"
    );
}
