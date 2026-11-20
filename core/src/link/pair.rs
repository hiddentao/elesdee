//! The one-time pairing transport.

use std::time::Duration;

use elesdee_link::{CLOSE_NOT_PAIRED, CONFIRMATION_TTL_SECS, UNARY_REQUEST_TIMEOUT};
use iroh::endpoint::Connection;
use iroh::endpoint::VarInt;
use iroh::protocol::{AcceptError, ProtocolHandler};

use super::hub::LinkHub;
use super::now_unix;
use super::short_id;
use super::wire::{PairDecision, PairReply, PairRequest, read_pair_frame, write_pair_frame};
use crate::state::AppState;

const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// Handles the `elesdee/pair/1` protocol.
#[derive(Clone)]
pub struct PairHandler {
    state: AppState,
    hub: LinkHub,
}

impl std::fmt::Debug for PairHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairHandler").finish_non_exhaustive()
    }
}

impl PairHandler {
    /// Builds a pair handler for one core service.
    pub fn new(state: AppState, hub: LinkHub) -> Self {
        Self { state, hub }
    }
}

impl ProtocolHandler for PairHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let phone_id = connection.remote_id();
        if !self.hub.pairing_open() {
            connection.close(VarInt::from_u32(CLOSE_NOT_PAIRED), b"no pairing session");
            return Ok(());
        }

        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "pairing stream failed");
                return Ok(());
            }
        };

        let request = match tokio::time::timeout(
            UNARY_REQUEST_TIMEOUT,
            read_pair_frame::<PairRequest>(&mut recv),
        )
        .await
        {
            Ok(Ok(request)) => request,
            Ok(Err(error)) => {
                tracing::warn!(%error, "pairing request could not be read");
                let reply = PairReply::rejected("invalid", "The pairing request is not valid.");
                let _ = write_pair_frame(&mut send, &reply).await;
                finish(connection, &mut send).await;
                return Ok(());
            }
            Err(_) => {
                tracing::warn!("pairing request timed out");
                finish(connection, &mut send).await;
                return Ok(());
            }
        };

        let now = now_unix();
        let phone_id_string = phone_id.to_z32();
        let already_paired = self.state.is_paired_device(&phone_id_string);
        let paired_count = self.state.paired_device_count();
        let confirm =
            match self
                .hub
                .begin_pairing(&request, phone_id, already_paired, paired_count, now)
            {
                Ok(confirm) => confirm,
                Err(reject) => {
                    tracing::info!(
                        phone = %short_id(&phone_id_string),
                        reason = reject.status(),
                        "pairing request rejected"
                    );
                    let _ = write_pair_frame(&mut send, &reject.reply()).await;
                    finish(connection, &mut send).await;
                    return Ok(());
                }
            };

        let Some(desktop) = self.hub.desktop() else {
            let reply = PairReply::rejected("not_ready", "The computer is not ready.");
            let _ = write_pair_frame(&mut send, &reply).await;
            finish(connection, &mut send).await;
            return Ok(());
        };
        let reply = PairReply::confirm(confirm.code, desktop.id_string.clone(), desktop.name);
        if write_pair_frame(&mut send, &reply).await.is_err() {
            finish(connection, &mut send).await;
            return Ok(());
        }

        let answer =
            tokio::time::timeout(Duration::from_secs(CONFIRMATION_TTL_SECS), confirm.decision)
                .await;
        let (reply, approved) = match answer {
            Ok(Ok(PairDecision::Approved(device))) => {
                tracing::info!(
                    phone = %short_id(&device.endpoint_id),
                    "pairing approved"
                );
                (PairReply::paired(), true)
            }
            Ok(Ok(PairDecision::Denied)) => (
                PairReply::rejected("denied", "The user denied this phone."),
                false,
            ),
            Ok(Err(_)) | Err(_) => {
                // Expire the snapshot so the desktop UI stops showing a code.
                let _ = self.hub.snapshot(now_unix());
                (
                    PairReply::rejected("expired", "The confirmation expired."),
                    false,
                )
            }
        };

        let _ = write_pair_frame(&mut send, &reply).await;
        if !approved {
            tracing::info!(phone = %short_id(&phone_id_string), "pairing failed");
        }
        finish(connection, &mut send).await;
        Ok(())
    }
}

async fn finish(connection: Connection, send: &mut iroh::endpoint::SendStream) {
    let _ = send.finish();
    // Give the peer time to read the final message before the connection drops.
    let _ = tokio::time::timeout(CLOSE_GRACE, connection.closed()).await;
    connection.close(VarInt::from_u32(0), b"pairing finished");
}
