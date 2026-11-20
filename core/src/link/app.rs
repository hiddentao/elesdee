//! The authenticated app path: handshake, requests, events and output.
//!
//! A paired phone opens one control stream and sends `hello`. The computer
//! answers `welcome`. Every later stream carries one unary request or one
//! stream subscription. Unary requests become in-process calls to the local
//! router, so the link and the desktop use the same contract.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::HeaderValue;
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, Method as HttpMethod, Request as HttpRequest, StatusCode};
use elesdee_link::{
    AppHello, CLOSE_NOT_PAIRED, CLOSE_REVOKED, ErrorCode, HELLO_TIMEOUT, MAX_APP_FRAME_BYTES,
    MAX_HEADER_BYTES, MAX_STREAMS_PER_CONNECTION, MAX_UI_PACKAGE_BYTES, MAX_UPLOAD_BYTES, Method,
    PROTOCOL_VERSION, Request, Response, StreamEvent, UNARY_REQUEST_TIMEOUT, Welcome,
    read_app_frame, write_app_frame,
};
use iroh::endpoint::VarInt;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tower::ServiceExt;

use super::hub::LinkHub;
use super::short_id;
use super::wire::MIN_BRIDGE_VERSION;
use crate::history::HistoryStatus;
use crate::sessions::{OutputChunk, OutputSnapshot, SnapshotFailure};
use crate::state::{AppState, AuxEvent, SessionEvent};

const MAX_DISPATCH_BYTES: usize = MAX_UI_PACKAGE_BYTES;

/// Handles the `elesdee/app/1` protocol.
#[derive(Clone)]
pub struct AppHandler {
    state: AppState,
    hub: LinkHub,
    router: Router,
}

impl std::fmt::Debug for AppHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppHandler").finish_non_exhaustive()
    }
}

impl AppHandler {
    /// Builds a handler that dispatches safe methods through the local router.
    pub fn new(state: AppState, hub: LinkHub, router: Router) -> Self {
        Self { state, hub, router }
    }
}

impl ProtocolHandler for AppHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let endpoint_id = connection.remote_id().to_z32();
        if !self.state.is_paired_device(&endpoint_id) {
            let code = if self.hub.is_revoked(&endpoint_id) {
                CLOSE_REVOKED
            } else {
                CLOSE_NOT_PAIRED
            };
            connection.close(VarInt::from_u32(code), b"not paired");
            return Ok(());
        }
        let Some(_permit) = self
            .hub
            .register_connection(&endpoint_id, connection.clone())
        else {
            connection.close(VarInt::from_u32(CLOSE_NOT_PAIRED), b"too many phones");
            return Ok(());
        };
        self.state.touch_paired_device(&endpoint_id);
        tracing::info!(phone = %short_id(&endpoint_id), "phone link opened");

        if let Err(reason) = handshake(&self.state, &connection).await {
            connection.close(VarInt::from_u32(0), reason.as_bytes());
            return Ok(());
        }

        let streams = Arc::new(Semaphore::new(MAX_STREAMS_PER_CONNECTION));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = connection.closed() => break,
                accepted = connection.accept_bi() => {
                    let Ok((send, recv)) = accepted else {
                        break;
                    };
                    let Some(stream_permit) = self.hub.acquire_stream() else {
                        let mut send = send;
                        let reply = Response::error(
                            ErrorCode::RateLimited,
                            "Too many open link streams.",
                        );
                        let _ = write_response(&mut send, &reply).await;
                        continue;
                    };
                    let Ok(permit) = Arc::clone(&streams).try_acquire_owned() else {
                        let mut send = send;
                        let reply = Response::error(
                            ErrorCode::RateLimited,
                            "Too many open streams on this connection.",
                        );
                        let _ = write_response(&mut send, &reply).await;
                        continue;
                    };
                    let router = self.router.clone();
                    let state = self.state.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _stream = stream_permit;
                        handle_stream(state, router, send, recv).await;
                    });
                }
            }
        }

        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        tracing::info!(phone = %short_id(&endpoint_id), "phone link closed");
        Ok(())
    }
}

async fn handshake(state: &AppState, connection: &Connection) -> Result<(), &'static str> {
    let accepted = tokio::time::timeout(HELLO_TIMEOUT, connection.accept_bi())
        .await
        .map_err(|_| "hello timeout")?
        .map_err(|_| "hello stream")?;
    let (mut send, mut recv) = accepted;
    let hello: AppHello = tokio::time::timeout(
        HELLO_TIMEOUT,
        read_app_frame(&mut recv, MAX_APP_FRAME_BYTES),
    )
    .await
    .map_err(|_| "hello timeout")?
    .map_err(|_| "hello invalid")?;
    if !hello.is_compatible() {
        return Err("protocol");
    }
    let welcome = welcome_for(state)?;
    tokio::time::timeout(
        HELLO_TIMEOUT,
        write_app_frame(&mut send, &welcome, MAX_APP_FRAME_BYTES),
    )
    .await
    .map_err(|_| "welcome timeout")?
    .map_err(|_| "welcome failed")?;
    let _ = send.finish();
    Ok(())
}

fn welcome_for(state: &AppState) -> Result<Welcome, &'static str> {
    let desktop = state.link().desktop().ok_or("link not ready")?;
    Ok(Welcome {
        v: PROTOCOL_VERSION,
        computer_id: desktop.id_string,
        computer_name: desktop.name,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: PROTOCOL_VERSION,
        min_bridge_version: MIN_BRIDGE_VERSION,
        ui: None,
    })
}

async fn handle_stream(
    state: AppState,
    mut router: Router,
    mut send: SendStream,
    mut recv: RecvStream,
) {
    let request = match tokio::time::timeout(UNARY_REQUEST_TIMEOUT, read_request(&mut recv)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => {
            let _ = write_response(&mut send, &error_response(error)).await;
            return;
        }
        Err(_) => {
            let reply = Response::error(ErrorCode::Internal, "The link request timed out.");
            let _ = write_response(&mut send, &reply).await;
            return;
        }
    };

    match request.method() {
        Method::EventsSubscribe => {
            let _ = stream_events(&state, &mut send, &request).await;
        }
        Method::OutputSubscribe => {
            let _ = stream_output(&state, &mut send, &request).await;
        }
        Method::HistorySubscribe => {
            let _ = stream_history(&state, &mut send, &request).await;
        }
        _ => {
            let response = dispatch(&mut router, &request).await;
            let _ = write_response(&mut send, &response).await;
        }
    }
}

fn error_response(error: RequestError) -> Response {
    match error {
        RequestError::TooLarge => {
            Response::error(ErrorCode::TooLarge, "The link request is too large.")
        }
        RequestError::Malformed => {
            Response::error(ErrorCode::BadRequest, "The link request is not valid.")
        }
        RequestError::Link(error) => Response::error(error.code(), error.to_string()),
        RequestError::Stream => {
            Response::error(ErrorCode::BadRequest, "The link request could not be read.")
        }
    }
}

enum RequestError {
    TooLarge,
    Malformed,
    Link(elesdee_link::LinkError),
    Stream,
}

async fn read_request(recv: &mut RecvStream) -> Result<Request, RequestError> {
    // Read the declared header length first. This bounds the allocation
    // before any body bytes are read.
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| RequestError::Stream)?;
    let header_length = u32::from_be_bytes(length) as usize;
    if header_length == 0 {
        return Err(RequestError::Malformed);
    }
    if header_length > MAX_HEADER_BYTES {
        return Err(RequestError::TooLarge);
    }

    let mut header = vec![0u8; header_length];
    recv.read_exact(&mut header)
        .await
        .map_err(|_| RequestError::Stream)?;

    let value: Value =
        serde_json::from_slice(&header).map_err(|error| RequestError::Link(error.into()))?;
    let body_length = match value.get("bytes") {
        None | Some(Value::Null) => 0,
        Some(value) => {
            let length = value.as_u64().ok_or(RequestError::Malformed)?;
            if length > MAX_UPLOAD_BYTES as u64 {
                return Err(RequestError::TooLarge);
            }
            length as usize
        }
    };

    let mut body = vec![0u8; body_length];
    recv.read_exact(&mut body)
        .await
        .map_err(|_| RequestError::Stream)?;

    let mut frame = Vec::with_capacity(4 + header_length + body_length);
    frame.extend_from_slice(&(header_length as u32).to_be_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    Request::decode(&frame).map_err(RequestError::Link)
}

/// Removes values that a paired phone may not choose.
///
/// A phone never chooses a job folder. A session it asks to create always
/// starts in the project folder, so any `folder` value is dropped before a
/// handler sees it.
pub fn link_params(method: Method, params: Option<&Value>) -> Option<Value> {
    let mut params = params.cloned();
    if method == Method::SessionsCreate
        && let Some(object) = params.as_mut().and_then(Value::as_object_mut)
    {
        object.remove("folder");
    }
    params
}

async fn dispatch(router: &mut Router, request: &Request) -> Response {
    match request.method() {
        Method::SessionsUpload => return dispatch_upload(router, request).await,
        Method::FilesImage => return dispatch_image(router, request).await,
        _ => {}
    }
    if !request.body().is_empty() {
        return Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
    }
    let route = match route_for(request.method(), request.params()) {
        Ok(route) => route,
        Err(response) => return response,
    };
    match call_router(router, route).await {
        Ok(reply) if reply.status.is_success() => Response::ok(json_value(&reply.body)),
        Ok(reply) => Response::error(
            code_for_status(reply.status),
            error_message(&reply.body)
                .unwrap_or_else(|| "The computer rejected the request.".to_owned()),
        ),
        Err(()) => Response::error(
            ErrorCode::Internal,
            "The computer could not answer the request.",
        ),
    }
}

/// Sends one uploaded file with its body to the local upload route.
async fn dispatch_upload(router: &mut Router, request: &Request) -> Response {
    let object = match request.params().and_then(Value::as_object) {
        Some(object) => object,
        None => return bad_request("The method parameters must be an object."),
    };
    let session_id = match session_id(object) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let filename = match required(object, "filename") {
        Ok(filename) => filename,
        Err(response) => return response,
    };
    let route = Route {
        method: HttpMethod::POST,
        path: format!("/api/sessions/{session_id}/upload"),
        body: None,
        binary: Some(request.body().to_vec()),
        headers: vec![
            (
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            ),
            (
                UPLOAD_NAME_HEADER,
                HeaderValue::from_str(&encode_query(&filename))
                    .unwrap_or_else(|_| HeaderValue::from_static("file")),
            ),
        ],
    };
    match call_router(router, route).await {
        Ok(reply) if reply.status.is_success() => Response::ok(json_value(&reply.body)),
        Ok(reply) => Response::error(
            code_for_status(reply.status),
            error_message(&reply.body)
                .unwrap_or_else(|| "The computer rejected the upload.".to_owned()),
        ),
        Err(()) => Response::error(ErrorCode::Internal, "The upload could not be stored."),
    }
}

/// Returns one image and its content type from the local image route.
async fn dispatch_image(router: &mut Router, request: &Request) -> Response {
    if !request.body().is_empty() {
        return Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
    }
    let object = match request.params().and_then(Value::as_object) {
        Some(object) => object,
        None => return bad_request("The method parameters must be an object."),
    };
    let session_id = match session_id(object) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let path = match required(object, "path") {
        Ok(path) => path,
        Err(response) => return response,
    };
    let route = get(&query_path(
        &format!("/api/sessions/{session_id}/image"),
        [("path", Some(path))],
    ));
    match call_router(router, route).await {
        Ok(reply) if reply.status.is_success() => {
            let content_type = reply
                .headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_owned();
            let size = reply.body.len();
            Response::ok_with_body(
                json!({"contentType": content_type, "size": size}),
                reply.body,
            )
        }
        Ok(reply) => Response::error(
            code_for_status(reply.status),
            error_message(&reply.body)
                .unwrap_or_else(|| "The computer rejected the image request.".to_owned()),
        ),
        Err(()) => Response::error(ErrorCode::Internal, "The image could not be read."),
    }
}

struct Route {
    method: HttpMethod,
    path: String,
    body: Option<Value>,
    binary: Option<Vec<u8>>,
    headers: Vec<(axum::http::HeaderName, HeaderValue)>,
}

const UPLOAD_NAME_HEADER: axum::http::HeaderName =
    axum::http::HeaderName::from_static("x-el-esdee-filename");

fn route_for(method: Method, params: Option<&Value>) -> Result<Route, Response> {
    let params = link_params(method, params);
    let empty = Map::new();
    let object = match params.as_ref() {
        None => &empty,
        Some(Value::Object(object)) => object,
        Some(_) => return Err(bad_request("The method parameters must be an object.")),
    };

    let route = match method {
        Method::Bootstrap => get("/api/bootstrap"),
        Method::DemoServersList => get("/api/demo-servers"),
        Method::Health => get("/api/health"),
        Method::ProjectsList => get(&query_path("/api/projects", [("q", optional(object, "q"))])),
        Method::BranchesList => get(&query_path(
            "/api/branches",
            [
                ("folder", Some(required(object, "folder")?)),
                ("q", optional(object, "q")),
            ],
        )),
        Method::ProvidersList => get("/api/providers"),
        Method::AccountsList => get(&query_path("/api/accounts", [("q", optional(object, "q"))])),
        Method::SessionsCreate => post("/api/sessions", Value::Object(object.clone())),
        Method::SessionsClose => post(
            &format!("/api/sessions/{}/close", session_id(object)?),
            json!({}),
        ),
        Method::SessionsInput => post(
            &format!("/api/sessions/{}/input", session_id(object)?),
            json!({"data": required(object, "data")?}),
        ),
        Method::SessionsPermission => {
            let decision = required(object, "decision")?;
            if decision != "allow" && decision != "deny" {
                return Err(bad_request("The permission decision is not valid."));
            }
            post(
                &format!("/api/sessions/{}/permission", session_id(object)?),
                json!({
                    "requestId": required(object, "requestId")?,
                    "decision": decision,
                }),
            )
        }
        Method::SessionsAccount => post(
            &format!("/api/sessions/{}/account", session_id(object)?),
            json!({"accountId": required(object, "accountId")?}),
        ),
        Method::SessionsConfig => post(
            &format!("/api/sessions/{}/config", session_id(object)?),
            json!({"configId": required(object, "configId")?}),
        ),
        Method::SessionsResume => post(
            &format!("/api/sessions/{}/resume", session_id(object)?),
            json!({}),
        ),
        Method::QuestionsList => get("/api/questions"),
        Method::QuestionsAnswer => post(
            &format!("/api/questions/{}/answer", id_segment(object, "id")?),
            json!({"answer": required(object, "answer")?}),
        ),
        Method::SessionRequestsApprove => {
            let allow = object
                .get("allowFurther")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            post(
                &format!(
                    "/api/session-requests/{}/approve",
                    id_segment(object, "id")?
                ),
                json!({"allowFurther": allow}),
            )
        }
        Method::SessionRequestsDeny => post(
            &format!("/api/session-requests/{}/deny", id_segment(object, "id")?),
            json!({}),
        ),
        Method::FilesList => get(&query_path(
            &format!("/api/sessions/{}/files", session_id(object)?),
            [("path", optional(object, "path"))],
        )),
        Method::FilesRead => get(&query_path(
            &format!("/api/sessions/{}/file", session_id(object)?),
            [("path", Some(required(object, "path")?))],
        )),
        Method::DiffRead => get(&format!("/api/sessions/{}/changes", session_id(object)?)),
        Method::SessionsUpload | Method::FilesImage => {
            return Err(bad_request(
                "This method is handled with a body or an image, not a plain route.",
            ));
        }
        Method::UiManifest | Method::UiDownload => {
            return Err(Response::error(
                ErrorCode::Unsupported,
                "This method is not available in this build.",
            ));
        }
        Method::EventsSubscribe | Method::OutputSubscribe | Method::HistorySubscribe => {
            return Err(Response::error(
                ErrorCode::BadRequest,
                "This method needs a stream.",
            ));
        }
    };
    Ok(route)
}

fn get(path: &str) -> Route {
    Route {
        method: HttpMethod::GET,
        path: path.to_owned(),
        body: None,
        binary: None,
        headers: Vec::new(),
    }
}

fn post(path: &str, body: Value) -> Route {
    Route {
        method: HttpMethod::POST,
        path: path.to_owned(),
        body: Some(body),
        binary: None,
        headers: Vec::new(),
    }
}

fn bad_request(message: impl Into<String>) -> Response {
    Response::error(ErrorCode::BadRequest, message)
}

fn required(object: &Map<String, Value>, key: &str) -> Result<String, Response> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(bad_request(format!("The {key} parameter is required."))),
    }
}

fn optional(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn session_id(object: &Map<String, Value>) -> Result<String, Response> {
    id_segment(object, "sessionId")
}

fn id_segment(object: &Map<String, Value>, key: &str) -> Result<String, Response> {
    let value = required(object, key)?;
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        Ok(value)
    } else {
        Err(bad_request(format!("The {key} parameter is not valid.")))
    }
}

fn query_path<const N: usize>(base: &str, pairs: [(&str, Option<String>); N]) -> String {
    let query = pairs
        .into_iter()
        .filter_map(|(name, value)| {
            value.map(|value| format!("{}={}", encode_query(name), encode_query(&value)))
        })
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{query}")
    }
}

fn encode_query(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

struct RouterReply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

async fn call_router(router: &mut Router, route: Route) -> Result<RouterReply, ()> {
    let mut builder = HttpRequest::builder()
        .method(route.method)
        .uri(route.path)
        .header(HOST, "127.0.0.1");
    for (name, value) in route.headers {
        builder = builder.header(name, value);
    }
    let body = if let Some(bytes) = route.binary {
        Body::from(bytes)
    } else if let Some(value) = route.body {
        builder = builder.header(CONTENT_TYPE, "application/json");
        Body::from(value.to_string())
    } else {
        Body::empty()
    };
    let request = builder.body(body).map_err(|_| ())?;
    let response = router.clone().oneshot(request).await.map_err(|_| ())?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), MAX_DISPATCH_BYTES)
        .await
        .map_err(|_| ())?;
    Ok(RouterReply {
        status,
        headers,
        body: bytes.to_vec(),
    })
}

fn json_value(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    }
}

fn error_message(bytes: &[u8]) -> Option<String> {
    json_value(bytes)
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn code_for_status(status: StatusCode) -> ErrorCode {
    match status {
        StatusCode::BAD_REQUEST => ErrorCode::BadRequest,
        StatusCode::FORBIDDEN => ErrorCode::Forbidden,
        StatusCode::NOT_FOUND => ErrorCode::NotFound,
        StatusCode::CONFLICT => ErrorCode::Conflict,
        StatusCode::PAYLOAD_TOO_LARGE => ErrorCode::TooLarge,
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ErrorCode::Unsupported,
        StatusCode::UNPROCESSABLE_ENTITY => ErrorCode::Unprocessable,
        StatusCode::TOO_MANY_REQUESTS => ErrorCode::RateLimited,
        StatusCode::REQUEST_TIMEOUT => ErrorCode::Internal,
        _ => ErrorCode::Internal,
    }
}

async fn stream_events(
    state: &AppState,
    send: &mut SendStream,
    request: &Request,
) -> Result<(), ()> {
    if !request.body().is_empty() {
        let reply = Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
        return write_response(send, &reply).await;
    }
    if let Some(params) = request.params()
        && !params.as_object().is_some_and(|object| object.is_empty())
    {
        let reply = Response::error(
            ErrorCode::BadRequest,
            "The event stream takes no parameters.",
        );
        return write_response(send, &reply).await;
    }
    write_frame(send, &Response::stream_start()).await?;

    let mut events = state.subscribe_events();
    let mut aux = state.subscribe_aux();
    loop {
        let value = tokio::select! {
            received = events.recv() => match received {
                Ok(event) => session_event_value(&event),
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            received = aux.recv() => match received {
                Ok(event) => aux_event_value(&event),
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            },
        };
        if let Some(value) = value {
            write_event(send, &value).await?;
        }
    }
    let _ = send.finish();
    Ok(())
}

fn session_event_value(event: &SessionEvent) -> Option<Value> {
    serde_json::to_value(event).ok()
}

fn aux_event_value(event: &AuxEvent) -> Option<Value> {
    serde_json::to_value(event).ok()
}

async fn stream_output(
    state: &AppState,
    send: &mut SendStream,
    request: &Request,
) -> Result<(), ()> {
    if !request.body().is_empty() {
        let reply = Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
        return write_response(send, &reply).await;
    }
    let Some(params) = request.params().and_then(Value::as_object) else {
        let reply = Response::error(
            ErrorCode::BadRequest,
            "The output stream needs a session id.",
        );
        return write_response(send, &reply).await;
    };
    let (session_id, after) = match stream_params(params, "output") {
        Ok(params) => params,
        Err(reply) => return write_response(send, &reply).await,
    };

    let Some(entry) = state.session(&session_id) else {
        let reply = Response::error(ErrorCode::NotFound, "Unknown session.");
        return write_response(send, &reply).await;
    };
    let Some(_permit) = state.try_acquire_output_sse() else {
        let reply = Response::error(ErrorCode::RateLimited, "Too many open output streams.");
        return write_response(send, &reply).await;
    };
    let mut receiver = entry.subscribe_output();
    let mut close_receiver = entry.subscribe_close();
    let snapshot = match entry.output_snapshot(after) {
        Ok(snapshot) => snapshot,
        Err(SnapshotFailure::AfterAhead { newest }) => {
            let reply = Response::error(
                ErrorCode::BadRequest,
                format!("The output position is beyond the newest sequence ({newest})."),
            );
            return write_response(send, &reply).await;
        }
        Err(SnapshotFailure::LogUnavailable) => {
            let reply = Response::error(
                ErrorCode::BadRequest,
                "Output at that position is no longer retained.",
            );
            return write_response(send, &reply).await;
        }
    };

    write_frame(send, &Response::stream_start()).await?;
    let mut last_seq = snapshot.newest_seq;
    for chunk in snapshot.chunks {
        write_event(send, &output_value(&chunk)).await?;
    }

    if !*close_receiver.borrow_and_update() {
        loop {
            tokio::select! {
                changed = close_receiver.changed() => {
                    let _ = changed;
                    break;
                }
                received = receiver.recv() => {
                    match received {
                        Ok(chunk) if chunk.seq > last_seq => {
                            last_seq = chunk.seq;
                            write_event(send, &output_value(&chunk)).await?;
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let snapshot: OutputSnapshot = match entry.output_snapshot(last_seq) {
                                Ok(snapshot) => snapshot,
                                Err(_) => break,
                            };
                            last_seq = snapshot.newest_seq;
                            for chunk in snapshot.chunks {
                                write_event(send, &output_value(&chunk)).await?;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    }
    let _ = send.finish();
    Ok(())
}

fn output_value(chunk: &OutputChunk) -> Value {
    serde_json::to_value(chunk).unwrap_or(Value::Null)
}

/// Reads the session id and replay position shared by the two session streams.
fn stream_params(params: &Map<String, Value>, label: &str) -> Result<(String, u64), Response> {
    let Some(session_id) = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Err(Response::error(
            ErrorCode::BadRequest,
            format!("The {label} stream needs a session id."),
        ));
    };
    let after = match params.get("after") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or_else(|| {
            Response::error(
                ErrorCode::BadRequest,
                "The replay position must be a non-negative integer.",
            )
        })?,
    };
    Ok((session_id.to_owned(), after))
}

async fn stream_history(
    state: &AppState,
    send: &mut SendStream,
    request: &Request,
) -> Result<(), ()> {
    if !request.body().is_empty() {
        let reply = Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
        return write_response(send, &reply).await;
    }
    let Some(params) = request.params().and_then(Value::as_object) else {
        let reply = Response::error(
            ErrorCode::BadRequest,
            "The history stream needs a session id.",
        );
        return write_response(send, &reply).await;
    };
    let (session_id, after) = match stream_params(params, "history") {
        Ok(params) => params,
        Err(reply) => return write_response(send, &reply).await,
    };

    let Some(entry) = state.session(&session_id) else {
        let reply = Response::error(ErrorCode::NotFound, "Unknown session.");
        return write_response(send, &reply).await;
    };
    let Some(_permit) = state.try_acquire_events_sse() else {
        let reply = Response::error(ErrorCode::RateLimited, "Too many open event streams.");
        return write_response(send, &reply).await;
    };

    let mut receiver = state.subscribe_aux();
    let mut close_receiver = entry.subscribe_close();
    let status = HistoryStatus::new(
        entry.history_supported(),
        entry
            .session()
            .client
            .map(|client| client.display_name().to_owned()),
    );
    let snapshot = match entry.history_snapshot(after) {
        Ok(snapshot) => snapshot,
        Err(SnapshotFailure::AfterAhead { newest }) => {
            let reply = Response::error(
                ErrorCode::BadRequest,
                format!("The history position is beyond the newest sequence ({newest})."),
            );
            return write_response(send, &reply).await;
        }
        Err(SnapshotFailure::LogUnavailable) => {
            let reply = Response::error(
                ErrorCode::BadRequest,
                "History at that position is no longer retained.",
            );
            return write_response(send, &reply).await;
        }
    };

    write_frame(send, &Response::stream_start()).await?;
    write_event(send, &serde_json::to_value(&status).unwrap_or(Value::Null)).await?;
    let mut last_seq = snapshot.newest_seq;
    for item in snapshot.items {
        write_event(send, &serde_json::to_value(&item).unwrap_or(Value::Null)).await?;
    }

    if !*close_receiver.borrow_and_update() {
        loop {
            tokio::select! {
                changed = close_receiver.changed() => {
                    let _ = changed;
                    break;
                }
                received = receiver.recv() => {
                    match received {
                        Ok(AuxEvent::History { session_id: event_session, item })
                            if event_session == session_id =>
                        {
                            if item.seq > last_seq {
                                last_seq = item.seq;
                                write_event(
                                    send,
                                    &serde_json::to_value(&item).unwrap_or(Value::Null),
                                )
                                .await?;
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let Ok(snapshot) = entry.history_snapshot(last_seq) else {
                                break;
                            };
                            last_seq = snapshot.newest_seq;
                            for item in snapshot.items {
                                write_event(
                                    send,
                                    &serde_json::to_value(&item).unwrap_or(Value::Null),
                                )
                                .await?;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    }
    let _ = send.finish();
    Ok(())
}

async fn write_frame(send: &mut SendStream, response: &Response) -> Result<(), ()> {
    let encoded = match response.encode() {
        Ok(encoded) => encoded,
        Err(error) => Response::error(error.code(), error.to_string())
            .encode()
            .map_err(|_| ())?,
    };
    send.write_all(&encoded).await.map_err(|_| ())
}

async fn write_event(send: &mut SendStream, value: &Value) -> Result<(), ()> {
    let encoded = StreamEvent::new(value.clone()).encode().map_err(|_| ())?;
    send.write_all(&encoded).await.map_err(|_| ())
}

async fn write_response(send: &mut SendStream, response: &Response) -> Result<(), ()> {
    write_frame(send, response).await?;
    let _ = send.finish();
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn session_create_drops_the_remote_folder() {
        let params = json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": "/Users/someone",
        });
        let cleaned = link_params(Method::SessionsCreate, Some(&params)).expect("params");
        assert!(cleaned.get("folder").is_none());
        assert_eq!(cleaned["projectId"], "web");
        assert_eq!(cleaned["kind"], "terminal");
    }

    #[test]
    fn other_methods_keep_their_params() {
        let params = json!({"folder": "kept"});
        let cleaned = link_params(Method::ProjectsList, Some(&params)).expect("params");
        assert_eq!(cleaned["folder"], "kept");
        assert!(link_params(Method::Bootstrap, None).is_none());
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(encode_query("hello world"), "hello%20world");
        assert_eq!(encode_query("a/b?c=d"), "a%2Fb%3Fc%3Dd");
        assert_eq!(encode_query("safe-name_1.txt"), "safe-name_1.txt");
    }

    #[test]
    fn a_session_create_route_drops_the_folder() {
        let params = json!({
            "projectId": "web",
            "kind": "terminal",
            "folder": "/Users/someone",
        });
        let route = route_for(Method::SessionsCreate, Some(&params)).expect("route");
        let body = route.body.expect("body");
        assert!(body.get("folder").is_none());
        assert_eq!(body["projectId"], "web");
    }

    #[test]
    fn a_route_uses_the_local_path_and_body() {
        let params = json!({"sessionId": "s1", "data": "ls\n"});
        let route = route_for(Method::SessionsInput, Some(&params)).expect("route");
        assert_eq!(route.method, HttpMethod::POST);
        assert_eq!(route.path, "/api/sessions/s1/input");
        assert_eq!(route.body, Some(json!({"data": "ls\n"})));
    }

    #[test]
    fn a_config_switch_route_uses_the_config_path() {
        let params = json!({"sessionId": "s1", "configId": "deepseek-opencode"});
        let route = route_for(Method::SessionsConfig, Some(&params)).expect("route");
        assert_eq!(route.method, HttpMethod::POST);
        assert_eq!(route.path, "/api/sessions/s1/config");
        assert_eq!(route.body, Some(json!({"configId": "deepseek-opencode"})));
    }

    #[test]
    fn an_unsafe_session_id_is_rejected() {
        let params = json!({"sessionId": "../state"});
        let error = match route_for(Method::SessionsClose, Some(&params)) {
            Ok(_) => panic!("the route was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.error_body().expect("error body").code(),
            ErrorCode::BadRequest
        );
    }

    #[test]
    fn the_demo_server_list_is_a_read_only_route() {
        let route = route_for(Method::DemoServersList, None).expect("route");
        assert_eq!(route.method, HttpMethod::GET);
        assert_eq!(route.path, "/api/demo-servers");
        assert!(route.body.is_none());
    }

    #[test]
    fn an_unbuilt_method_is_unsupported() {
        let error = match route_for(Method::UiManifest, None) {
            Ok(_) => panic!("the route was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.error_body().expect("error body").code(),
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn the_upload_and_image_methods_need_their_own_dispatch() {
        for method in [Method::SessionsUpload, Method::FilesImage] {
            let error = match route_for(method, None) {
                Ok(_) => panic!("{method} produced a plain route"),
                Err(error) => error,
            };
            assert_eq!(
                error.error_body().expect("error body").code(),
                ErrorCode::BadRequest
            );
        }
    }

    #[test]
    fn a_file_name_is_escaped_before_it_becomes_a_header() {
        assert_eq!(encode_query("shot one.png"), "shot%20one.png");
        assert_eq!(
            encode_query("\u{0444}\u{043e}\u{0442}\u{043e}.png"),
            "%D1%84%D0%BE%D1%82%D0%BE.png"
        );
        assert_eq!(encode_query("..%2Fevil.png"), "..%252Fevil.png");
    }
}
