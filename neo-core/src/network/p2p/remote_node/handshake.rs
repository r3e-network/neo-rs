//! Handshake bootstrap and reader lifecycle for `RemoteNode`.
use super::{RemoteNode, RemoteNodeCommand};
use crate::akka::{ActorContext, ActorResult};
use crate::network::error::NetworkError;
use crate::network::p2p::connection::ConnectionState;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::p2p::timeouts;
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
        self.spawn_handshake_timeout(ctx);
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
        tokio::spawn(async move {
            loop {
                let result = {
                    let mut guard = connection.lock().await;
                    debug!(target: "neo", endpoint = %guard.address, "waiting for inbound message");
                    let done = handshake_done.load(Ordering::Relaxed);
                    guard.receive_message(done).await.map(|msg| (msg, done))
                };

                match result {
                    Ok((message, _done)) => {
                        let command = message.command();
                        if let Err(err) = actor.tell(RemoteNodeCommand::Inbound(message)) {
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

                yield_now().await;
            }
        });

        self.reader_spawned = true;
    }

    pub(super) fn spawn_handshake_timeout(&self, ctx: &ActorContext) {
        let actor = ctx.self_ref();
        let timeout = self.config.handshake_timeout;
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = actor.tell(RemoteNodeCommand::HandshakeTimeout);
        });
    }
}
