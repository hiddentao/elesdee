//! Pairing and the authenticated app path over two real loopback iroh endpoints.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use data_encoding::BASE32_NOPAD;
use http_body_util::BodyExt;
use iroh::endpoint::{ConnectionError, RecvStream, VarInt, presets};
use iroh::{Endpoint, EndpointAddr};
use iroh_tickets::Ticket;
use serde_json::{Value, json};
use tower::ServiceExt;

use elesdee_core::api::router;
use elesdee_core::link::{PairReply, PairRequest, read_pair_frame, write_pair_frame};
use elesdee_core::state::PairedDevice;
use elesdee_core::{AppState, LinkService};
use elesdee_link::{
    APP_ALPN, ErrorCode, MAX_HEADER_BYTES, MAX_UPLOAD_BYTES, Method, PAIR_ALPN, PairTicket,
    Request, Response, SecretKey, pairing_code,
};

const TEST_HOST: &str = "127.0.0.1:4319";
const TIMEOUT: Duration = Duration::from_secs(60);
const READ_LIMIT: usize = 4 + MAX_HEADER_BYTES + MAX_UPLOAD_BYTES;

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "elesdee-link-{label}-{}-{}",
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
    HttpRequest::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, TEST_HOST)
}

async fn send_raw(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = request_builder(method, uri);
    let request = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            builder.body(Body::from(value.to_string()))
        }
        None => builder.body(Body::empty()),
    }
    .expect("build request");
    let response = app.clone().oneshot(request).await.expect("call route");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes()
        .to_vec();
    (status, bytes)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (status, bytes) = send_raw(app, method, uri, body).await;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

fn test_state(path: &Path) -> AppState {
    AppState::with_test_command_and_data_dir(
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        path.to_path_buf(),
    )
}

async fn loopback_endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind address")
        .bind()
        .await
        .expect("bind endpoint")
}

async fn create_terminal(app: &Router, directory: &Path) -> Value {
    let folder = directory.join("project");
    fs::create_dir_all(&folder).expect("create project folder");
    let (status, project) = call(
        app,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "web",
            "folder": folder.to_string_lossy(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create project: {project}");

    let (status, session) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": folder.to_string_lossy(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create session: {session}");
    session
}

async fn start_pairing(
    phone: &Endpoint,
    ticket: &PairTicket,
    name: &str,
) -> (iroh::endpoint::Connection, RecvStream, String) {
    let connection = phone
        .connect(ticket.addr().clone(), PAIR_ALPN)
        .await
        .expect("pair connect");
    let (mut send, mut recv) = connection.open_bi().await.expect("pair stream");
    let request = PairRequest::new(ticket.secret(), name, "android");
    write_pair_frame(&mut send, &request)
        .await
        .expect("write pair request");
    send.finish().expect("finish pair request");
    let reply: PairReply = read_pair_frame(&mut recv).await.expect("read confirm");
    assert!(reply.ok, "pairing was rejected: {reply:?}");
    let code = reply.code.expect("confirmation code");
    (connection, recv, code)
}

async fn approve_and_read(
    app: &Router,
    phone: &Endpoint,
    ticket: &PairTicket,
) -> (iroh::endpoint::Connection, Value) {
    let (connection, mut recv, _code) = start_pairing(phone, ticket, "Phone 7F3A").await;
    let (status, device) = call(app, "POST", "/api/pairing/approve", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "approve: {device}");
    let reply: PairReply = read_pair_frame(&mut recv).await.expect("paired reply");
    assert!(reply.ok, "pairing did not finish: {reply:?}");
    assert_eq!(reply.status.as_deref(), Some("paired"));
    (connection, device)
}

async fn app_request(phone: &Endpoint, addr: &EndpointAddr, request: Request) -> Response {
    let connection = phone
        .connect(addr.clone(), APP_ALPN)
        .await
        .expect("app connect");
    let (mut send, mut recv) = connection.open_bi().await.expect("app stream");
    let encoded = request.encode().expect("encode request");
    send.write_all(&encoded).await.expect("write request");
    send.finish().expect("finish request");
    let frame = recv.read_to_end(READ_LIMIT).await.expect("read response");
    let response = Response::decode(&frame).expect("decode response");
    connection.close(0u32.into(), b"done");
    response
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_phone_pairs_once_and_reads_sessions() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("pair-once");
        let state = test_state(&directory.path);
        let app = router(state.clone());
        let session = create_terminal(&app, &directory.path).await;
        let session_id = session["id"].as_str().expect("session id").to_owned();

        let service = LinkService::start_with_endpoint(state.clone(), loopback_endpoint().await);
        let addr = service.addr();

        // The local route creates a ticket while the endpoint runs.
        let (status, created) = call(&app, "POST", "/api/pairing", Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK, "create ticket: {created}");
        assert!(created["expiresAt"].as_u64().expect("expiry") > 0);
        assert_eq!(created["relayReady"], false);

        let ticket = PairTicket::decode_string(
            &state
                .link()
                .create_ticket_at(elesdee_core::link::now_unix() + 120, false)
                .expect("ticket")
                .ticket,
        )
        .expect("decode ticket");

        let (status, png) = send_raw(&app, "GET", "/api/pairing/qr", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let phone = loopback_endpoint().await;
        let (pair_connection, mut pair_recv, code) =
            start_pairing(&phone, &ticket, "Phone 7F3A").await;
        assert_eq!(
            code,
            pairing_code(ticket.secret(), &phone.id(), &ticket.addr().id)
        );

        let (status, snapshot) = call(&app, "GET", "/api/pairing", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(snapshot["state"], "confirm");
        assert_eq!(snapshot["code"], code);
        assert_eq!(snapshot["phoneName"], "Phone 7F3A");

        let (status, device) = call(&app, "POST", "/api/pairing/approve", Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK, "approve: {device}");
        assert_eq!(device["endpointId"], phone.id().to_z32());
        let reply: PairReply = read_pair_frame(&mut pair_recv).await.expect("paired reply");
        assert!(reply.ok);
        assert_eq!(reply.status.as_deref(), Some("paired"));
        pair_connection.close(0u32.into(), b"done");

        // A paired phone can read the session list through the app protocol.
        let response = app_request(&phone, &addr, Request::new(Method::Bootstrap, None)).await;
        let result = response.result().expect("bootstrap result");
        let sessions = result["sessions"].as_array().expect("sessions array");
        assert!(
            sessions.iter().any(|value| value["id"] == session_id),
            "bootstrap did not list the session: {result}"
        );

        // A paired phone cannot create a session with its own folder.
        let response = app_request(
            &phone,
            &addr,
            Request::new(
                Method::SessionsCreate,
                Some(json!({
                    "projectId": "web",
                    "kind": "terminal",
                    "folder": "/",
                })),
            ),
        )
        .await;
        assert_eq!(
            response.error_body().expect("session create error").code(),
            ErrorCode::Unsupported
        );
        let after = app_request(&phone, &addr, Request::new(Method::Bootstrap, None)).await;
        let result = after.result().expect("bootstrap result");
        assert_eq!(
            result["sessions"].as_array().expect("sessions").len(),
            1,
            "the remote folder request created a session"
        );

        // The device is saved without any secret.
        let text = fs::read_to_string(directory.path.join("state.json")).expect("state file");
        let stored: Value = serde_json::from_str(&text).expect("parse state");
        assert_eq!(
            stored["pairedDevices"]
                .as_array()
                .expect("paired devices")
                .len(),
            1
        );
        assert!(!text.contains(&BASE32_NOPAD.encode(&ticket.secret()[..])));
        assert!(!text.contains("\"secret\""));

        // The identity key is private and 32 bytes.
        let key = fs::read(directory.path.join("link.key")).expect("identity file");
        assert_eq!(key.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(directory.path.join("link.key"))
                .expect("identity metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // The same ticket cannot be used a second time.
        let reuse = phone
            .connect(ticket.addr().clone(), PAIR_ALPN)
            .await
            .expect("reuse connect");
        let (mut reuse_send, mut reuse_recv) = reuse.open_bi().await.expect("reuse stream");
        write_pair_frame(
            &mut reuse_send,
            &PairRequest::new(ticket.secret(), "Phone 7F3A", "android"),
        )
        .await
        .expect("write reuse request");
        reuse_send.finish().expect("finish reuse request");
        let reuse_reply: PairReply = read_pair_frame(&mut reuse_recv).await.expect("reuse reply");
        assert!(!reuse_reply.ok, "a used ticket was accepted");
        assert_eq!(reuse_reply.status.as_deref(), Some("used"));
        reuse.close(0u32.into(), b"done");

        let (status, devices) = call(&app, "GET", "/api/devices", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(devices.as_array().expect("devices").len(), 1);

        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("pairing integration test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_header_is_rejected_before_the_body() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("oversized-header");
        let state = test_state(&directory.path);
        let app = router(state.clone());
        let service = LinkService::start_with_endpoint(state.clone(), loopback_endpoint().await);
        let addr = service.addr();

        let ticket = PairTicket::decode_string(
            &state
                .link()
                .create_ticket_at(elesdee_core::link::now_unix() + 120, false)
                .expect("ticket")
                .ticket,
        )
        .expect("decode ticket");
        let phone = loopback_endpoint().await;
        let (pair_connection, _device) = approve_and_read(&app, &phone, &ticket).await;
        pair_connection.close(0u32.into(), b"done");

        let connection = phone.connect(addr, APP_ALPN).await.expect("app connect");
        let (mut send, mut recv) = connection.open_bi().await.expect("app stream");
        let length = ((MAX_HEADER_BYTES + 1) as u32).to_be_bytes();
        send.write_all(&length).await.expect("write length");
        send.finish().expect("finish request");
        let frame = recv.read_to_end(READ_LIMIT).await.expect("read response");
        let response = Response::decode(&frame).expect("decode response");
        assert_eq!(
            response.error_body().expect("error").code(),
            ErrorCode::TooLarge
        );
        connection.close(0u32.into(), b"done");

        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("oversized header test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_ticket_is_rejected() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("expired");
        let state = test_state(&directory.path);
        let service = LinkService::start_with_endpoint(state.clone(), loopback_endpoint().await);
        let _ = service.addr();

        let ticket = PairTicket::decode_string(
            &state
                .link()
                .create_ticket_at(elesdee_core::link::now_unix().saturating_sub(1), false)
                .expect("ticket")
                .ticket,
        )
        .expect("decode ticket");

        let phone = loopback_endpoint().await;
        let connection = phone
            .connect(ticket.addr().clone(), PAIR_ALPN)
            .await
            .expect("pair connect");
        let (mut send, mut recv) = connection.open_bi().await.expect("pair stream");
        write_pair_frame(
            &mut send,
            &PairRequest::new(ticket.secret(), "Phone", "android"),
        )
        .await
        .expect("write request");
        send.finish().expect("finish request");
        let reply: PairReply = read_pair_frame(&mut recv).await.expect("expired reply");
        assert!(!reply.ok);
        assert_eq!(reply.status.as_deref(), Some("expired"));
        connection.close(0u32.into(), b"done");

        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("expiry integration test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_device_cannot_connect() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("removed");
        let state = test_state(&directory.path);
        let app = router(state.clone());
        let service = LinkService::start_with_endpoint(state.clone(), loopback_endpoint().await);
        let addr = service.addr();

        let ticket = PairTicket::decode_string(
            &state
                .link()
                .create_ticket_at(elesdee_core::link::now_unix() + 120, false)
                .expect("ticket")
                .ticket,
        )
        .expect("decode ticket");
        let phone = loopback_endpoint().await;
        let (pair_connection, device) = approve_and_read(&app, &phone, &ticket).await;
        pair_connection.close(0u32.into(), b"done");
        let endpoint_id = device["endpointId"]
            .as_str()
            .expect("endpoint id")
            .to_owned();
        assert_eq!(endpoint_id, phone.id().to_z32());

        let (status, _) = call(&app, "DELETE", &format!("/api/devices/{endpoint_id}"), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (_, devices) = call(&app, "GET", "/api/devices", None).await;
        assert!(devices.as_array().expect("devices").is_empty());

        let connection = phone
            .connect(addr.clone(), APP_ALPN)
            .await
            .expect("app connect");
        let closed = tokio::time::timeout(Duration::from_secs(10), connection.closed())
            .await
            .expect("the removed phone was not closed");
        match closed {
            ConnectionError::ApplicationClosed(close) => {
                assert_eq!(close.error_code, VarInt::from_u32(3));
            }
            other => panic!("unexpected close for a removed phone: {other:?}"),
        }

        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("removal integration test timed out");
}

#[tokio::test]
async fn the_local_pairing_routes_are_safe_without_an_endpoint() {
    let directory = TempDirectory::new("routes");
    let state = AppState::with_data_dir(directory.path.clone());
    let app = router(state.clone());

    let (status, body) = call(&app, "POST", "/api/pairing", Some(json!({}))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");

    let (status, body) = call(&app, "GET", "/api/pairing", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "idle");

    let (status, _) = send_raw(&app, "GET", "/api/pairing/qr", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(&app, "POST", "/api/pairing/approve", Some(json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = call(&app, "POST", "/api/pairing/deny", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, devices) = call(&app, "GET", "/api/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(devices.as_array().expect("devices").is_empty());

    let (status, _) = call(&app, "DELETE", "/api/devices/unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let device = PairedDevice {
        endpoint_id: SecretKey::from_bytes(&[7; 32]).public().to_z32(),
        name: "Phone 7F3A".to_owned(),
        platform: "android".to_owned(),
        paired_at: "2026-11-20T09:00:00Z".to_owned(),
        last_seen: None,
    };
    state.add_paired_device(device.clone()).expect("add device");
    state.save();

    let (status, devices) = call(&app, "GET", "/api/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(devices.as_array().expect("devices").len(), 1);

    let text = fs::read_to_string(directory.path.join("state.json")).expect("state file");
    assert!(text.contains("pairedDevices"));

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/devices/{}", device.endpoint_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, devices) = call(&app, "GET", "/api/devices", None).await;
    assert!(devices.as_array().expect("devices").is_empty());
}

#[tokio::test]
async fn a_bad_origin_cannot_read_devices() {
    let state = AppState::ephemeral();
    let app = router(state);
    let request = HttpRequest::builder()
        .method("GET")
        .uri("/api/devices")
        .header(header::HOST, TEST_HOST)
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("call route");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
