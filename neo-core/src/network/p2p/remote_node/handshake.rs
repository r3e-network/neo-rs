//! Handshake bootstrap and reader lifecycle for `RemoteNode`.
use super::{RemoteNode, RemoteNodeCommand};
use crate::network::error::NetworkError;
use crate::network::p2p::connection::ConnectionState;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use neo_p2p::timeouts;
use crate::runtime::{ActorContext, ActorResult};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::yield_now;
use tracing::{debug, warn};

impl RemoteNode {
    pub(super) async fn start_protocol(&mut self, ctx: &mut ActorContext) -> ActorResult {
        debug!(
            target: "neo",
            endpoint = %self.endpoint,
            reader_spawned = self.reader_spawned,
            "starting protocol handshake"
        );
        self.ensure_timer(ctx);
        self.arm_handshake_timeout(ctx);
        self.spawn_reader(ctx);

        let mut connection = self.connection.lock().await;
        connection.set_state(ConnectionState::Handshaking);

        // Check if compression is allowed based on capabilities (C# parity)
        let allow_compression = !self.local_version.capabilities.iter().any(|c| {
            matches!(
                c,
                crate::network::p2p::capabilities::NodeCapability::DisableCompression
            )
        });
        connection.compression_allowed = allow_compression && self.config.enable_compression;

        let message = NetworkMessage::new(ProtocolMessage::Version(self.local_version.clone()));

        // Send version message
        if let Err(err) = connection.send_message(&message).await {
            drop(connection);
            let network_error = NetworkError::ConnectionError(err.to_string());
            return self.fail(ctx, network_error).await;
        }

        // Flush immediately for handshake messages to ensure timely delivery
        if let Err(err) = connection.flush().await {
            drop(connection);
            let network_error =
                NetworkError::ConnectionError(format!("Failed to flush version: {}", err));
            return self.fail(ctx, network_error).await;
        }

        drop(connection);
        self.last_sent = Instant::now();
        Ok(())
    }

    pub(super) fn spawn_reader(&mut self, ctx: &ActorContext) {
        if self.reader_spawned {
            return;
        }

        let actor = ctx.self_ref();
        let connection = Arc::clone(&self.connection);
        let endpoint = self.endpoint;
        let handshake_done = Arc::clone(&self.handshake_done);
        let cancellation = self.reader_cancellation.child_token();
        let _ = self.reader_tasks.spawn(async move {
            loop {
                let result = tokio::select! {
                    _ = cancellation.cancelled() => break,
                    result = async {
                        let mut guard = connection.lock().await;
                        debug!(target: "neo", endpoint = %guard.address, "waiting for inbound message");
                        let done = handshake_done.load(Ordering::Relaxed);
                        guard.receive_message(done).await.map(|msg| (msg, done))
                    } => result,
                };

                match result {
                    Ok((message, _done)) => {
                        let command = message.command();
                        let delivered = tokio::select! {
                            _ = cancellation.cancelled() => break,
                            delivered = actor.tell_async(RemoteNodeCommand::Inbound(message)) => delivered,
                        };
                        if let Err(err) = delivered {
                            warn!(target: "neo", error = %err, "failed to deliver inbound message to remote node actor");
                            break;
                        } else {
                            debug!(
                                target: "neo",
                                endpoint = %endpoint,
                                ?command,
                                "enqueued inbound message to actor"
                            );
                        }
                    }
                    Err(error) => {
                        let done = handshake_done.load(Ordering::Relaxed);
                        let is_timeout = error.is_timeout();
                        if is_timeout {
                            if !done {
                                warn!(
                                    target: "neo",
                                    endpoint = %endpoint,
                                    "handshake read timed out"
                                );
                                timeouts::inc_handshake_timeout();
                                // Keep the reader alive; the explicit handshake timer owns teardown.
                                continue;
                            } else {
                                debug!(
                                    target: "neo",
                                    endpoint = %endpoint,
                                    "read loop timed out during active session"
                                );
                                timeouts::inc_read_timeout();
                                continue;
                            }
                        }

                        debug!(
                            target: "neo",
                            endpoint = %endpoint,
                            error = %error,
                            "read loop encountered network error"
                        );
                        let _ = actor.tell(RemoteNodeCommand::ConnectionError { error });
                        break;
                    }
                }

                tokio::select! {
                    _ = cancellation.cancelled() => break,
                    _ = yield_now() => {}
                }
            }
        });

        self.reader_spawned = true;
    }

    pub(super) async fn stop_reader(&mut self) {
        self.reader_cancellation.cancel();
        self.reader_tasks.close();
        if tokio::time::timeout(
            super::READER_TASK_SHUTDOWN_TIMEOUT,
            self.reader_tasks.wait(),
        )
        .await
        .is_err()
        {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                "timed out waiting for remote node reader task to stop"
            );
        }
    }

    pub(super) fn arm_handshake_timeout(&mut self, ctx: &ActorContext) {
        if self.handshake_timeout.is_some() {
            return;
        }

        let actor = ctx.self_ref();
        let handle = ctx.schedule_tell_once_cancelable(
            self.config.handshake_timeout,
            &actor,
            RemoteNodeCommand::HandshakeTimeout,
            None,
        );
        self.handshake_timeout = Some(handle);
    }

    pub(super) fn cancel_handshake_timeout(&mut self) {
        if let Some(timeout) = self.handshake_timeout.take() {
            timeout.cancel();
        }
    }
}
