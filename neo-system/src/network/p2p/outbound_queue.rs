//! Outbound message queueing with per-peer memory quota and priority lanes.
//!
//! Mirrors `neo-core::network::p2p::remote_node::outbound_queue`. Implements
//! a two-lane (high/low priority) bounded queue with duplicate detection for
//! single-queued commands and a byte-budget to prevent memory exhaustion.
//!
//! Uses a `[bool; 256]` array instead of `bitvec` to stay within the allowed
//! dependency set.

use super::message_command::MessageCommand;
use super::protocol_message::{NetworkMessage, ProtocolMessage};
use neo_io_crate::Serializable;
use std::collections::VecDeque;

/// Number of bits in the message command domain (one byte = 256 values).
const MESSAGE_COMMAND_DOMAIN_BITS: usize = 256;

// ---------------------------------------------------------------------------
// CommandBitSet
// ---------------------------------------------------------------------------

/// Compact set of message commands using a flat boolean array.
///
/// Tracks which commands are currently queued or have been sent, supporting
/// the single-queued deduplication and sent-command consumption patterns.
#[derive(Debug, Clone)]
pub struct CommandBitSet([bool; MESSAGE_COMMAND_DOMAIN_BITS]);

impl Default for CommandBitSet {
    fn default() -> Self {
        Self([false; MESSAGE_COMMAND_DOMAIN_BITS])
    }
}

impl CommandBitSet {
    /// Returns `true` if the command is currently tracked.
    pub fn contains(&self, command: MessageCommand) -> bool {
        self.0[Self::slot(command)]
    }

    /// Marks a command as present.
    pub fn insert(&mut self, command: MessageCommand) {
        self.0[Self::slot(command)] = true;
    }

    /// Clears a command's presence flag.
    pub fn remove(&mut self, command: MessageCommand) {
        self.0[Self::slot(command)] = false;
    }

    /// Removes and returns the previous presence state.
    pub fn take(&mut self, command: MessageCommand) -> bool {
        let slot = Self::slot(command);
        let was_present = self.0[slot];
        self.0[slot] = false;
        was_present
    }

    fn slot(command: MessageCommand) -> usize {
        command.to_byte() as usize
    }
}

// ---------------------------------------------------------------------------
// QueuePushError
// ---------------------------------------------------------------------------

/// Reason a message could not be enqueued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePushError {
    /// The target lane is at its message count capacity.
    Full,
    /// The global per-peer byte budget would be exceeded.
    MemoryLimit,
    /// A single-queued command is already pending in the lane.
    Duplicate,
}

// ---------------------------------------------------------------------------
// OutboundMessageQueue (single lane)
// ---------------------------------------------------------------------------

/// A queued message with its estimated wire byte cost.
struct QueuedOutboundMessage {
    message: NetworkMessage,
    estimated_bytes: usize,
}

/// Single-priority outbound message lane.
#[derive(Default)]
pub struct OutboundMessageQueue {
    messages: VecDeque<QueuedOutboundMessage>,
    queued_single_commands: CommandBitSet,
}

impl OutboundMessageQueue {
    /// Number of messages currently queued.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
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

// ---------------------------------------------------------------------------
// OutboundQueues (dual-lane)
// ---------------------------------------------------------------------------

/// Dual-priority outbound queue with per-peer memory accounting.
///
/// High-priority lane is drained first. Each lane enforces a message count
/// limit; both share a global byte budget.
#[derive(Default)]
pub struct OutboundQueues {
    high: OutboundMessageQueue,
    low: OutboundMessageQueue,
    queued_bytes: usize,
}

impl OutboundQueues {
    /// Pushes a message into the appropriate priority lane.
    ///
    /// Returns `Err` with the reason if the message cannot be queued.
    pub fn push_back(
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

    /// Pops the next message (high-priority lane first, then low).
    pub fn pop_front(&mut self) -> Option<NetworkMessage> {
        let queued = self.high.pop_front().or_else(|| self.low.pop_front())?;
        self.queued_bytes = self.queued_bytes.saturating_sub(queued.estimated_bytes);
        Some(queued.message)
    }

    /// Total estimated bytes currently queued across both lanes.
    pub fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }

    /// Total messages queued across both lanes.
    pub fn len(&self) -> usize {
        self.high.len() + self.low.len()
    }

    /// Whether both lanes are empty.
    pub fn is_empty(&self) -> bool {
        self.high.is_empty() && self.low.is_empty()
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

// ---------------------------------------------------------------------------
// Message size estimation
// ---------------------------------------------------------------------------

/// Base overhead per message (framing, allocation slack).
const BASE_OVERHEAD: usize = 64;
/// Fixed overhead for payloads whose size is not variable.
const FIXED_PAYLOAD_OVERHEAD: usize = 128;

/// Estimates the wire byte cost of a network message for memory accounting.
///
/// Variable-length payloads (blocks, headers, transactions) use their real
/// serialized size; fixed-shape payloads use a constant estimate.
pub fn estimate_message_size(message: &NetworkMessage) -> usize {
    let payload_size = match &message.payload {
        ProtocolMessage::Block(block) => block.size(),
        ProtocolMessage::Headers(headers) => headers
            .headers
            .iter()
            .map(Serializable::size)
            .sum::<usize>(),
        ProtocolMessage::Transaction(tx) => tx.size(),
        ProtocolMessage::Inv(inv) => inv.hashes.len() * 32,
        ProtocolMessage::GetData(inv) => inv.hashes.len() * 32,
        ProtocolMessage::GetBlocks(_) => 64,
        ProtocolMessage::Extensible(ext) => ext.data.len() + FIXED_PAYLOAD_OVERHEAD,
        _ => FIXED_PAYLOAD_OVERHEAD,
    };

    BASE_OVERHEAD + payload_size
}

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

/// Maximum number of messages allowed in each queue lane.
pub const MAX_QUEUE_SIZE: usize = 1024;

/// Maximum memory usage per peer in bytes (8 MB).
pub const MAX_MEMORY_PER_PEER: usize = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::{InvPayload, PingPayload};
    use neo_primitives::{InventoryType, UInt256};

    const TEST_MAX_QUEUE_BYTES: usize = usize::MAX;

    fn ping(nonce: u32) -> NetworkMessage {
        NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(
            nonce, 42,
        )))
    }

    fn push_queues(
        queues: &mut OutboundQueues,
        message: NetworkMessage,
        max_messages_per_lane: usize,
    ) -> Result<(), QueuePushError> {
        queues.push_back(message, max_messages_per_lane, TEST_MAX_QUEUE_BYTES)
    }

    #[test]
    fn command_bit_set_insert_contains_take() {
        let mut set = CommandBitSet::default();
        let cmd = MessageCommand::Ping;

        assert!(!set.contains(cmd));
        set.insert(cmd);
        assert!(set.contains(cmd));
        assert!(set.take(cmd));
        assert!(!set.contains(cmd));
        assert!(!set.take(cmd));
    }

    #[test]
    fn command_bit_set_tracks_unknown_commands() {
        let mut set = CommandBitSet::default();
        let cmd = MessageCommand::Unknown(0xFF);

        assert!(!set.contains(cmd));
        set.insert(cmd);
        assert!(set.contains(cmd));
        assert!(set.take(cmd));
        assert!(!set.contains(cmd));
    }

    #[test]
    fn duplicate_single_command_is_detected() {
        let mut queues = OutboundQueues::default();
        push_queues(&mut queues, ping(1), 1024).unwrap();

        let err =
            push_queues(&mut queues, ping(2), 1024).expect_err("duplicate single-queued command");
        assert_eq!(err, QueuePushError::Duplicate);
    }

    #[test]
    fn non_single_command_is_not_deduplicated() {
        let hash = UInt256::from([7u8; 32]);
        let msg = NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
            InventoryType::Block,
            &[hash],
        )));
        let mut queues = OutboundQueues::default();

        push_queues(&mut queues, msg.clone(), 1024).unwrap();
        push_queues(&mut queues, msg, 1024).unwrap();
        assert_eq!(queues.len(), 2);
    }

    #[test]
    fn popped_single_command_can_be_queued_again() {
        let mut queues = OutboundQueues::default();
        push_queues(&mut queues, ping(1), 1024).unwrap();

        let popped = queues.pop_front().expect("queued ping");
        assert_eq!(popped.command(), MessageCommand::Ping);

        push_queues(&mut queues, ping(2), 1024).unwrap();
        assert_eq!(queues.len(), 1);
    }

    #[test]
    fn high_priority_pops_before_low() {
        let mut queues = OutboundQueues::default();

        push_queues(&mut queues, ping(1), 1024).unwrap(); // low priority
        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::FilterClear),
            1024,
        )
        .unwrap(); // high priority

        assert_eq!(
            queues.pop_front().expect("first").command(),
            MessageCommand::FilterClear
        );
        assert_eq!(
            queues.pop_front().expect("second").command(),
            MessageCommand::Ping
        );
        assert!(queues.pop_front().is_none());
    }

    #[test]
    fn capacity_is_per_lane() {
        let mut queues = OutboundQueues::default();

        // Fill low lane with 2 Reject messages.
        for i in 0..2u8 {
            push_queues(
                &mut queues,
                NetworkMessage::new(ProtocolMessage::Reject(vec![i])),
                2,
            )
            .unwrap();
        }
        // Low lane full.
        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::Reject(vec![99])),
            2,
        )
        .expect_err("low lane should be full");

        // High lane still has room.
        push_queues(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::FilterClear),
            2,
        )
        .unwrap();
    }

    #[test]
    fn memory_limit_rejects_without_mutating_state() {
        let mut queues = OutboundQueues::default();
        let first = ping(1);
        let first_size = estimate_message_size(&first);

        let err = queues
            .push_back(first, 1024, first_size - 1)
            .expect_err("byte limit should reject");

        assert_eq!(err, QueuePushError::MemoryLimit);
        assert_eq!(queues.queued_bytes(), 0);
        assert!(queues.is_empty());
    }

    #[test]
    fn pop_releases_byte_estimates() {
        let mut queues = OutboundQueues::default();
        let msg = NetworkMessage::new(ProtocolMessage::Reject(vec![1]));
        let msg_size = estimate_message_size(&msg);

        queues.push_back(msg, 1024, msg_size * 2).unwrap();
        assert_eq!(queues.queued_bytes(), msg_size);

        queues.pop_front().unwrap();
        assert_eq!(queues.queued_bytes(), 0);
    }

    #[test]
    fn inventory_message_estimate_scales_with_hash_count() {
        let hashes = [UInt256::from([1u8; 32]), UInt256::from([2u8; 32])];
        let message = NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
            InventoryType::Transaction,
            &hashes,
        )));

        assert_eq!(estimate_message_size(&message), BASE_OVERHEAD + 2 * 32);
    }
}
