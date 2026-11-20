//! A request and response over two real iroh endpoints on loopback.

use std::net::SocketAddr;
use std::time::Duration;

use elesdee_link::{APP_ALPN, Method, Request, Response};
use iroh::Endpoint;
use iroh::endpoint::presets;
use serde_json::json;

const TEST_READ_LIMIT: usize = 1024 * 1024;

fn loopback() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

#[tokio::test]
async fn request_and_response_cross_two_iroh_endpoints() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let server = Endpoint::builder(presets::Minimal)
            .alpns(vec![APP_ALPN.to_vec()])
            .bind_addr(loopback())
            .expect("server bind address")
            .bind()
            .await
            .expect("server binds");

        let client = Endpoint::builder(presets::Minimal)
            .bind_addr(loopback())
            .expect("client bind address")
            .bind()
            .await
            .expect("client binds");

        let server_addr = server.addr();
        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("incoming connection");
            let connection = incoming.await.expect("connection completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("request stream");

            let frame = recv
                .read_to_end(TEST_READ_LIMIT)
                .await
                .expect("request frame");
            let request = Request::decode(&frame).expect("request decodes");
            assert_eq!(request.method(), Method::SessionsInput);
            assert_eq!(request.params(), Some(&json!({"sessionId": "s1"})));
            assert_eq!(request.body(), b"hello");

            let response = Response::ok(json!({"accepted": true}));
            let encoded = response.encode().expect("response encodes");
            send.write_all(&encoded).await.expect("response writes");
            send.finish().expect("response stream finishes");

            let _ = connection.closed().await;
            server.close().await;
        });

        let connection = client
            .connect(server_addr, APP_ALPN)
            .await
            .expect("client connects");
        let (mut send, mut recv) = connection.open_bi().await.expect("request stream");

        let request = Request::new(Method::SessionsInput, Some(json!({"sessionId": "s1"})))
            .with_body(b"hello".to_vec());
        let encoded = request.encode().expect("request encodes");
        send.write_all(&encoded).await.expect("request writes");
        send.finish().expect("request stream finishes");

        let frame = recv
            .read_to_end(TEST_READ_LIMIT)
            .await
            .expect("response frame");
        let response = Response::decode(&frame).expect("response decodes");
        assert_eq!(response.result(), Some(&json!({"accepted": true})));
        assert!(response.body().is_empty());

        connection.close(0u32.into(), b"done");
        client.close().await;
        server_task.await.expect("server task");
    })
    .await
    .expect("loopback test timed out");
}
