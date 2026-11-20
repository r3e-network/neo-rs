//! Outbound message queueing, bounded batch coalescing, and per-peer memory
//! quota handling.
//!
//! Every outbound message enters a two-lane queue (high/low priority) and leaves
//! it through exactly one of two write paths:
//!
//! * `Immediate` — one message per `send_message` + `flush` write window. The
//!   handshake, consensus payloads, liveness probes, bloom-filter state changes,
//!   and every request or reply a peer can time always take this path.
//! * `Batchable` — up to [`MAX_BATCH_MESSAGES`] messages or [`MAX_BATCH_BYTES`]
//!   of serialized payload per `send_messages_batch` vectored write, held for at
//!   most [`MAX_BATCH_DELAY`] so sparse announcement traffic still coalesces.
//!
//! Ordering is the queue's FIFO order across both paths. A pending batch always
//! holds the oldest messages, so a newer immediate message is written only after
//! that batch, and a batchable message never overtakes an immediate message that
//! is already at the front of the queue.

use super::RemoteNode;
use crate::p2p::MessageCommand;
use crate::p2p::message_command::OutboundClass;
use crate::p2p::messages::{NetworkMessage, ProtocolMessage};
use bitvec::{array::BitArray, order::Lsb0};
use neo_actors::{ActorContext, ActorResult};
use neo_io_crate::Serializable;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tracing::warn;

const MESSAGE_COMMAND_DOMAIN_BITS: usize = 256;

/// Maximum number of messages coalesced into one vectored write.
pub(super) const MAX_BATCH_MESSAGES: usize = 16;

/// Maximum serialized payload budget carried by one pending batch (64 KiB).
pub(super) const MAX_BATCH_BYTES: usize = 64 * 1024;

/// Longest a sparse batch may wait for company before it is written (5 ms).
pub(super) const MAX_BATCH_DELAY: Duration = Duration::from_millis(5);

#[derive(Default)]
pub(super) struct CommandBitSet(BitArray<[u8; 32], Lsb0>);

impl CommandBitSet {
    pub(super) fn contains(&self, command: MessageCommand) -> bool {
        self.0.get(Self::slot(command)).is_some_and(|bit| *bit)
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

#[derive(Debug)]
pub(super) struct QueuedOutboundMessage {
    pub(super) message: NetworkMessage,
    /// Bytes reserved in the peer ledger for this message while it is queued.
    pub(super) estimated_bytes: usize,
    /// Write class chosen at the enqueue site.
    pub(super) class: OutboundClass,
}

#[derive(Default)]
pub(super) struct OutboundMessageQueue {
    messages: VecDeque<QueuedOutboundMessage>,
    queued_single_commands: CommandBitSet,
}

impl OutboundMessageQueue {
    fn len(&self) -> usize {
        self.messages.len()
    }

    fn front(&self) -> Option<&QueuedOutboundMessage> {
        self.messages.front()
    }

    fn push_back(&mut self, message: NetworkMessage, estimated_bytes: usize, class: OutboundClass) {
        self.record_queued_single_command(message.command());
        self.messages.push_back(QueuedOutboundMessage {
            message,
            estimated_bytes,
            class,
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

/// Why a coalesced batch left the accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchFlushReason {
    /// The message-count budget was reached.
    CountThreshold,
    /// The serialized-payload budget was reached or would be exceeded.
    ByteThreshold,
    /// The bounded delay for sparse traffic elapsed.
    Timer,
    /// An immediate message arrived behind the pending batch.
    Urgent,
    /// The connection is shutting down and the batch is drained.
    Shutdown,
}

impl BatchFlushReason {
    pub(super) fn is_threshold(self) -> bool {
        matches!(self, Self::CountThreshold | Self::ByteThreshold)
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::CountThreshold => "count_threshold",
            Self::ByteThreshold => "byte_threshold",
            Self::Timer => "timer",
            Self::Urgent => "urgent",
            Self::Shutdown => "shutdown",
        }
    }
}

/// One coalesced batch that has left the accumulator.
#[derive(Debug)]
pub(super) struct PendingBatchTaken {
    pub(super) messages: Vec<NetworkMessage>,
    /// Serialized payload bytes carried by the batch.
    pub(super) payload_bytes: usize,
    pub(super) reason: BatchFlushReason,
}

/// Telemetry snapshot describing the outbound batching behaviour of one peer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OutboundBatchStats {
    /// Coalesced batches written with `send_messages_batch`.
    pub batches: u64,
    /// Messages carried by those batches.
    pub batch_messages: u64,
    /// Serialized payload bytes carried by those batches.
    pub batch_bytes: u64,
    /// Batches flushed because the delay window closed.
    pub timer_flushes: u64,
    /// Batches flushed because the count or byte budget was reached.
    pub threshold_flushes: u64,
    /// Batches flushed because a newer immediate message arrived.
    pub urgent_flushes: u64,
    /// Batches drained while the connection was shutting down.
    pub shutdown_flushes: u64,
    /// Immediate messages written in their own write window.
    pub immediate_writes: u64,
    /// Write calls issued for outbound messages (immediate plus batch).
    pub write_calls: u64,
    /// Outbound messages written (immediate plus batched).
    pub messages_written: u64,
    /// Write calls avoided by coalescing (`messages_written - write_calls`).
    pub write_calls_saved: u64,
}

/// Messages coalesced for a single vectored write.
///
/// The batch keeps the ledger reservation of every member message, so the
/// serialized frames waiting for the write stay inside the peer byte budget
/// instead of being dropped from the accounting when they leave the queue.
#[derive(Default)]
pub(super) struct PendingBatch {
    messages: Vec<NetworkMessage>,
    payload_bytes: usize,
    reservation_bytes: usize,
    deadline: Option<Instant>,
}

impl PendingBatch {
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.messages.len()
    }

    #[cfg(test)]
    fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    fn is_due(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    fn remaining_delay(&self, now: Instant) -> Duration {
        self.deadline()
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or_default()
    }

    /// Returns true when `wire_bytes` still fits the count and byte budgets.
    ///
    /// An empty batch always accepts the next message, even when that single
    /// message exceeds the byte budget: an oversized block still has to be
    /// written, and it then flushes as a one-message batch.
    fn can_accept(&self, wire_bytes: usize) -> bool {
        if self.messages.len() >= MAX_BATCH_MESSAGES {
            return false;
        }
        if self.messages.is_empty() {
            return true;
        }
        self.payload_bytes.saturating_add(wire_bytes) <= MAX_BATCH_BYTES
    }

    fn push(
        &mut self,
        message: NetworkMessage,
        wire_bytes: usize,
        reservation_bytes: usize,
        now: Instant,
    ) {
        if self.messages.is_empty() {
            self.deadline = Some(now + MAX_BATCH_DELAY);
        }
        self.messages.push(message);
        self.payload_bytes = self.payload_bytes.saturating_add(wire_bytes);
        self.reservation_bytes = self.reservation_bytes.saturating_add(reservation_bytes);
    }

    /// Returns the threshold that closed the batch, if any.
    fn readiness(&self) -> Option<BatchFlushReason> {
        if self.messages.len() >= MAX_BATCH_MESSAGES {
            Some(BatchFlushReason::CountThreshold)
        } else if self.payload_bytes >= MAX_BATCH_BYTES {
            Some(BatchFlushReason::ByteThreshold)
        } else {
            None
        }
    }

    fn take(&mut self) -> (Vec<NetworkMessage>, usize, usize) {
        let messages = std::mem::take(&mut self.messages);
        let payload_bytes = std::mem::take(&mut self.payload_bytes);
        let reservation_bytes = std::mem::take(&mut self.reservation_bytes);
        self.deadline = None;
        (messages, payload_bytes, reservation_bytes)
    }
}

/// One decision from the outbound drain state machine.
#[derive(Debug)]
pub(super) enum DrainStep {
    /// No queued work and no pending batch.
    Idle,
    /// A pending batch exists whose delay window is still open.
    Defer,
    /// Front message must be written in its own write window.
    SendImmediate(QueuedOutboundMessage),
    /// Coalesced batch ready for a single vectored write.
    SendBatch(PendingBatchTaken),
}

#[derive(Default)]
pub(super) struct OutboundQueues {
    high: OutboundMessageQueue,
    low: OutboundMessageQueue,
    pending_batch: PendingBatch,
    queued_bytes: usize,
}

impl OutboundQueues {
    /// Queues a message with an explicit outbound class.
    fn push_back_as(
        &mut self,
        message: NetworkMessage,
        class: OutboundClass,
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
        self.lane_mut(command)
            .push_back(message, estimated_bytes, class);
        self.queued_bytes = self.queued_bytes.saturating_add(estimated_bytes);
        Ok(())
    }

    fn front(&self) -> Option<&QueuedOutboundMessage> {
        self.high.front().or_else(|| self.low.front())
    }

    fn pop_front_queued(&mut self) -> Option<QueuedOutboundMessage> {
        self.high.pop_front().or_else(|| self.low.pop_front())
    }

    fn take_batch(&mut self, reason: BatchFlushReason) -> PendingBatchTaken {
        let (messages, payload_bytes, reservation_bytes) = self.pending_batch.take();
        self.queued_bytes = self.queued_bytes.saturating_sub(reservation_bytes);
        PendingBatchTaken {
            messages,
            payload_bytes,
            reason,
        }
    }

    /// Takes the pending batch regardless of its delay window (shutdown drain).
    fn take_pending_batch(&mut self) -> Option<PendingBatchTaken> {
        if self.pending_batch.is_empty() {
            return None;
        }
        Some(self.take_batch(BatchFlushReason::Shutdown))
    }

    /// Decides the next outbound write.
    ///
    /// The walk follows queue FIFO order: batchable fronts join the pending
    /// batch, an immediate front flushes that batch first and then leaves on its
    /// own, and the walk stops only when the queue is empty or a batch is waiting
    /// for its delay window.
    fn next_drain_step(&mut self, now: Instant) -> DrainStep {
        loop {
            let Some(front) = self.front() else {
                if self.pending_batch.is_empty() {
                    return DrainStep::Idle;
                }
                if self.pending_batch.is_due(now) {
                    return DrainStep::SendBatch(self.take_batch(BatchFlushReason::Timer));
                }
                return DrainStep::Defer;
            };

            let class = front.class;
            let wire_bytes = front.message.wire_size();

            match class {
                OutboundClass::Immediate => {
                    // The pending batch is older than anything still queued.
                    if !self.pending_batch.is_empty() {
                        return DrainStep::SendBatch(self.take_batch(BatchFlushReason::Urgent));
                    }
                    let queued = self
                        .pop_front_queued()
                        .expect("front message must be present");
                    self.queued_bytes = self.queued_bytes.saturating_sub(queued.estimated_bytes);
                    return DrainStep::SendImmediate(queued);
                }
                OutboundClass::Batchable => {
                    if !self.pending_batch.can_accept(wire_bytes) {
                        let reason = self
                            .pending_batch
                            .readiness()
                            .unwrap_or(BatchFlushReason::ByteThreshold);
                        return DrainStep::SendBatch(self.take_batch(reason));
                    }

                    let queued = self
                        .pop_front_queued()
                        .expect("front message must be present");
                    self.pending_batch.push(
                        queued.message,
                        wire_bytes,
                        queued.estimated_bytes,
                        now,
                    );

                    if let Some(reason) = self.pending_batch.readiness() {
                        return DrainStep::SendBatch(self.take_batch(reason));
                    }
                }
            }
        }
    }

    fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }

    fn pending_batch_is_empty(&self) -> bool {
        self.pending_batch.is_empty()
    }

    #[cfg(test)]
    fn pending_batch_len(&self) -> usize {
        self.pending_batch.len()
    }

    #[cfg(test)]
    fn pending_batch_payload_bytes(&self) -> usize {
        self.pending_batch.payload_bytes()
    }

    #[cfg(test)]
    fn pending_batch_deadline(&self) -> Option<Instant> {
        self.pending_batch.deadline()
    }

    fn pending_batch_remaining_delay(&self, now: Instant) -> Duration {
        self.pending_batch.remaining_delay(now)
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

    /// Queues a message using its command-level outbound class.
    pub(super) async fn enqueue_message(&mut self, message: NetworkMessage) -> ActorResult {
        let class = message.command().outbound_class();
        self.enqueue_message_as(message, class).await
    }

    /// Queues a message that must be written in its own write window.
    ///
    /// Request answers use this even when the command is batchable by default,
    /// because the peer is waiting on that reply.
    pub(super) async fn enqueue_immediate(&mut self, message: NetworkMessage) -> ActorResult {
        self.enqueue_message_as(message, OutboundClass::Immediate)
            .await
    }

    async fn enqueue_message_as(
        &mut self,
        message: NetworkMessage,
        class: OutboundClass,
    ) -> ActorResult {
        let command = message.command();

        match self.message_queues.push_back_as(
            message,
            class,
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

    /// Writes everything the queue can release, in queue order.
    pub(super) async fn flush_queue(&mut self) -> ActorResult {
        if !self.verack_received || !self.ack_ready {
            return Ok(());
        }

        loop {
            if !self.verack_received || !self.ack_ready {
                break;
            }

            match self.message_queues.next_drain_step(Instant::now()) {
                DrainStep::Idle | DrainStep::Defer => break,
                DrainStep::SendImmediate(queued) => {
                    self.ack_ready = false;
                    let result = self.send_wire_message(&queued.message).await;
                    self.ack_ready = true;
                    result?;
                    self.record_immediate_written();
                }
                DrainStep::SendBatch(taken) => self.send_batch(taken).await?,
            }
        }

        Ok(())
    }

    /// Writes one coalesced batch with a single vectored write call.
    async fn send_batch(&mut self, taken: PendingBatchTaken) -> ActorResult {
        let count = taken.messages.len();
        self.ack_ready = false;
        for message in &taken.messages {
            self.sent_commands.insert(message.command());
        }
        self.last_sent = Instant::now();

        let result = {
            let mut connection = self.connection.lock().await;
            connection.send_messages_batch(&taken.messages).await
        };

        self.ack_ready = true;

        match result {
            Ok(()) => {
                self.record_batch_written(taken.reason, count, taken.payload_bytes);
                Ok(())
            }
            Err(err) => {
                if err.is_timeout() {
                    crate::p2p::timeouts::inc_write_timeout();
                }
                // The batch was already removed from the queue, so a failed
                // write is never retried: the actor tears the connection down
                // instead of duplicating frames.
                warn!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    messages = count,
                    bytes = taken.payload_bytes,
                    error = %err,
                    "failed to write outbound message batch"
                );
                Err(neo_actors::ActorRuntimeError::system(err.to_string()))
            }
        }
    }

    /// Writes any pending batch before the connection closes (best effort).
    pub(super) async fn drain_pending_batch(&mut self) {
        self.cancel_batch_timer();
        let Some(taken) = self.message_queues.take_pending_batch() else {
            return;
        };
        if !self.verack_received {
            tracing::debug!(
                target: "neo",
                endpoint = %self.endpoint,
                messages = taken.messages.len(),
                "discarding pending outbound batch: the handshake never completed"
            );
            return;
        }

        let count = taken.messages.len();
        let result = {
            let mut connection = self.connection.lock().await;
            connection.send_messages_batch(&taken.messages).await
        };

        match result {
            Ok(()) => {
                self.record_batch_written(taken.reason, count, taken.payload_bytes);
                tracing::debug!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    reason = taken.reason.as_str(),
                    messages = count,
                    bytes = taken.payload_bytes,
                    "drained pending outbound batch during shutdown"
                );
            }
            Err(err) => {
                warn!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    messages = count,
                    error = %err,
                    "failed to drain outbound batch during shutdown"
                );
            }
        }
    }

    /// Arms the one-shot timer that closes the pending batch delay window.
    ///
    /// The timer is skipped while the acknowledgement gate is closed: the queue
    /// cannot write anything in that state, so re-arming a zero delay would
    /// spin the scheduler.
    pub(super) fn sync_batch_timer(&mut self, ctx: &ActorContext) {
        if !self.verack_received || !self.ack_ready || self.message_queues.pending_batch_is_empty()
        {
            self.cancel_batch_timer();
            return;
        }
        if self.batch_timer.is_some() {
            return;
        }

        let delay = self
            .message_queues
            .pending_batch_remaining_delay(Instant::now());
        self.batch_generation = self.batch_generation.wrapping_add(1);
        let handle = ctx.schedule_tell_once_cancelable(
            delay,
            &ctx.self_ref(),
            super::RemoteNodeCommand::FlushBatch {
                generation: self.batch_generation,
            },
            None,
        );
        self.batch_timer = Some(handle);
    }

    pub(super) fn cancel_batch_timer(&mut self) {
        if let Some(timer) = self.batch_timer.take() {
            timer.cancel();
            // Invalidate the cancelled timer so a message that was already
            // enqueued by the scheduler is recognised as stale.
            self.batch_generation = self.batch_generation.wrapping_add(1);
        }
    }

    /// Returns the outbound batching telemetry snapshot for this peer.
    pub fn outbound_batch_stats(&self) -> OutboundBatchStats {
        self.batch_stats
    }

    fn record_immediate_written(&mut self) {
        self.batch_stats.immediate_writes += 1;
        self.batch_stats.messages_written += 1;
        self.batch_stats.write_calls += 1;
        self.refresh_write_calls_saved();
    }

    fn record_batch_written(
        &mut self,
        reason: BatchFlushReason,
        messages: usize,
        payload_bytes: usize,
    ) {
        self.batch_stats.batches += 1;
        self.batch_stats.batch_messages += messages as u64;
        self.batch_stats.batch_bytes += payload_bytes as u64;
        if reason.is_threshold() {
            self.batch_stats.threshold_flushes += 1;
        }
        self.batch_stats.timer_flushes += u64::from(reason == BatchFlushReason::Timer);
        self.batch_stats.urgent_flushes += u64::from(reason == BatchFlushReason::Urgent);
        self.batch_stats.shutdown_flushes += u64::from(reason == BatchFlushReason::Shutdown);
        self.batch_stats.messages_written += messages as u64;
        self.batch_stats.write_calls += 1;
        self.refresh_write_calls_saved();
    }

    fn refresh_write_calls_saved(&mut self) {
        self.batch_stats.write_calls_saved = self
            .batch_stats
            .messages_written
            .saturating_sub(self.batch_stats.write_calls);
    }
}

fn estimate_message_size(message: &NetworkMessage) -> usize {
    const BASE_OVERHEAD: usize = 64;
    const FIXED_PAYLOAD_OVERHEAD: usize = 128;

    // Review §5.1: the per-peer byte budget must reflect what the message
    // will actually retain on the wire, not fixed guesses (a Block guessed
    // at 2 KiB can serialize to hundreds of KiB). Variable payloads use
    // their real serialized size; fixed-shape payloads keep exact-size
    // arithmetic.
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

#[cfg(test)]
#[path = "outbound_queue_tests.rs"]
mod tests;
