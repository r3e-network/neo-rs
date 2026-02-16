use super::*;
use crate::akka::Actor;
use async_trait::async_trait;

#[async_trait]
impl Actor for RemoteNode {
    async fn handle(
        &mut self,
        message: Box<dyn std::any::Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match message.downcast::<RemoteNodeCommand>() {
            Ok(command) => match *command {
                RemoteNodeCommand::StartProtocol => self.start_protocol(ctx).await,
                RemoteNodeCommand::Send(message) => self.enqueue_message(message).await,
                RemoteNodeCommand::Inbound(message) => self.on_inbound(message, ctx).await,
                RemoteNodeCommand::ConnectionError { error } => self.fail(ctx, error).await,
                RemoteNodeCommand::HandshakeTimeout => {
                    if self.handshake_complete {
                        return Ok(());
                    }
                    let error = NetworkError::ProtocolViolation {
                        peer: self.endpoint,
                        violation: "handshake timeout".to_string(),
                    };
                    self.fail(ctx, error).await
                }
                RemoteNodeCommand::TimerTick => self.on_timer(ctx).await,
                RemoteNodeCommand::Disconnect { reason } => {
                    debug!(target: "neo", endpoint = %self.endpoint, reason, "disconnecting remote node");
                    ctx.stop_self()?;
                    Ok(())
                }
            },
            Err(other) => {
                // Drop unknown message types quietly to avoid log spam and mismatched routing.
                trace!(
                    target: "neo",
                    message_type_id = ?other.as_ref().type_id(),
                    "unknown message routed to remote node actor"
                );
                Ok(())
            }
        }
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        {
            let mut connection = self.connection.lock().await;
            if let Err(err) = connection.close().await {
                warn!(target: "neo", error = %err, "error shutting down TCP stream during stop");
            }
        }
        self.known_hashes.clear();
        self.sent_hashes.clear();
        self.pending_known_hashes.clear();
        self.bloom_filter = None;
        self.cancel_timer();
        if let Some(parent) = ctx.parent() {
            let self_ref = ctx.self_ref();
            if let Err(err) = parent.tell(PeerCommand::ConnectionTerminated { actor: self_ref }) {
                trace!(target: "neo", error = %err, "failed to notify parent about remote node termination");
            }
        }
        Ok(())
    }
}
