//! Outbound message queueing and per-peer memory quota handling.

use super::RemoteNode;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::MessageCommand;
use crate::runtime::ActorResult;
use std::collections::VecDeque;
use std::time::Instant;
use tracing::warn;

const MESSAGE_COMMAND_DOMAIN_BYTES: usize = 32;

#[derive(Default)]
pub(super) struct CommandBitSet([u8; MESSAGE_COMMAND_DOMAIN_BYTES]);

impl CommandBitSet {
    pub(super) fn contains(&self, command: MessageCommand) -> bool {
        let (byte_index, mask) = Self::slot(command);
        self.0[byte_index] & mask != 0
    }

    pub(super) fn insert(&mut self, command: MessageCommand) {
        let (byte_index, mask) = Self::slot(command);
        self.0[byte_index] |= mask;
    }

    pub(super) fn remove(&mut self, command: MessageCommand) {
        let (byte_index, mask) = Self::slot(command);
        self.0[byte_index] &= !mask;
    }

    pub(super) fn take(&mut self, command: MessageCommand) -> bool {
        let was_present = self.contains(command);
        self.remove(command);
        was_present
    }

    fn slot(command: MessageCommand) -> (usize, u8) {
        let index = command.to_byte() as usize;
        (index / 8, 1u8 << (index % 8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueuePushError {
    Full,
    Duplicate,
}

pub(super) struct OutboundMessageQueue {
    messages: VecDeque<NetworkMessage>,
    queued_single_commands: CommandBitSet,
}

impl Default for OutboundMessageQueue {
    fn default() -> Self {
        Self {
            messages: VecDeque::new(),
            queued_single_commands: CommandBitSet::default(),
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
        command.is_single_queued() && self.queued_single_commands.contains(command)
    }

    fn record_queued_single_command(&mut self, command: MessageCommand) {
        if command.is_single_queued() {
            self.queued_single_commands.insert(command);
        }
    }

    fn clear_queued_single_command(&mut self, command: MessageCommand) {
        if command.is_single_queued() {
            self.queued_single_commands.remove(command);
        }
    }
}

pub(super) struct OutboundQueues {
    high: OutboundMessageQueue,
    low: OutboundMessageQueue,
}

impl Default for OutboundQueues {
    fn default() -> Self {
        Self {
            high: OutboundMessageQueue::default(),
            low: OutboundMessageQueue::default(),
        }
    }
}

impl OutboundQueues {
    fn push_back(
        &mut self,
        message: NetworkMessage,
        max_queue_size: usize,
    ) -> Result<(), QueuePushError> {
        let command = message.command();
        let queue = self.lane_mut(command);
        if queue.len() >= max_queue_size {
            return Err(QueuePushError::Full);
        }
        if queue.has_duplicate_single_command(command) {
            return Err(QueuePushError::Duplicate);
        }
        queue.push_back(message);
        Ok(())
    }

    fn pop_front(&mut self) -> Option<NetworkMessage> {
        self.high.pop_front().or_else(|| self.low.pop_front())
    }

    fn lane_mut(&mut self, command: MessageCommand) -> &mut OutboundMessageQueue {
        if command.is_high_priority_queue() {
            &mut self.high
        } else {
            &mut self.low
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

        match self.message_queues.push_back(message, Self::MAX_QUEUE_SIZE) {
            Ok(()) => self.add_memory_usage(message_size),
            Err(QueuePushError::Full) => {
                warn!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    command = ?command,
                    "message queue full, dropping message"
                );
                return Ok(());
            }
            Err(QueuePushError::Duplicate) => return Ok(()),
        }

        self.flush_queue().await
    }

    pub(super) async fn flush_queue(&mut self) -> ActorResult {
        if !self.verack_received || !self.ack_ready {
            return Ok(());
        }

        while self.verack_received && self.ack_ready {
            let Some(message) = self.message_queues.pop_front() else {
                break;
            };

            let message_size = Self::estimate_message_size(&message);
            self.release_memory_usage(message_size);

            self.ack_ready = false;
            self.last_sent = Instant::now();
            self.sent_commands.insert(message.command());
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
    fn command_bit_set_tracks_unknown_command_bytes() {
        let mut commands = CommandBitSet::default();
        let command = MessageCommand::Unknown(0xff);

        assert!(!commands.contains(command));
        commands.insert(command);
        assert!(commands.contains(command));
        assert!(commands.take(command));
        assert!(!commands.contains(command));
        assert!(!commands.take(command));
    }

    #[test]
    fn command_bit_sets_are_independent_boolean_trackers() {
        let mut queued = CommandBitSet::default();
        let mut sent = CommandBitSet::default();

        queued.insert(MessageCommand::GetAddr);
        sent.insert(MessageCommand::GetAddr);

        assert!(queued.take(MessageCommand::GetAddr));
        assert!(!queued.contains(MessageCommand::GetAddr));
        assert!(sent.contains(MessageCommand::GetAddr));
        assert!(sent.take(MessageCommand::GetAddr));
        assert!(!sent.take(MessageCommand::GetAddr));
    }

    #[test]
    fn outbound_queues_pop_high_priority_before_low_fifo_within_lane() {
        let mut queues = OutboundQueues::default();

        queues
            .push_back(
                NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(1, 42))),
                1024,
            )
            .unwrap();
        queues
            .push_back(NetworkMessage::new(ProtocolMessage::FilterClear), 1024)
            .unwrap();
        queues
            .push_back(NetworkMessage::new(ProtocolMessage::Mempool), 1024)
            .unwrap();

        assert_eq!(
            queues.pop_front().expect("first").command(),
            MessageCommand::FilterClear
        );
        assert_eq!(
            queues.pop_front().expect("second").command(),
            MessageCommand::Mempool
        );
        assert_eq!(
            queues.pop_front().expect("third").command(),
            MessageCommand::Ping
        );
        assert!(queues.pop_front().is_none());
    }

    #[test]
    fn outbound_queues_capacity_is_per_lane() {
        let mut queues = OutboundQueues::default();

        for nonce in 0..2 {
            queues
                .push_back(NetworkMessage::new(ProtocolMessage::Reject(vec![nonce])), 2)
                .unwrap();
        }
        queues
            .push_back(NetworkMessage::new(ProtocolMessage::Reject(vec![99])), 2)
            .expect_err("low lane should be full");

        for _ in 0..2 {
            queues
                .push_back(NetworkMessage::new(ProtocolMessage::FilterClear), 2)
                .unwrap();
        }
        queues
            .push_back(NetworkMessage::new(ProtocolMessage::FilterClear), 2)
            .expect_err("high lane should be full");
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
