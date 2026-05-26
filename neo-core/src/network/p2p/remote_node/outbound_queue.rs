//! Outbound message queueing and per-peer memory quota handling.

use super::RemoteNode;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::MessageCommand;
use crate::runtime::ActorResult;
use bitvec::{array::BitArray, order::Lsb0};
use std::collections::VecDeque;
use std::time::Instant;
use tracing::warn;

const MESSAGE_COMMAND_DOMAIN_BITS: usize = 256;

#[derive(Default)]
pub(super) struct CommandBitSet(BitArray<[u8; 32], Lsb0>);

impl CommandBitSet {
    pub(super) fn contains(&self, command: MessageCommand) -> bool {
        self.0
            .get(Self::slot(command))
            .map(|bit| *bit)
            .unwrap_or(false)
    }

    pub(super) fn insert(&mut self, command: MessageCommand) {
        self.0.set(Self::slot(command), true);
    }

    pub(super) fn remove(&mut self, command: MessageCommand) {
        self.0.set(Self::slot(command), false);
    }

    pub(super) fn take(&mut self, command: MessageCommand) -> bool {
        let was_present = self.contains(command);
        self.remove(command);
        was_present
    }

    fn slot(command: MessageCommand) -> usize {
        let index = command.to_byte() as usize;
        debug_assert!(index < MESSAGE_COMMAND_DOMAIN_BITS);
        index
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueuePushError {
    Full,
    MemoryLimit,
    Duplicate,
}

pub(super) struct QueuedOutboundMessage {
    message: NetworkMessage,
    estimated_bytes: usize,
}

pub(super) struct OutboundMessageQueue {
    messages: VecDeque<QueuedOutboundMessage>,
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

    fn push_back(&mut self, message: NetworkMessage, estimated_bytes: usize) {
        self.record_queued_single_command(message.command());
        self.messages.push_back(QueuedOutboundMessage {
            message,
            estimated_bytes,
        });
    }

    fn pop_front(&mut self) -> Option<QueuedOutboundMessage> {
        let queued = self.messages.pop_front()?;
        self.clear_queued_single_command(queued.message.command());
        Some(queued)
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
    queued_bytes: usize,
}

impl Default for OutboundQueues {
    fn default() -> Self {
        Self {
            high: OutboundMessageQueue::default(),
            low: OutboundMessageQueue::default(),
            queued_bytes: 0,
        }
    }
}

impl OutboundQueues {
    fn push_back(
        &mut self,
        message: NetworkMessage,
        max_messages_per_lane: usize,
        max_queued_bytes: usize,
    ) -> Result<(), QueuePushError> {
        let command = message.command();
        let queue = self.lane(command);
        if queue.has_duplicate_single_command(command) {
            return Err(QueuePushError::Duplicate);
        }
        if queue.len() >= max_messages_per_lane {
            return Err(QueuePushError::Full);
        }
        let estimated_bytes = estimate_message_size(&message);
        if self.queued_bytes.saturating_add(estimated_bytes) > max_queued_bytes {
            return Err(QueuePushError::MemoryLimit);
        }
        self.lane_mut(command).push_back(message, estimated_bytes);
        self.queued_bytes = self.queued_bytes.saturating_add(estimated_bytes);
        Ok(())
    }

    fn pop_front(&mut self) -> Option<NetworkMessage> {
        let queued = self.high.pop_front().or_else(|| self.low.pop_front())?;
        self.queued_bytes = self.queued_bytes.saturating_sub(queued.estimated_bytes);
        Some(queued.message)
    }

    fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }

    fn lane(&self, command: MessageCommand) -> &OutboundMessageQueue {
        if command.is_high_priority_queue() {
            &self.high
        } else {
            &self.low
        }
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

        match self.message_queues.push_back(
            message,
            Self::MAX_QUEUE_SIZE,
            Self::MAX_MEMORY_PER_PEER,
        ) {
            Ok(()) => {}
            Err(QueuePushError::Full) => {
                warn!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    command = ?command,
                    "message queue full, dropping message"
                );
                return Ok(());
            }
            Err(QueuePushError::MemoryLimit) => {
                warn!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    command = ?command,
                    current_usage = self.message_queues.queued_bytes(),
                    max_allowed = Self::MAX_MEMORY_PER_PEER,
                    "per-peer outbound queue memory quota exceeded, dropping message"
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

            self.ack_ready = false;
            self.last_sent = Instant::now();
            self.sent_commands.insert(message.command());
            self.send_wire_message(&message).await?;
            self.ack_ready = true;
        }

        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::{
        inv_payload::InvPayload, ping_payload::PingPayload, InventoryType,
    };
    use crate::UInt256;

    const TEST_MAX_QUEUE_BYTES: usize = usize::MAX;

    fn push_lane(queue: &mut OutboundMessageQueue, message: NetworkMessage) {
        let estimated_bytes = estimate_message_size(&message);
        queue.push_back(message, estimated_bytes);
    }

    fn push_queues(
        queues: &mut OutboundQueues,
        message: NetworkMessage,
        max_messages_per_lane: usize,
    ) -> Result<(), QueuePushError> {
        queues.push_back(message, max_messages_per_lane, TEST_MAX_QUEUE_BYTES)
    }

    fn ping(nonce: u32) -> NetworkMessage {
        NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(
            nonce, 42,
        )))
    }

    #[test]
    fn duplicate_single_command_is_detected() {
        let mut queue = OutboundMessageQueue::default();
        push_lane(&mut queue, ping(1));

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
        push_lane(&mut queue, message);

        assert!(!queue.has_duplicate_single_command(command));
    }

    #[test]
    fn popped_single_command_can_be_queued_again() {
        let mut queue = OutboundMessageQueue::default();
        push_lane(&mut queue, ping(1));
        assert!(queue.has_duplicate_single_command(MessageCommand::Ping));

        let popped = queue.pop_front().expect("queued ping");
        assert_eq!(popped.message.command(), MessageCommand::Ping);
        assert!(!queue.has_duplicate_single_command(MessageCommand::Ping));

        push_lane(&mut queue, ping(2));
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

        push_queues(&mut queues, ping(1), 1024).unwrap();
        queues
            .push_back(
                NetworkMessage::new(ProtocolMessage::FilterClear),
                1024,
                TEST_MAX_QUEUE_BYTES,
            )
            .unwrap();
        queues
            .push_back(
                NetworkMessage::new(ProtocolMessage::Mempool),
                1024,
                TEST_MAX_QUEUE_BYTES,
            )
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
            push_queues(
                &mut queues,
                NetworkMessage::new(ProtocolMessage::Reject(vec![nonce])),
                2,
            )
            .unwrap();
        }
        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::Reject(vec![99])),
            2,
        )
        .expect_err("low lane should be full");

        for _ in 0..2 {
            push_queues(
                &mut queues,
                NetworkMessage::new(ProtocolMessage::FilterClear),
                2,
            )
            .unwrap();
        }
        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::FilterClear),
            2,
        )
        .expect_err("high lane should be full");
    }

    #[test]
    fn duplicate_single_command_wins_over_full_queue() {
        let mut queues = OutboundQueues::default();

        push_queues(&mut queues, ping(1), 1).unwrap();

        let err = push_queues(&mut queues, ping(2), 1)
            .expect_err("duplicate single command should be detected before capacity");

        assert_eq!(err, QueuePushError::Duplicate);
    }

    #[test]
    fn non_duplicate_message_still_reports_full_queue() {
        let mut queues = OutboundQueues::default();

        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::Reject(vec![1])),
            1,
        )
        .unwrap();

        let err = push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::Reject(vec![2])),
            1,
        )
        .expect_err("non-duplicate message should report capacity");

        assert_eq!(err, QueuePushError::Full);
    }

    #[test]
    fn outbound_queues_memory_limit_rejects_without_mutating_state() {
        let mut queues = OutboundQueues::default();
        let first_ping = ping(1);
        let first_ping_size = estimate_message_size(&first_ping);

        let err = queues
            .push_back(first_ping, 1024, first_ping_size - 1)
            .expect_err("byte limit should reject even when count capacity remains");

        assert_eq!(err, QueuePushError::MemoryLimit);
        assert_eq!(queues.queued_bytes(), 0);
        assert!(queues.pop_front().is_none());

        queues
            .push_back(ping(1), 1024, first_ping_size)
            .expect("failed memory-limit attempt must not record a duplicate");
        assert_eq!(queues.queued_bytes(), first_ping_size);
    }

    #[test]
    fn duplicate_single_command_wins_over_memory_limit() {
        let mut queues = OutboundQueues::default();
        let first_ping = ping(1);
        let first_ping_size = estimate_message_size(&first_ping);

        queues
            .push_back(first_ping, 1024, first_ping_size)
            .expect("first ping fits exactly");

        let err = queues
            .push_back(ping(2), 1024, first_ping_size)
            .expect_err("duplicate check should run before byte-limit check");

        assert_eq!(err, QueuePushError::Duplicate);
        assert_eq!(queues.queued_bytes(), first_ping_size);
    }

    #[test]
    fn outbound_queue_pop_releases_stored_byte_estimates() {
        let mut queues = OutboundQueues::default();
        let low = NetworkMessage::new(ProtocolMessage::Reject(vec![1]));
        let high = NetworkMessage::new(ProtocolMessage::FilterClear);
        let low_size = estimate_message_size(&low);
        let high_size = estimate_message_size(&high);
        let byte_limit = low_size + high_size;

        queues.push_back(low, 1024, byte_limit).unwrap();
        queues.push_back(high, 1024, byte_limit).unwrap();
        assert_eq!(queues.queued_bytes(), byte_limit);

        assert_eq!(
            queues.pop_front().expect("high priority first").command(),
            MessageCommand::FilterClear
        );
        assert_eq!(queues.queued_bytes(), low_size);

        assert_eq!(
            queues.pop_front().expect("low priority second").command(),
            MessageCommand::Reject
        );
        assert_eq!(queues.queued_bytes(), 0);

        assert!(queues.pop_front().is_none());
        assert_eq!(queues.queued_bytes(), 0);
    }

    #[test]
    fn high_priority_messages_do_not_bypass_global_byte_limit() {
        let mut queues = OutboundQueues::default();
        let high = NetworkMessage::new(ProtocolMessage::FilterClear);
        let high_size = estimate_message_size(&high);

        let err = queues
            .push_back(high, 1024, high_size - 1)
            .expect_err("high priority queue should still obey global byte cap");

        assert_eq!(err, QueuePushError::MemoryLimit);
        assert_eq!(queues.queued_bytes(), 0);
    }

    #[test]
    fn inventory_message_estimate_scales_with_hash_count() {
        let hashes = [UInt256::from([1u8; 32]), UInt256::from([2u8; 32])];
        let message = NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
            InventoryType::Transaction,
            &hashes,
        )));

        assert_eq!(estimate_message_size(&message), 64 + 2 * 32);
    }
}
