//! Framing and envelopes for requests, responses and stream events.
//!
//! One bidirectional stream carries one request and its response. A frame is a
//! four-byte big-endian JSON length, the JSON header and an optional binary
//! body.
//!
//! ```text
//! 4-byte big-endian JSON length | JSON header | optional binary body
//! ```

use serde::Serialize;
use serde_json::Value;

use crate::limits::{MAX_HEADER_BYTES, MAX_UI_PACKAGE_BYTES, MAX_UPLOAD_BYTES};
use crate::method::Method;
use crate::{ErrorBody, ErrorCode, LinkError, PROTOCOL_VERSION};

/// A request for one method.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    method: Method,
    params: Option<Value>,
    body: Vec<u8>,
}

impl Request {
    /// Creates a request with optional JSON parameters and no binary body.
    pub fn new(method: Method, params: Option<Value>) -> Self {
        Self {
            method,
            params,
            body: Vec::new(),
        }
    }

    /// Attaches a binary body.
    ///
    /// The body limit is checked when the request is encoded.
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Returns the requested method.
    pub fn method(&self) -> Method {
        self.method
    }

    /// Returns the JSON parameters, when present.
    pub fn params(&self) -> Option<&Value> {
        self.params.as_ref()
    }

    /// Returns the binary body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Encodes the request as one frame.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::BodyTooLarge`] when the body is over the upload
    /// limit, and [`LinkError::HeaderTooLarge`] when the JSON header is over
    /// the header limit.
    pub fn encode(&self) -> Result<Vec<u8>, LinkError> {
        if self.body.len() > MAX_UPLOAD_BYTES {
            return Err(LinkError::BodyTooLarge {
                len: self.body.len(),
                max: MAX_UPLOAD_BYTES,
            });
        }
        let header = RequestHeader {
            v: PROTOCOL_VERSION,
            method: self.method.as_str(),
            params: self.params.as_ref(),
            bytes: declared_len(self.body.len()),
        };
        encode_frame(&header, &self.body)
    }

    /// Decodes one request frame.
    ///
    /// # Errors
    ///
    /// Rejects an unsupported version, an unknown method, a malformed frame, a
    /// header or body over the limit, and bytes past the declared body.
    pub fn decode(frame: &[u8]) -> Result<Self, LinkError> {
        let (header_bytes, body_bytes) = split_frame(frame)?;
        let header: RawRequest = serde_json::from_slice(header_bytes)?;
        if header.v != PROTOCOL_VERSION {
            return Err(LinkError::UnsupportedVersion(header.v));
        }
        check_body(header.bytes, body_bytes, MAX_UPLOAD_BYTES)?;
        let method = header.method.parse::<Method>()?;
        Ok(Self {
            method,
            params: header.params,
            body: body_bytes.to_vec(),
        })
    }
}

#[derive(Serialize)]
struct RequestHeader<'a> {
    v: u8,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u32>,
}

#[derive(serde::Deserialize)]
struct RawRequest {
    v: u8,
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    bytes: Option<u32>,
}

/// A response to one request.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// A unary result and an optional binary body.
    Ok {
        /// The JSON result.
        result: Value,
        /// The binary body, such as a UI package.
        body: Vec<u8>,
    },
    /// The first message of a streaming response.
    StreamStart,
    /// A failure.
    Error(ErrorBody),
}

impl Response {
    /// Creates a unary success response with no binary body.
    pub fn ok(result: Value) -> Self {
        Self::Ok {
            result,
            body: Vec::new(),
        }
    }

    /// Creates a unary success response with a binary body.
    pub fn ok_with_body(result: Value, body: Vec<u8>) -> Self {
        Self::Ok { result, body }
    }

    /// Creates a failure response.
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Error(ErrorBody::new(code, message))
    }

    /// Creates the first message of a streaming response.
    pub const fn stream_start() -> Self {
        Self::StreamStart
    }

    /// Returns the JSON result of a unary success.
    pub fn result(&self) -> Option<&Value> {
        match self {
            Self::Ok { result, .. } => Some(result),
            Self::StreamStart | Self::Error(_) => None,
        }
    }

    /// Returns the binary body.
    pub fn body(&self) -> &[u8] {
        match self {
            Self::Ok { body, .. } => body,
            Self::StreamStart | Self::Error(_) => &[],
        }
    }

    /// Returns the error object of a failure.
    pub fn error_body(&self) -> Option<&ErrorBody> {
        match self {
            Self::Error(error) => Some(error),
            Self::Ok { .. } | Self::StreamStart => None,
        }
    }

    /// Encodes the response as one frame.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::BodyTooLarge`] when the body is over the UI package
    /// limit, and [`LinkError::HeaderTooLarge`] when the JSON header is over
    /// the header limit.
    pub fn encode(&self) -> Result<Vec<u8>, LinkError> {
        match self {
            Self::Ok { result, body } => {
                if body.len() > MAX_UI_PACKAGE_BYTES {
                    return Err(LinkError::BodyTooLarge {
                        len: body.len(),
                        max: MAX_UI_PACKAGE_BYTES,
                    });
                }
                let header = ResponseHeader {
                    v: PROTOCOL_VERSION,
                    ok: true,
                    stream: false,
                    result: Some(result),
                    error: None,
                    bytes: declared_len(body.len()),
                };
                encode_frame(&header, body)
            }
            Self::StreamStart => {
                let header = ResponseHeader {
                    v: PROTOCOL_VERSION,
                    ok: true,
                    stream: true,
                    result: None,
                    error: None,
                    bytes: None,
                };
                encode_frame(&header, &[])
            }
            Self::Error(error) => {
                let header = ResponseHeader {
                    v: PROTOCOL_VERSION,
                    ok: false,
                    stream: false,
                    result: None,
                    error: Some(error),
                    bytes: None,
                };
                encode_frame(&header, &[])
            }
        }
    }

    /// Decodes one response frame.
    ///
    /// # Errors
    ///
    /// Rejects an unsupported version, an inconsistent envelope, a malformed
    /// frame, and a header or body over the limit.
    pub fn decode(frame: &[u8]) -> Result<Self, LinkError> {
        let (header_bytes, body_bytes) = split_frame(frame)?;
        let header: RawResponse = serde_json::from_slice(header_bytes)?;
        if header.v != PROTOCOL_VERSION {
            return Err(LinkError::UnsupportedVersion(header.v));
        }
        check_body(header.bytes, body_bytes, MAX_UI_PACKAGE_BYTES)?;
        let body = body_bytes.to_vec();

        if header.ok {
            if header.stream {
                if header.result.is_some() || header.error.is_some() || !body.is_empty() {
                    return Err(LinkError::MalformedHeader(
                        "stream start carries a result, error or body",
                    ));
                }
                return Ok(Self::StreamStart);
            }
            if header.error.is_some() {
                return Err(LinkError::MalformedHeader("success carries an error"));
            }
            let result = header
                .result
                .ok_or(LinkError::MalformedHeader("success carries no result"))?;
            return Ok(Self::Ok { result, body });
        }

        if header.stream || !body.is_empty() || header.result.is_some() {
            return Err(LinkError::MalformedHeader(
                "failure carries a stream, result or body",
            ));
        }
        let error = header
            .error
            .ok_or(LinkError::MalformedHeader("failure carries no error"))?;
        Ok(Self::Error(error))
    }
}

#[derive(Serialize)]
struct ResponseHeader<'a> {
    v: u8,
    ok: bool,
    #[serde(skip_serializing_if = "is_false")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a ErrorBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u32>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(serde::Deserialize)]
struct RawResponse {
    v: u8,
    ok: bool,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ErrorBody>,
    #[serde(default)]
    bytes: Option<u32>,
}

/// One JSON event in a streaming response.
///
/// The first frame of a stream is [`Response::StreamStart`]. Each later frame
/// is a stream event, with no binary body.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamEvent(Value);

impl StreamEvent {
    /// Creates an event from a JSON value.
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Returns the event value.
    pub fn value(&self) -> &Value {
        &self.0
    }

    /// Consumes the event and returns its value.
    pub fn into_value(self) -> Value {
        self.0
    }

    /// Encodes the event as one frame.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::HeaderTooLarge`] when the JSON header is over the
    /// header limit.
    pub fn encode(&self) -> Result<Vec<u8>, LinkError> {
        encode_frame(&self.0, &[])
    }

    /// Decodes one stream event frame.
    ///
    /// # Errors
    ///
    /// Rejects a malformed frame, an oversized header and any binary body.
    pub fn decode(frame: &[u8]) -> Result<Self, LinkError> {
        let (header_bytes, body_bytes) = split_frame(frame)?;
        let value: Value = serde_json::from_slice(header_bytes)?;
        if !body_bytes.is_empty() {
            return Err(LinkError::TrailingBytes(body_bytes.len()));
        }
        Ok(Self(value))
    }
}

fn declared_len(len: usize) -> Option<u32> {
    if len == 0 { None } else { Some(len as u32) }
}

fn encode_frame<T: Serialize>(header: &T, body: &[u8]) -> Result<Vec<u8>, LinkError> {
    let json = serde_json::to_vec(header)?;
    if json.len() > MAX_HEADER_BYTES {
        return Err(LinkError::HeaderTooLarge {
            len: json.len(),
            max: MAX_HEADER_BYTES,
        });
    }
    let mut frame = Vec::with_capacity(4 + json.len() + body.len());
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(body);
    Ok(frame)
}

fn split_frame(frame: &[u8]) -> Result<(&[u8], &[u8]), LinkError> {
    if frame.len() < 4 {
        return Err(LinkError::Truncated);
    }
    let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if header_len > MAX_HEADER_BYTES {
        return Err(LinkError::HeaderTooLarge {
            len: header_len,
            max: MAX_HEADER_BYTES,
        });
    }
    let header_end = 4 + header_len;
    let header = frame.get(4..header_end).ok_or(LinkError::Truncated)?;
    Ok((header, &frame[header_end..]))
}

fn check_body(declared: Option<u32>, actual: &[u8], max: usize) -> Result<(), LinkError> {
    let declared = declared.map_or(0, |len| len as usize);
    if declared > max {
        return Err(LinkError::BodyTooLarge { len: declared, max });
    }
    if actual.len() < declared {
        return Err(LinkError::Truncated);
    }
    if actual.len() > declared {
        return Err(LinkError::TrailingBytes(actual.len() - declared));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn header_json(frame: &[u8]) -> Value {
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        serde_json::from_slice(&frame[4..4 + len]).expect("header is JSON")
    }

    fn raw_frame(header: Value, body: &[u8]) -> Vec<u8> {
        let json = serde_json::to_vec(&header).expect("header encodes");
        let mut frame = Vec::new();
        frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
        frame.extend_from_slice(&json);
        frame.extend_from_slice(body);
        frame
    }

    #[test]
    fn request_round_trip_with_params_and_body() {
        let request = Request::new(
            Method::SessionsInput,
            Some(json!({"sessionId": "s1", "data": "ls\n"})),
        )
        .with_body(b"payload".to_vec());

        let frame = request.encode().expect("request encodes");
        assert_eq!(
            header_json(&frame),
            json!({
                "v": 1,
                "method": "sessions.input",
                "params": {"sessionId": "s1", "data": "ls\n"},
                "bytes": 7,
            })
        );
        assert_eq!(Request::decode(&frame).expect("request decodes"), request);
    }

    #[test]
    fn request_without_params_has_no_params_field() {
        let request = Request::new(Method::Bootstrap, None);
        let frame = request.encode().expect("request encodes");
        assert_eq!(header_json(&frame), json!({"v": 1, "method": "bootstrap"}));
        assert_eq!(Request::decode(&frame).expect("request decodes"), request);
    }

    #[test]
    fn success_response_round_trip() {
        let response = Response::ok(json!({"sessions": []}));
        let frame = response.encode().expect("response encodes");
        assert_eq!(
            header_json(&frame),
            json!({"v": 1, "ok": true, "result": {"sessions": []}})
        );
        assert_eq!(
            Response::decode(&frame).expect("response decodes"),
            response
        );
    }

    #[test]
    fn success_response_with_body_round_trip() {
        let response = Response::ok_with_body(json!({"buildId": "blake3:x"}), vec![1, 2, 3]);
        let frame = response.encode().expect("response encodes");
        assert_eq!(
            header_json(&frame),
            json!({"v": 1, "ok": true, "result": {"buildId": "blake3:x"}, "bytes": 3})
        );
        assert_eq!(
            Response::decode(&frame).expect("response decodes"),
            response
        );
    }

    #[test]
    fn failure_response_round_trip() {
        let response = Response::error(ErrorCode::Conflict, "session already closed");
        let frame = response.encode().expect("response encodes");
        assert_eq!(
            header_json(&frame),
            json!({
                "v": 1,
                "ok": false,
                "error": {"code": "conflict", "message": "session already closed"},
            })
        );
        assert_eq!(
            Response::decode(&frame).expect("response decodes"),
            response
        );
    }

    #[test]
    fn stream_start_round_trip() {
        let response = Response::stream_start();
        let frame = response.encode().expect("response encodes");
        assert_eq!(
            header_json(&frame),
            json!({"v": 1, "ok": true, "stream": true})
        );
        assert_eq!(
            Response::decode(&frame).expect("response decodes"),
            response
        );
    }

    #[test]
    fn stream_event_round_trip() {
        let event = StreamEvent::new(json!({"type": "output", "seq": 42}));
        let frame = event.encode().expect("event encodes");
        assert_eq!(StreamEvent::decode(&frame).expect("event decodes"), event);
    }

    #[test]
    fn unknown_method_is_rejected() {
        let frame = raw_frame(json!({"v": 1, "method": "projects.create"}), &[]);
        assert!(matches!(
            Request::decode(&frame),
            Err(LinkError::UnknownMethod(ref name)) if name == "projects.create"
        ));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let frame = raw_frame(json!({"v": 2, "method": "bootstrap"}), &[]);
        assert!(matches!(
            Request::decode(&frame),
            Err(LinkError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn oversized_header_is_rejected() {
        let mut frame = ((MAX_HEADER_BYTES + 1) as u32).to_be_bytes().to_vec();
        frame.push(b'{');
        assert!(matches!(
            Request::decode(&frame),
            Err(LinkError::HeaderTooLarge { len, .. }) if len == MAX_HEADER_BYTES + 1
        ));
    }

    #[test]
    fn oversized_request_body_is_rejected() {
        let frame = raw_frame(
            json!({
                "v": 1,
                "method": "sessions.upload",
                "bytes": MAX_UPLOAD_BYTES + 1,
            }),
            &[],
        );
        assert!(matches!(
            Request::decode(&frame),
            Err(LinkError::BodyTooLarge { len, .. }) if len == MAX_UPLOAD_BYTES + 1
        ));
    }

    #[test]
    fn oversized_response_body_is_rejected() {
        let frame = raw_frame(
            json!({
                "v": 1,
                "ok": true,
                "result": {},
                "bytes": MAX_UI_PACKAGE_BYTES + 1,
            }),
            &[],
        );
        assert!(matches!(
            Response::decode(&frame),
            Err(LinkError::BodyTooLarge { len, .. }) if len == MAX_UI_PACKAGE_BYTES + 1
        ));
    }

    #[test]
    fn oversized_request_header_on_encode_is_rejected() {
        let request = Request::new(
            Method::SessionsInput,
            Some(json!({"data": "x".repeat(MAX_HEADER_BYTES)})),
        );
        assert!(matches!(
            request.encode(),
            Err(LinkError::HeaderTooLarge { .. })
        ));
    }

    #[test]
    fn oversized_request_body_on_encode_is_rejected() {
        let request =
            Request::new(Method::SessionsUpload, None).with_body(vec![0; MAX_UPLOAD_BYTES + 1]);
        assert!(matches!(
            request.encode(),
            Err(LinkError::BodyTooLarge { .. })
        ));
    }

    #[test]
    fn truncated_frames_are_rejected() {
        assert!(matches!(Request::decode(&[]), Err(LinkError::Truncated)));
        assert!(matches!(
            Request::decode(&[0, 0]),
            Err(LinkError::Truncated)
        ));

        let mut frame = 10u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"{}");
        assert!(matches!(Request::decode(&frame), Err(LinkError::Truncated)));
    }

    #[test]
    fn body_length_mismatch_is_rejected() {
        let frame = raw_frame(
            json!({"v": 1, "method": "sessions.input", "bytes": 2}),
            b"abc",
        );
        assert!(matches!(
            Request::decode(&frame),
            Err(LinkError::TrailingBytes(1))
        ));

        let frame = raw_frame(
            json!({"v": 1, "method": "sessions.input", "bytes": 4}),
            b"abc",
        );
        assert!(matches!(Request::decode(&frame), Err(LinkError::Truncated)));
    }

    #[test]
    fn inconsistent_success_envelope_is_rejected() {
        let frame = raw_frame(json!({"v": 1, "ok": true}), &[]);
        assert!(matches!(
            Response::decode(&frame),
            Err(LinkError::MalformedHeader(_))
        ));
    }

    #[test]
    fn inconsistent_failure_envelope_is_rejected() {
        let frame = raw_frame(
            json!({"v": 1, "ok": false, "error": {"code": "conflict", "message": "x"}, "result": {}}),
            &[],
        );
        assert!(matches!(
            Response::decode(&frame),
            Err(LinkError::MalformedHeader(_))
        ));
    }
}
