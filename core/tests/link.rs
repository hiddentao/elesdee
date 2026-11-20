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
    APP_ALPN, AppHello, CLOSE_REVOKED, ErrorCode, MAX_APP_FRAME_BYTES, MAX_HEADER_BYTES,
    MAX_UPLOAD_BYTES, Method, PAIR_ALPN, PairTicket, Request, Response, SecretKey, StreamEvent,
    Welcome, pairing_code, read_app_frame, write_app_frame,
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

fn shell_state(path: &Path) -> AppState {
    AppState::with_test_command_and_data_dir(vec!["/bin/sh".to_owned()], path.to_path_buf())
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

async fn app_connect(
    phone: &Endpoint,
    addr: &EndpointAddr,
) -> (iroh::endpoint::Connection, Welcome) {
    let connection = phone
        .connect(addr.clone(), APP_ALPN)
        .await
        .expect("app connect");
    let (mut send, mut recv) = connection.open_bi().await.expect("control stream");
    let hello = AppHello::new(1, "android", "0.1.0");
    write_app_frame(&mut send, &hello, MAX_APP_FRAME_BYTES)
        .await
        .expect("write hello");
    send.finish().expect("finish hello");
    let welcome: Welcome = read_app_frame(&mut recv, MAX_APP_FRAME_BYTES)
        .await
        .expect("read welcome");
    (connection, welcome)
}

async fn request_on(connection: &iroh::endpoint::Connection, request: Request) -> Response {
    let (mut send, mut recv) = connection.open_bi().await.expect("app stream");
    let encoded = request.encode().expect("encode request");
    send.write_all(&encoded).await.expect("write request");
    send.finish().expect("finish request");
    read_response(&mut recv).await
}

async fn raw_request_on(connection: &iroh::endpoint::Connection, frame: Vec<u8>) -> Response {
    let (mut send, mut recv) = connection.open_bi().await.expect("raw app stream");
    send.write_all(&frame).await.expect("write raw request");
    send.finish().expect("finish raw request");
    read_response(&mut recv).await
}

async fn read_response(recv: &mut RecvStream) -> Response {
    let (header, body) = read_frame(recv).await;
    let encoded = serde_json::to_vec(&header).expect("header encodes");
    let mut frame = (encoded.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&encoded);
    frame.extend_from_slice(&body);
    Response::decode(&frame).expect("decode response")
}

async fn read_stream_event(recv: &mut RecvStream) -> Option<Value> {
    let mut length = [0u8; 4];
    match recv.read_exact(&mut length).await {
        Ok(()) => {}
        Err(iroh::endpoint::ReadExactError::FinishedEarly(0)) => return None,
        Err(error) => panic!("stream event length: {error}"),
    }
    let length = u32::from_be_bytes(length) as usize;
    assert!(length > 0 && length <= MAX_HEADER_BYTES);
    let mut header = vec![0u8; length];
    recv.read_exact(&mut header).await.expect("stream event");
    Some(
        StreamEvent::decode(&{
            let mut frame = (length as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&header);
            frame
        })
        .expect("decode stream event")
        .into_value(),
    )
}

async fn read_frame(recv: &mut RecvStream) -> (Value, Vec<u8>) {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length).await.expect("frame length");
    let length = u32::from_be_bytes(length) as usize;
    assert!(length > 0 && length <= MAX_HEADER_BYTES);
    let mut header = vec![0u8; length];
    recv.read_exact(&mut header).await.expect("frame header");
    let value: Value = serde_json::from_slice(&header).expect("frame header is JSON");
    let body_length = value
        .get("bytes")
        .and_then(Value::as_u64)
        .unwrap_or_default() as usize;
    let mut body = vec![0u8; body_length];
    if body_length > 0 {
        recv.read_exact(&mut body).await.expect("frame body");
    }
    (value, body)
}

async fn app_request(phone: &Endpoint, addr: &EndpointAddr, request: Request) -> Response {
    let (connection, _welcome) = app_connect(phone, addr).await;
    let response = request_on(&connection, request).await;
    connection.close(0u32.into(), b"done");
    response
}

fn raw_request_frame(method: &str, params: Option<Value>) -> Vec<u8> {
    let mut header = json!({"v": 1, "method": method});
    if let Some(params) = params {
        header["params"] = params;
    }
    let encoded = serde_json::to_vec(&header).expect("raw header encodes");
    let mut frame = (encoded.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&encoded);
    frame
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

        // A paired phone cannot choose a session folder.
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
        let created = response.result().expect("session create result");
        assert_eq!(
            created["folder"], session["folder"],
            "the phone chose the session folder"
        );
        let after = app_request(&phone, &addr, Request::new(Method::Bootstrap, None)).await;
        let result = after.result().expect("bootstrap result");
        assert_eq!(
            result["sessions"].as_array().expect("sessions").len(),
            2,
            "the remote session was not created in the project folder"
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

        let (connection, _welcome) = app_connect(&phone, &addr).await;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_phone_says_hello_and_calls_the_allowed_methods() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("hello");
        let state = test_state(&directory.path);
        let app = router(state.clone());
        let session = create_terminal(&app, &directory.path).await;
        let session_id = session["id"].as_str().expect("session id").to_owned();
        let folder = session["folder"]
            .as_str()
            .expect("session folder")
            .to_owned();

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

        let (connection, welcome) = app_connect(&phone, &addr).await;
        let desktop = state.link().desktop().expect("desktop");
        assert_eq!(welcome.v, 1);
        assert_eq!(welcome.computer_id, desktop.id_string);
        assert_eq!(welcome.computer_name, desktop.name);
        assert_eq!(welcome.protocol_version, 1);
        assert_eq!(welcome.min_bridge_version, 1);

        let response = request_on(&connection, Request::new(Method::Health, None)).await;
        let health = response.result().expect("health result");
        assert_eq!(health["ok"], true);
        assert!(
            health["agents"].is_array(),
            "health has no agents: {health}"
        );

        let response = request_on(&connection, Request::new(Method::ProjectsList, None)).await;
        let projects = response.result().expect("projects result");
        assert!(
            projects
                .as_array()
                .expect("projects array")
                .iter()
                .any(|project| project["id"] == "web"),
            "projects did not list the project: {projects}"
        );

        let response = request_on(
            &connection,
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
        let created = response.result().expect("created session");
        let created_id = created["id"].as_str().expect("created id").to_owned();
        assert_eq!(created["folder"], folder, "the phone chose the folder");

        let response = request_on(
            &connection,
            Request::new(
                Method::SessionsClose,
                Some(json!({"sessionId": created_id})),
            ),
        )
        .await;
        let closed = response.result().expect("closed session");
        assert_eq!(closed["status"], "closed");

        let response = request_on(&connection, Request::new(Method::QuestionsList, None)).await;
        assert!(
            response
                .result()
                .expect("questions")
                .as_array()
                .expect("questions array")
                .is_empty()
        );

        let denied = raw_request_on(
            &connection,
            raw_request_frame("sessions.delete", Some(json!({"sessionId": session_id}))),
        )
        .await;
        assert_eq!(
            denied.error_body().expect("denied error").code(),
            ErrorCode::Forbidden
        );

        let response = request_on(
            &connection,
            Request::new(Method::FilesList, Some(json!({"sessionId": session_id}))),
        )
        .await;
        assert!(
            response.result().expect("file list")["entries"].is_array(),
            "file list is missing entries"
        );

        connection.close(0u32.into(), b"done");
        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("hello test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_and_output_replay_reach_the_phone() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("streams");
        let state = shell_state(&directory.path);
        let app = router(state.clone());
        let session = create_terminal(&app, &directory.path).await;
        let session_id = session["id"].as_str().expect("session id").to_owned();

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
        let (connection, _welcome) = app_connect(&phone, &addr).await;

        let (mut event_send, mut event_recv) = connection.open_bi().await.expect("event stream");
        write_request(&mut event_send, Request::new(Method::EventsSubscribe, None)).await;
        assert!(matches!(
            read_response(&mut event_recv).await,
            Response::StreamStart
        ));

        let (status, _) = call(
            &app,
            "PATCH",
            &format!("/api/sessions/{session_id}"),
            Some(json!({"name": "renamed from the desktop"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let renamed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = read_stream_event(&mut event_recv)
                    .await
                    .expect("event stream ended early");
                if event["type"] == "session.updated"
                    && event["session"]["name"] == "renamed from the desktop"
                {
                    break event;
                }
            }
        })
        .await
        .expect("the rename event did not arrive");
        assert_eq!(renamed["session"]["id"], session_id);

        let (mut output_send, mut output_recv) = connection.open_bi().await.expect("output stream");
        write_request(
            &mut output_send,
            Request::new(
                Method::OutputSubscribe,
                Some(json!({"sessionId": session_id, "after": 0})),
            ),
        )
        .await;
        assert!(matches!(
            read_response(&mut output_recv).await,
            Response::StreamStart
        ));

        let response = request_on(
            &connection,
            Request::new(
                Method::SessionsInput,
                Some(json!({"sessionId": session_id, "data": "printf 'hello\\n'\n"})),
            ),
        )
        .await;
        assert_eq!(response.result(), Some(&Value::Null), "input was rejected");

        let first = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = read_stream_event(&mut output_recv)
                    .await
                    .expect("output stream ended early");
                let data = event["data"].as_str().unwrap_or_default();
                if data.contains("hello") {
                    break event;
                }
            }
        })
        .await
        .expect("the first output did not arrive");
        let first_seq = first["seq"].as_u64().expect("first sequence");

        let response = request_on(
            &connection,
            Request::new(
                Method::SessionsInput,
                Some(json!({"sessionId": session_id, "data": "printf 'world\\n'\n"})),
            ),
        )
        .await;
        assert!(response.result().is_some(), "second input was rejected");

        let second = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = read_stream_event(&mut output_recv)
                    .await
                    .expect("output stream ended early");
                let data = event["data"].as_str().unwrap_or_default();
                if data.contains("world") {
                    break event;
                }
            }
        })
        .await
        .expect("the live output did not arrive");
        let second_seq = second["seq"].as_u64().expect("second sequence");
        assert!(
            second_seq > first_seq,
            "sequences did not increase: {first_seq} then {second_seq}"
        );

        let response = request_on(
            &connection,
            Request::new(
                Method::SessionsClose,
                Some(json!({"sessionId": session_id})),
            ),
        )
        .await;
        assert_eq!(response.result().expect("closed")["status"], "closed");
        let ended = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if read_stream_event(&mut output_recv).await.is_none() {
                    break;
                }
            }
        })
        .await;
        assert!(ended.is_ok(), "the output stream did not end on close");

        connection.close(0u32.into(), b"done");
        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("stream test timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_revoked_connection_closes_with_code_three() {
    tokio::time::timeout(TIMEOUT, async {
        let directory = TempDirectory::new("live-revoked");
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
        let (connection, _welcome) = app_connect(&phone, &addr).await;
        let endpoint_id = device["endpointId"]
            .as_str()
            .expect("endpoint id")
            .to_owned();

        let (status, _) = call(&app, "DELETE", &format!("/api/devices/{endpoint_id}"), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let closed = tokio::time::timeout(Duration::from_secs(10), connection.closed())
            .await
            .expect("the live phone was not closed");
        match closed {
            ConnectionError::ApplicationClosed(close) => {
                assert_eq!(close.error_code, VarInt::from_u32(CLOSE_REVOKED));
            }
            other => panic!("unexpected close for a removed phone: {other:?}"),
        }

        phone.close().await;
        service.shutdown().await;
        assert!(state.shutdown_all());
    })
    .await
    .expect("live revocation test timed out");
}

async fn write_request(send: &mut iroh::endpoint::SendStream, request: Request) {
    let encoded = request.encode().expect("encode request");
    send.write_all(&encoded).await.expect("write request");
    send.finish().expect("finish request");
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
