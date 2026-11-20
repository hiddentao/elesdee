//! The phone client against a small test endpoint on loopback.
//!
//! The server implements the pair and app ALPNs directly. It does not use the
//! desktop core, so the test covers the client and the protocol only.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use elesdee_link::{
    APP_ALPN, AppHello, ComputerRecord, DeviceIdentity, LinkClient, MAX_APP_FRAME_BYTES,
    MAX_HEADER_BYTES, MAX_PAIR_FRAME_BYTES, Method, PAIR_ALPN, PROTOCOL_VERSION, PairReply,
    PairTicket, PhoneStore, Request, Response, StoreError, StreamEvent, Welcome, read_app_frame,
    write_app_frame,
};
use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointId};
use iroh_tickets::Ticket;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(60);
const READ_LIMIT: usize = 4 + MAX_HEADER_BYTES + 25 * 1024 * 1024;

#[derive(Default)]
struct MemoryStore {
    identity: Mutex<Option<[u8; 32]>>,
    computers: Mutex<Vec<ComputerRecord>>,
}

impl PhoneStore for MemoryStore {
    fn load_identity(&self) -> Result<Option<[u8; 32]>, StoreError> {
        Ok(*self.identity.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn save_identity(&self, secret: &[u8; 32]) -> Result<(), StoreError> {
        *self.identity.lock().unwrap_or_else(PoisonError::into_inner) = Some(*secret);
        Ok(())
    }

    fn load_computers(&self) -> Result<Vec<ComputerRecord>, StoreError> {
        Ok(self
            .computers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn save_computers(&self, computers: &[ComputerRecord]) -> Result<(), StoreError> {
        *self
            .computers
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = computers.to_vec();
        Ok(())
    }
}

fn loopback() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

async fn server_endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .alpns(vec![PAIR_ALPN.to_vec(), APP_ALPN.to_vec()])
        .bind_addr(loopback())
        .expect("server bind address")
        .bind()
        .await
        .expect("server binds")
}

async fn client_endpoint(identity: &DeviceIdentity) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(identity.secret_key().clone())
        .bind_addr(loopback())
        .expect("client bind address")
        .bind()
        .await
        .expect("client binds")
}

async fn read_pair<T: DeserializeOwned>(recv: &mut RecvStream) -> T {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length).await.expect("pair length");
    let length = u32::from_be_bytes(length) as usize;
    assert!(length > 0 && length <= MAX_PAIR_FRAME_BYTES);
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes).await.expect("pair body");
    serde_json::from_slice(&bytes).expect("pair message")
}

async fn write_pair<T: Serialize>(send: &mut SendStream, value: &T) {
    let bytes = serde_json::to_vec(value).expect("pair message encodes");
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .expect("pair length");
    send.write_all(&bytes).await.expect("pair body");
}

async fn read_request(recv: &mut RecvStream) -> Request {
    let frame = recv.read_to_end(READ_LIMIT).await.expect("request frame");
    Request::decode(&frame).expect("request decodes")
}

async fn write_response(send: &mut SendStream, response: &Response) {
    let frame = response.encode().expect("response encodes");
    send.write_all(&frame).await.expect("response writes");
    send.finish().expect("response finishes");
}

async fn handshake(connection: &iroh::endpoint::Connection, server: &Endpoint) -> AppHello {
    let (mut send, mut recv) = connection.accept_bi().await.expect("control stream");
    let hello: AppHello = read_app_frame(&mut recv, MAX_APP_FRAME_BYTES)
        .await
        .expect("hello frame");
    assert!(hello.is_compatible());
    let welcome = Welcome {
        v: PROTOCOL_VERSION,
        computer_id: server.id().to_z32(),
        computer_name: "Studio computer".to_owned(),
        app_version: "0.1.0".to_owned(),
        protocol_version: PROTOCOL_VERSION,
        min_bridge_version: 1,
        ui: None,
    };
    write_app_frame(&mut send, &welcome, MAX_APP_FRAME_BYTES)
        .await
        .expect("welcome frame");
    send.finish().expect("handshake finishes");
    hello
}

async fn write_stream_event(send: &mut SendStream, value: Value) {
    let frame = StreamEvent::new(value).encode().expect("event encodes");
    send.write_all(&frame).await.expect("event writes");
    send.finish().expect("event stream finishes");
}

async fn write_stream_start(send: &mut SendStream) {
    let frame = Response::stream_start().encode().expect("start encodes");
    send.write_all(&frame).await.expect("start writes");
}

fn confirm(server: &Endpoint, code: &str) -> PairReply {
    PairReply {
        v: PROTOCOL_VERSION,
        ok: true,
        status: Some("confirm".to_owned()),
        code: Some(code.to_owned()),
        computer_id: Some(server.id().to_z32()),
        computer_name: Some("Studio computer".to_owned()),
        protocol_version: Some(PROTOCOL_VERSION),
        min_bridge_version: Some(1),
        message: None,
    }
}

fn paired() -> PairReply {
    PairReply {
        v: PROTOCOL_VERSION,
        ok: true,
        status: Some("paired".to_owned()),
        ..PairReply::default()
    }
}

struct SecretKeyForTest;

impl SecretKeyForTest {
    fn id() -> EndpointId {
        iroh::SecretKey::from_bytes(&[9; 32]).public()
    }
}

#[tokio::test]
async fn pairs_and_reads_the_session_snapshot_over_loopback() {
    tokio::time::timeout(TIMEOUT, async {
        let server = server_endpoint().await;
        let server_id = server.id();
        let server_addr = server.addr();
        let secret = [0xAB; 32];
        let ticket = PairTicket::new(server.addr(), secret, "Studio computer", u64::MAX)
            .expect("ticket")
            .encode_string();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("pair connection");
            let connection = incoming.await.expect("pair completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("pair stream");
            let hello: Value = read_pair(&mut recv).await;
            assert_eq!(hello["v"], PROTOCOL_VERSION);
            assert_eq!(hello["name"], "Phone");
            assert_eq!(hello["platform"], "test");
            assert_eq!(hello["secret"].as_str().expect("secret").len(), 52);

            write_pair(&mut send, &confirm(&server, "428913")).await;
            write_pair(&mut send, &paired()).await;
            send.finish().expect("pair finishes");
            let _ = connection.closed().await;

            let incoming = server.accept().await.expect("app connection");
            let connection = incoming.await.expect("app completes");
            handshake(&connection, &server).await;
            let (mut send, mut recv) = connection.accept_bi().await.expect("app stream");
            let request = read_request(&mut recv).await;
            assert_eq!(request.method(), Method::Bootstrap);
            write_response(
                &mut send,
                &Response::ok(json!({
                    "projects": [],
                    "accounts": [],
                    "sessions": [{"id": "s1", "name": "api"}],
                })),
            )
            .await;
            let _ = connection.closed().await;
            server.close().await;
        });

        let store = Arc::new(MemoryStore::default());
        let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
        let identity = DeviceIdentity::generate();
        let endpoint = client_endpoint(&identity).await;
        let client = LinkClient::from_endpoint(store_trait, identity, endpoint);

        let pairing = client
            .pair(&ticket, "Phone", "test")
            .await
            .expect("pairing starts");
        assert_eq!(pairing.confirm().expect("confirm").code, "428913");
        let computer = pairing.finish().await.expect("pairing finishes");
        assert_eq!(computer.id, server_id.to_z32());
        assert_eq!(computer.name, "Studio computer");
        assert_eq!(computer.addr, server_addr);

        let stored = store
            .computers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, server_id.to_z32());

        let app = client.connect(&computer).await.expect("app connects");
        let response = app
            .request(Method::Bootstrap, None, Vec::new())
            .await
            .expect("bootstrap");
        assert_eq!(
            response.result().expect("result")["sessions"][0]["id"],
            "s1"
        );
        app.close();

        client.forget(&computer.id).expect("forget");
        assert!(
            store
                .computers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );

        server_task.await.expect("server task");
    })
    .await
    .expect("loopback test timed out");
}

#[tokio::test]
async fn a_refused_pairing_reports_the_status() {
    tokio::time::timeout(TIMEOUT, async {
        let server = server_endpoint().await;
        let ticket = PairTicket::new(server.addr(), [0xCD; 32], "Studio computer", u64::MAX)
            .expect("ticket")
            .encode_string();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("pair connection");
            let connection = incoming.await.expect("pair completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("pair stream");
            let _: Value = read_pair(&mut recv).await;
            write_pair(
                &mut send,
                &PairReply {
                    v: PROTOCOL_VERSION,
                    ok: false,
                    status: Some("rejected".to_owned()),
                    message: Some("The pairing secret does not match.".to_owned()),
                    ..PairReply::default()
                },
            )
            .await;
            send.finish().expect("pair finishes");
            let _ = connection.closed().await;
            server.close().await;
        });

        let store = Arc::new(MemoryStore::default());
        let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
        let identity = DeviceIdentity::generate();
        let endpoint = client_endpoint(&identity).await;
        let client = LinkClient::from_endpoint(store_trait, identity, endpoint);

        let error = client.pair(&ticket, "Phone", "test").await.unwrap_err();
        match error {
            elesdee_link::ClientError::Rejected(rejection) => {
                assert_eq!(rejection.status, "rejected");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(
            store
                .computers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );

        server_task.await.expect("server task");
    })
    .await
    .expect("loopback test timed out");
}

#[tokio::test]
async fn an_oversized_response_header_is_refused_before_allocation() {
    tokio::time::timeout(TIMEOUT, async {
        let server = server_endpoint().await;
        let ticket = PairTicket::new(server.addr(), [0xEF; 32], "Studio computer", u64::MAX)
            .expect("ticket")
            .encode_string();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("pair connection");
            let connection = incoming.await.expect("pair completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("pair stream");
            let _: Value = read_pair(&mut recv).await;
            write_pair(&mut send, &confirm(&server, "111111")).await;
            write_pair(&mut send, &paired()).await;
            send.finish().expect("pair finishes");
            let _ = connection.closed().await;

            let incoming = server.accept().await.expect("app connection");
            let connection = incoming.await.expect("app completes");
            handshake(&connection, &server).await;
            let (mut send, mut recv) = connection.accept_bi().await.expect("app stream");
            let _ = read_request(&mut recv).await;
            // Declare a header over the cap and write no body.
            send.write_all(&((MAX_HEADER_BYTES + 1) as u32).to_be_bytes())
                .await
                .expect("oversized declaration");
            let _ = send.finish();
            let _ = connection.closed().await;
            server.close().await;
        });

        let store = Arc::new(MemoryStore::default());
        let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
        let identity = DeviceIdentity::generate();
        let endpoint = client_endpoint(&identity).await;
        let client = LinkClient::from_endpoint(store_trait, identity, endpoint);

        let pairing = client
            .pair(&ticket, "Phone", "test")
            .await
            .expect("pairing");
        let computer = pairing.finish().await.expect("paired");
        let app = client.connect(&computer).await.expect("app connects");
        let error = app
            .request(Method::Bootstrap, None, Vec::new())
            .await
            .unwrap_err();
        assert!(
            matches!(error, elesdee_link::ClientError::TooLarge),
            "unexpected error: {error}"
        );
        app.close();

        server_task.await.expect("server task");
    })
    .await
    .expect("loopback test timed out");
}

#[tokio::test]
async fn an_expired_ticket_is_refused_before_the_dial() {
    let identity = DeviceIdentity::generate();
    let store = Arc::new(MemoryStore::default());
    let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
    let endpoint = client_endpoint(&identity).await;
    let client = LinkClient::from_endpoint(store_trait, identity, endpoint);
    let ticket = PairTicket::new(
        iroh::EndpointAddr::new(SecretKeyForTest::id()),
        [1; 32],
        "Studio computer",
        1_000,
    )
    .expect("ticket")
    .encode_string();

    let error = client
        .pair_at(&ticket, "Phone", "test", 1_000)
        .await
        .unwrap_err();
    assert!(matches!(error, elesdee_link::ClientError::Expired));
}

#[tokio::test]
async fn an_event_and_output_stream_cross_loopback() {
    tokio::time::timeout(TIMEOUT, async {
        let server = server_endpoint().await;
        let ticket = PairTicket::new(server.addr(), [0x31; 32], "Studio computer", u64::MAX)
            .expect("ticket")
            .encode_string();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("pair connection");
            let connection = incoming.await.expect("pair completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("pair stream");
            let _: Value = read_pair(&mut recv).await;
            write_pair(&mut send, &confirm(&server, "123456")).await;
            write_pair(&mut send, &paired()).await;
            send.finish().expect("pair finishes");
            let _ = connection.closed().await;

            let incoming = server.accept().await.expect("app connection");
            let connection = incoming.await.expect("app completes");
            handshake(&connection, &server).await;

            let (mut send, mut recv) = connection.accept_bi().await.expect("event stream");
            let request = read_request(&mut recv).await;
            assert_eq!(request.method(), Method::EventsSubscribe);
            write_stream_start(&mut send).await;
            write_stream_event(
                &mut send,
                json!({"type": "session.updated", "session": {"id": "s1", "name": "renamed"}}),
            )
            .await;

            let (mut send, mut recv) = connection.accept_bi().await.expect("output stream");
            let request = read_request(&mut recv).await;
            assert_eq!(request.method(), Method::OutputSubscribe);
            assert_eq!(request.params().expect("params")["sessionId"], "s1");
            assert_eq!(request.params().expect("params")["after"], 7);
            write_stream_start(&mut send).await;
            write_stream_event(
                &mut send,
                json!({
                    "seq": 8,
                    "data": "hello",
                    "gap": {"after": 7, "oldestSeq": 8, "newestSeq": 8},
                }),
            )
            .await;

            let _ = connection.closed().await;
            server.close().await;
        });

        let store = Arc::new(MemoryStore::default());
        let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
        let identity = DeviceIdentity::generate();
        let endpoint = client_endpoint(&identity).await;
        let client = LinkClient::from_endpoint(store_trait, identity, endpoint);
        let pairing = client
            .pair(&ticket, "Phone", "test")
            .await
            .expect("pairing");
        let computer = pairing.finish().await.expect("paired");

        let app = client.connect(&computer).await.expect("app connects");
        assert_eq!(app.welcome().computer_id, computer.id);

        let mut events = app.events().await.expect("event stream opens");
        let event = events.next().await.expect("event").expect("event value");
        assert_eq!(event["type"], "session.updated");
        assert_eq!(event["session"]["name"], "renamed");
        assert!(events.next().await.expect("event end").is_none());

        let mut output = app.output("s1", 7).await.expect("output stream opens");
        let event = output.next().await.expect("output").expect("output value");
        assert_eq!(event["seq"], 8);
        assert_eq!(event["data"], "hello");
        assert_eq!(event["gap"]["after"], 7);
        assert!(output.next().await.expect("output end").is_none());

        app.close();
        server_task.await.expect("server task");
    })
    .await
    .expect("loopback stream test timed out");
}

#[tokio::test]
async fn a_revoked_connection_ends_the_stream_and_stops_retries() {
    tokio::time::timeout(TIMEOUT, async {
        let server = server_endpoint().await;
        let ticket = PairTicket::new(server.addr(), [0x41; 32], "Studio computer", u64::MAX)
            .expect("ticket")
            .encode_string();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("pair connection");
            let connection = incoming.await.expect("pair completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("pair stream");
            let _: Value = read_pair(&mut recv).await;
            write_pair(&mut send, &confirm(&server, "654321")).await;
            write_pair(&mut send, &paired()).await;
            send.finish().expect("pair finishes");
            let _ = connection.closed().await;

            let incoming = server.accept().await.expect("app connection");
            let connection = incoming.await.expect("app completes");
            handshake(&connection, &server).await;
            connection.close(elesdee_link::CLOSE_REVOKED.into(), b"revoked");
            server.close().await;
        });

        let store = Arc::new(MemoryStore::default());
        let store_trait: Arc<dyn PhoneStore> = Arc::clone(&store) as Arc<dyn PhoneStore>;
        let identity = DeviceIdentity::generate();
        let endpoint = client_endpoint(&identity).await;
        let client = LinkClient::from_endpoint(store_trait, identity, endpoint);
        let pairing = client
            .pair(&ticket, "Phone", "test")
            .await
            .expect("pairing");
        let computer = pairing.finish().await.expect("paired");
        let reason = match client.connect(&computer).await {
            Ok(app) => app.wait_closed().await,
            Err(error) => error,
        };
        assert!(
            matches!(reason, elesdee_link::ClientError::Revoked),
            "unexpected close: {reason}"
        );
        let mut policy = elesdee_link::ReconnectPolicy::with_seed(9);
        assert!(
            elesdee_link::next_retry(&reason, Duration::ZERO, &mut policy).is_none(),
            "a revoked device must not retry"
        );

        server_task.await.expect("server task");
    })
    .await
    .expect("revocation test timed out");
}

#[test]
fn reconnect_waits_follow_a_fake_clock() {
    struct FakeClock {
        now: Duration,
    }

    impl FakeClock {
        fn advance(&mut self, delay: Duration) -> Duration {
            self.now += delay;
            self.now
        }
    }

    let mut clock = FakeClock {
        now: Duration::ZERO,
    };
    let mut policy = elesdee_link::ReconnectPolicy::with_seed(21);
    let bases = [500u64, 1_000, 2_000, 4_000, 8_000, 15_000, 30_000];
    for (index, base) in bases.into_iter().enumerate() {
        let delay = elesdee_link::next_retry(
            &elesdee_link::ClientError::ConnectionClosed,
            Duration::ZERO,
            &mut policy,
        )
        .expect("retry");
        let now = clock.advance(delay).as_millis() as u64;
        let low = base * 8 / 10;
        let high = base * 12 / 10;
        assert!(
            (low..=high).contains(&(delay.as_millis() as u64)),
            "attempt {index} waited {delay:?}"
        );
        assert!(now >= delay.as_millis() as u64);
    }
    assert!(
        elesdee_link::next_retry(
            &elesdee_link::ClientError::Revoked,
            Duration::ZERO,
            &mut policy
        )
        .is_none()
    );
}

#[test]
fn unknown_method_names_stay_outside_the_allowlist() {
    // The shell bridge takes method names from the interface and parses them
    // against the shared allowlist before any connection is opened.
    assert!(Method::parse("sessions.delete").is_none());
    assert!(Method::parse("projects.create").is_none());
    assert_eq!(Method::parse("bootstrap"), Some(Method::Bootstrap));
}
