//! Protocol routing and wire message helpers for `RemoteNode`.
use super::RemoteNode;
use crate::akka::{ActorContext, ActorResult};
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::p2p::payloads::addr_payload::{AddrPayload, MAX_COUNT_TO_SEND};
use crate::network::MessageCommand;
use rand::{seq::IteratorRandom, thread_rng};
use tracing::trace;
impl RemoteNode {
    pub(super) async fn forward_protocol(
        &mut self,
        message: NetworkMessage,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if !self.handshake_complete {
            trace!(target: "neo", command = ?message.command(), "dropping protocol message prior to handshake");
            return Ok(());
        }
        match &message.payload {
            ProtocolMessage::Ping(payload) => self.on_ping(payload, ctx).await,
            ProtocolMessage::Pong(payload) => {
                self.on_pong(payload);
                Ok(())
            }
            ProtocolMessage::Inv(payload) => {
                self.on_inv(payload, ctx);
                Ok(())
            }
            ProtocolMessage::Transaction(tx) => self.on_transaction(tx.clone(), ctx).await,
            ProtocolMessage::Block(block) => self.on_block(block.clone(), ctx).await,
            ProtocolMessage::Extensible(payload) => self.on_extensible(payload.clone(), ctx).await,
            ProtocolMessage::GetBlocks(payload) => self.on_get_blocks(payload.clone()).await,
            ProtocolMessage::GetBlockByIndex(payload) => {
                self.on_get_block_by_index(payload.clone()).await
            }
            ProtocolMessage::GetHeaders(payload) => self.on_get_headers(payload.clone()).await,
            ProtocolMessage::Headers(payload) => {
                self.on_headers(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Addr(payload) => {
                self.on_addr(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Mempool => self.on_mempool().await,
            ProtocolMessage::GetData(payload) => self.on_get_data(payload).await,
            ProtocolMessage::FilterLoad(payload) => {
                self.on_filter_load(payload);
                Ok(())
            }
            ProtocolMessage::FilterAdd(payload) => {
                self.on_filter_add(payload);
                Ok(())
            }
            ProtocolMessage::FilterClear => {
                self.on_filter_clear();
                Ok(())
            }
            ProtocolMessage::NotFound(payload) => {
                self.on_not_found(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Reject(data) => {
                self.on_reject(data);
                Ok(())
            }
            ProtocolMessage::Alert(data) => {
                self.on_alert(data);
                Ok(())
            }
            ProtocolMessage::GetAddr => {
                let addresses = {
                    let mut rng = thread_rng();
                    self.local_node
                        .address_book()
                        .into_iter()
                        .filter(|addr| addr.endpoint().map(|e| e.port() > 0).unwrap_or(false))
                        .choose_multiple(&mut rng, MAX_COUNT_TO_SEND)
                };
                if addresses.is_empty() {
                    return Ok(());
                }
                let payload = AddrPayload::create(addresses);
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::Addr(payload)))
                    .await
            }
            _ => Ok(()),
        }
    }
    pub(super) async fn send_verack(&mut self) -> ActorResult {
        let message = NetworkMessage::new(ProtocolMessage::Verack);
        self.send_wire_message(&message).await
    }
    pub(super) async fn send_wire_message(&mut self, message: &NetworkMessage) -> ActorResult {
        let mut connection = self.connection.lock().await;
        if let Err(err) = connection.send_message(message).await {
            if err.is_timeout() {
                crate::network::p2p::timeouts::inc_write_timeout();
            }
            tracing::warn!(
                target: "neo",
                endpoint = %self.endpoint,
                error = %err,
                "failed to send message to peer"
            );
            return Err(crate::akka::AkkaError::system(err.to_string()));
        }
        self.last_sent = std::time::Instant::now();
        let index = message.command().to_byte() as usize;
        if index < self.sent_commands.len() {
            self.sent_commands[index] = true;
        }
        Ok(())
    }
    pub(super) fn build_wire_message(
        message: &NetworkMessage,
    ) -> Option<crate::network::p2p::message::Message> {
        if let Some(raw) = message.wire_payload() {
            return crate::network::p2p::message::Message::from_wire_parts(
                message.flags,
                message.command(),
                raw,
            )
            .ok();
        }
        let payload = match message.payload.to_bytes() {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(
                    target: "neo",
                    error = %err,
                    "failed to serialize protocol payload for message handlers"
                );
                return None;
            }
        };
        let mut wire = crate::network::p2p::message::Message {
            flags: message.flags,
            command: message.command(),
            payload_raw: payload.clone(),
            payload_compressed: payload.clone(),
        };
        if wire.flags.is_compressed() {
            match crate::compression::compress_lz4(&wire.payload_raw) {
                Ok(compressed) => wire.payload_compressed = compressed,
                Err(err) => {
                    tracing::warn!(
                        target: "neo",
                        error = %err,
                        "failed to recompress payload for message handlers"
                    );
                    wire.flags = crate::network::MessageFlags::NONE;
                    wire.payload_compressed = wire.payload_raw.clone();
                }
            }
        }
        Some(wire)
    }
    pub(super) fn is_single_command(command: MessageCommand) -> bool {
        matches!(
            command,
            MessageCommand::Addr
                | MessageCommand::GetAddr
                | MessageCommand::GetBlocks
                | MessageCommand::GetHeaders
                | MessageCommand::Mempool
                | MessageCommand::Ping
                | MessageCommand::Pong
        )
    }
    pub(super) fn is_high_priority(command: MessageCommand) -> bool {
        matches!(
            command,
            MessageCommand::Alert
                | MessageCommand::Extensible
                | MessageCommand::FilterAdd
                | MessageCommand::FilterClear
                | MessageCommand::FilterLoad
                | MessageCommand::GetAddr
                | MessageCommand::Mempool
        )
    }
}
