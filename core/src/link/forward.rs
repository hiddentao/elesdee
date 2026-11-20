//! The authenticated demo-server forward path.
//!
//! A paired phone sends a two-byte port on each stream. The computer forwards
//! only a port that an active session registered, and it connects only to
//! `127.0.0.1`. Any other port gets a refusal byte and no connection.

use std::sync::Arc;

use elesdee_link::{
    CLOSE_NOT_PAIRED, CLOSE_REVOKED, FORWARD_OK, FORWARD_REFUSED, FORWARD_REQUEST_BYTES,
    FORWARD_UNAVAILABLE, MAX_STREAMS_PER_CONNECTION, decode_port,
};
use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;

use super::hub::{ForwardOpen, LinkHub};
use super::short_id;
use crate::state::AppState;

/// Handles the `elesdee/forward/1` protocol.
#[derive(Clone)]
pub struct ForwardHandler {
    state: AppState,
    hub: LinkHub,
}

impl std::fmt::Debug for ForwardHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ForwardHandler")
            .finish_non_exhaustive()
    }
}

impl ForwardHandler {
    /// Builds a handler for a paired phone's demo-server tunnels.
    pub fn new(state: AppState, hub: LinkHub) -> Self {
        Self { state, hub }
    }
}

impl ProtocolHandler for ForwardHandler {
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
        tracing::info!(phone = %short_id(&endpoint_id), "forward link opened");

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
                        let _ = send.write_all(&[FORWARD_REFUSED]).await;
                        let _ = send.finish();
                        continue;
                    };
                    let Ok(permit) = Arc::clone(&streams).try_acquire_owned() else {
                        let mut send = send;
                        let _ = send.write_all(&[FORWARD_REFUSED]).await;
                        let _ = send.finish();
                        continue;
                    };
                    let state = self.state.clone();
                    let hub = self.hub.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _stream = stream_permit;
                        handle_tunnel(state, hub, send, recv).await;
                    });
                }
            }
        }

        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        tracing::info!(phone = %short_id(&endpoint_id), "forward link closed");
        Ok(())
    }
}

async fn handle_tunnel(state: AppState, hub: LinkHub, mut send: SendStream, mut recv: RecvStream) {
    let mut request = [0u8; FORWARD_REQUEST_BYTES];
    if recv.read_exact(&mut request).await.is_err() {
        return;
    }
    let Some(port) = decode_port(request) else {
        let _ = send.write_all(&[FORWARD_REFUSED]).await;
        let _ = send.finish();
        return;
    };
    if state.demo_server_by_port(port).is_none() {
        tracing::info!(port, "refused an unregistered demo-server port");
        let _ = send.write_all(&[FORWARD_REFUSED]).await;
        let _ = send.finish();
        return;
    }
    let ForwardOpen::Opened(permit) = hub.open_forward(port) else {
        let _ = send.write_all(&[FORWARD_REFUSED]).await;
        let _ = send.finish();
        return;
    };
    // Re-check after reserving the tunnel. A stop between the first check and
    // the reservation would otherwise leave a tunnel open. A later stop closes
    // this tunnel through its cancellation channel.
    if state.demo_server_by_port(port).is_none() {
        let _ = send.write_all(&[FORWARD_REFUSED]).await;
        let _ = send.finish();
        return;
    }
    let cancel = permit.cancellation();

    let Ok(tcp) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        let _ = send.write_all(&[FORWARD_UNAVAILABLE]).await;
        let _ = send.finish();
        return;
    };
    if send.write_all(&[FORWARD_OK]).await.is_err() {
        return;
    }
    let _ = tcp.set_nodelay(true);

    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let mut to_phone = tokio::spawn(async move {
        let result = tokio::io::copy(&mut tcp_read, &mut send).await;
        let _ = send.finish();
        result
    });
    let mut to_server = tokio::spawn(async move {
        let result = tokio::io::copy(&mut recv, &mut tcp_write).await;
        let _ = tcp_write.shutdown().await;
        result
    });

    tokio::select! {
        _ = cancelled(cancel) => {}
        _ = async {
            let _ = (&mut to_phone).await;
            let _ = (&mut to_server).await;
        } => {}
    }
    to_phone.abort();
    to_server.abort();
}

async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}
