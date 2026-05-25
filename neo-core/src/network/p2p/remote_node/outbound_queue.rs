//! Outbound message queueing and per-peer memory quota handling.

use super::RemoteNode;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::MessageCommand;
use crate::runtime::ActorResult;
use bitvec::prelude::{BitVec, Lsb0};
use std::collections::VecDeque;
use std::time::Instant;
use tracing::warn;

type QueuedCommandBits = BitVec<u8, Lsb0>;
const MESSAGE_COMMAND_DOMAIN_BYTES: usize = 32;

pub(super) struct OutboundMessageQueue {
    messages: VecDeque<NetworkMessage>,
    queued_single_commands: QueuedCommandBits,
}

impl Default for OutboundMessageQueue {
    fn default() -> Self {
        Self {
            messages: VecDeque::new(),
            queued_single_commands: QueuedCommandBits::from_vec(vec![
                0u8;
                MESSAGE_COMMAND_DOMAIN_BYTES
            ]),
        }
    }
}

impl OutboundMessageQueue {
    fn len(&self) -> usize {
        self.messages.len()
    }

    fn push_back(&mut self, message: NetworkMessage) {
        self.record_queued_single_command(message.command());
        self.messages.push_back(message);
    }

    fn pop_front(&mut self) -> Option<NetworkMessage> {
        let message = self.messages.pop_front()?;
        self.clear_queued_single_command(message.command());
        Some(message)
    }

    fn has_duplicate_single_command(&self, command: MessageCommand) -> bool {
        RemoteNode::is_single_command(command)
            && self
                .queued_single_commands
                .get(command.to_byte() as usize)
                .map(|queued| *queued)
                .unwrap_or(false)
    }

    fn record_queued_single_command(&mut self, command: MessageCommand) {
        self.set_queued_single_command(command, true);
    }

    fn clear_queued_single_command(&mut self, command: MessageCommand) {
        self.set_queued_single_command(command, false);
    }

    fn set_queued_single_command(&mut self, command: MessageCommand, value: bool) {
        if !RemoteNode::is_single_command(command) {
            return;
        }
        if let Some(mut queued) = self
            .queued_single_commands
            .get_mut(command.to_byte() as usize)
        {
            queued.set(value);
        }
    }
}

impl RemoteNode {
    /// Maximum number of messages allowed in each queue to prevent memory exhaustion.
    /// This protects against DoS attacks from malicious peers flooding messages.
    const MAX_QUEUE_SIZE: usize = 1024;

    /// SECURITY: Maximum memory usage per peer in bytes (8 MB).
    /// This prevents a single malicious peer from exhausting node memory.
    const MAX_MEMORY_PER_PEER: usize = 8 * 1024 * 1024;

    pub(super) async fn enqueue_message(&mut self, message: NetworkMessage) -> ActorResult {
        let command = message.command();
        let is_high_priority = Self::is_high_priority(command);

        let message_size = Self::estimate_message_size(&message);
        if !self.check_memory_quota(message_size) {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                command = ?command,
                message_size = message_size,
                current_usage = self.memory_usage_bytes,
                max_allowed = Self::MAX_MEMORY_PER_PEER,
                "per-peer memory quota exceeded, dropping message"
            );
            return Ok(());
        }

        let (queue_full, has_duplicate) = if is_high_priority {
            let full = self.message_queue_high.len() >= Self::MAX_QUEUE_SIZE;
            let duplicate = self
                .message_queue_high
                .has_duplicate_single_command(command);
            (full, duplicate)
        } else {
            let full = self.message_queue_low.len() >= Self::MAX_QUEUE_SIZE;
            let duplicate = self.message_queue_low.has_duplicate_single_command(command);
            (full, duplicate)
        };

        if queue_full {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                command = ?command,
                "message queue full, dropping message"
            );
            return Ok(());
        }

        if has_duplicate {
            return Ok(());
        }

        self.add_memory_usage(message_size);

        if is_high_priority {
            self.message_queue_high.push_back(message);
        } else {
            self.message_queue_low.push_back(message);
        }

        self.flush_queue().await
    }

    pub(super) async fn flush_queue(&mut self) -> ActorResult {
        if !self.verack_received || !self.ack_ready {
            return Ok(());
        }

        while self.verack_received && self.ack_ready {
            let next_message = if let Some(message) = self.message_queue_high.pop_front() {
                Some(message)
            } else {
                self.message_queue_low.pop_front()
            };

            let Some(message) = next_message else {
                break;
            };

            let message_size = Self::estimate_message_size(&message);
            self.release_memory_usage(message_size);

            self.ack_ready = false;
            self.last_sent = Instant::now();
            let index = message.command().to_byte() as usize;
            if index < self.sent_commands.len() {
                self.sent_commands[index] = true;
            }
            self.send_wire_message(&message).await?;
            self.ack_ready = true;
        }

        Ok(())
    }

    fn check_memory_quota(&self, additional_bytes: usize) -> bool {
        self.memory_usage_bytes.saturating_add(additional_bytes) <= Self::MAX_MEMORY_PER_PEER
    }

    fn add_memory_usage(&mut self, bytes: usize) {
        self.memory_usage_bytes = self.memory_usage_bytes.saturating_add(bytes);
    }

    fn release_memory_usage(&mut self, bytes: usize) {
        self.memory_usage_bytes = self.memory_usage_bytes.saturating_sub(bytes);
    }

    fn estimate_message_size(message: &NetworkMessage) -> usize {
        const BASE_OVERHEAD: usize = 64;

        let payload_size = match &message.payload {
            ProtocolMessage::Block(_) => 2048,
            ProtocolMessage::Headers(headers) => headers.headers.len() * 512,
            ProtocolMessage::Transaction(_) => 1024,
            ProtocolMessage::Inv(inv) => inv.hashes.len() * 32,
            ProtocolMessage::GetData(inv) => inv.hashes.len() * 32,
            ProtocolMessage::GetBlocks(_) => 64,
            ProtocolMessage::Extensible(ext) => ext.data.len() + 128,
            _ => 128,
        };

        BASE_OVERHEAD + payload_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::{
        inv_payload::InvPayload, ping_payload::PingPayload, InventoryType,
    };
    use crate::UInt256;

    #[test]
    fn duplicate_single_command_is_detected() {
        let mut queue = OutboundMessageQueue::default();
        queue.push_back(NetworkMessage::new(ProtocolMessage::Ping(
            PingPayload::create_with_nonce(1, 42),
        )));

        assert!(queue.has_duplicate_single_command(MessageCommand::Ping));
    }

    #[test]
    fn non_single_command_is_not_deduplicated() {
        let hash = UInt256::from([7u8; 32]);
        let message = NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
            InventoryType::Block,
            &[hash],
        )));
        let command = message.command();
        let mut queue = OutboundMessageQueue::default();
        queue.push_back(message);

        assert!(!queue.has_duplicate_single_command(command));
    }

    #[test]
    fn popped_single_command_can_be_queued_again() {
        let mut queue = OutboundMessageQueue::default();
        queue.push_back(NetworkMessage::new(ProtocolMessage::Ping(
            PingPayload::create_with_nonce(1, 42),
        )));
        assert!(queue.has_duplicate_single_command(MessageCommand::Ping));

        let popped = queue.pop_front().expect("queued ping");
        assert_eq!(popped.command(), MessageCommand::Ping);
        assert!(!queue.has_duplicate_single_command(MessageCommand::Ping));

        queue.push_back(NetworkMessage::new(ProtocolMessage::Ping(
            PingPayload::create_with_nonce(2, 42),
        )));
        assert!(queue.has_duplicate_single_command(MessageCommand::Ping));
    }

    #[test]
    fn inventory_message_estimate_scales_with_hash_count() {
        let hashes = [UInt256::from([1u8; 32]), UInt256::from([2u8; 32])];
        let message = NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
            InventoryType::Transaction,
            &hashes,
        )));

        assert_eq!(RemoteNode::estimate_message_size(&message), 64 + 2 * 32);
    }
}
