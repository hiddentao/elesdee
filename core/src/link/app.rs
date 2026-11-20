//! The authenticated app path: one request and response per stream.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::HOST;
use axum::http::{Method as HttpMethod, Request as HttpRequest, StatusCode};
use elesdee_link::{
    CLOSE_NOT_PAIRED, CLOSE_REVOKED, ErrorCode, MAX_HEADER_BYTES, MAX_STREAMS_PER_CONNECTION,
    MAX_UPLOAD_BYTES, Method, Request, Response, UNARY_REQUEST_TIMEOUT,
};
use iroh::endpoint::VarInt;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tower::ServiceExt;

use super::hub::LinkHub;
use super::short_id;
use crate::state::AppState;

const MAX_DISPATCH_BYTES: usize = 16 * 1024 * 1024;

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
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _stream = stream_permit;
                        handle_stream(router, send, recv).await;
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

async fn handle_stream(mut router: Router, mut send: SendStream, mut recv: RecvStream) {
    let response = match tokio::time::timeout(UNARY_REQUEST_TIMEOUT, read_request(&mut recv)).await
    {
        Ok(Ok(request)) => dispatch(&mut router, &request).await,
        Ok(Err(error)) => error_response(error),
        Err(_) => Response::error(ErrorCode::Internal, "The link request timed out."),
    };
    let _ = write_response(&mut send, &response).await;
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

    let value: serde_json::Value =
        serde_json::from_slice(&header).map_err(|error| RequestError::Link(error.into()))?;
    let body_length = match value.get("bytes") {
        None | Some(serde_json::Value::Null) => 0,
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
pub fn link_params(
    method: Method,
    params: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let mut params = params.cloned();
    if method == Method::SessionsCreate
        && let Some(object) = params.as_mut().and_then(serde_json::Value::as_object_mut)
    {
        object.remove("folder");
    }
    params
}

async fn dispatch(router: &mut Router, request: &Request) -> Response {
    match request.method() {
        Method::SessionsCreate => {
            // The method is not available yet. The sanitized parameters are
            // what a future handler must use, and they never carry a folder.
            let _params = link_params(request.method(), request.params());
            Response::error(
                ErrorCode::Unsupported,
                "Remote session creation is not available in this build.",
            )
        }
        Method::Bootstrap => {
            if !request.body().is_empty() {
                return Response::error(ErrorCode::BadRequest, "This method takes no binary body.");
            }
            match call_router(router, "/api/bootstrap").await {
                Ok((status, value)) if status.is_success() => Response::ok(value),
                Ok((status, _)) => Response::error(
                    code_for_status(status),
                    "The computer rejected the request.",
                ),
                Err(()) => Response::error(
                    ErrorCode::Internal,
                    "The computer could not answer the request.",
                ),
            }
        }
        _ => Response::error(
            ErrorCode::Unsupported,
            "This method is not available in this build.",
        ),
    }
}

async fn call_router(
    router: &mut Router,
    path: &str,
) -> Result<(StatusCode, serde_json::Value), ()> {
    let request = HttpRequest::builder()
        .method(HttpMethod::GET)
        .uri(path)
        .header(HOST, "127.0.0.1")
        .body(Body::empty())
        .map_err(|_| ())?;
    let response = router.clone().oneshot(request).await.map_err(|_| ())?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), MAX_DISPATCH_BYTES)
        .await
        .map_err(|_| ())?;
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    Ok((status, value))
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
        _ => ErrorCode::Internal,
    }
}

async fn write_response(send: &mut SendStream, response: &Response) -> Result<(), ()> {
    let encoded = match response.encode() {
        Ok(encoded) => encoded,
        Err(error) => Response::error(error.code(), error.to_string())
            .encode()
            .map_err(|_| ())?,
    };
    send.write_all(&encoded).await.map_err(|_| ())?;
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
}
