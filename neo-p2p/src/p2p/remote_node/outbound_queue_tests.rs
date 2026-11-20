//! Deterministic tests for the bounded outbound batching state machine.
//!
//! Every test drives [`OutboundQueues::next_drain_step`] with an explicit
//! `Instant`, so count, byte, and delay behaviour is verified without sleeping
//! and without depending on the wall clock.

use super::*;
use crate::p2p::inventory_type::InventoryType;
use crate::p2p::payloads::{InvPayload, PingPayload};
use neo_primitives::UInt256;
use std::time::Duration;

/// Message cap per lane used unless a test is exercising the quota itself.
const QUEUE_LIMIT: usize = 1024;
/// Byte quota used unless a test is exercising the quota itself.
const BYTE_LIMIT: usize = usize::MAX;

/// One action produced by the drain state machine.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// A single message written in its own write window.
    Immediate(MessageCommand),
    /// A coalesced batch with the threshold that closed it.
    Batch(Vec<MessageCommand>, BatchFlushReason),
}

fn ping(nonce: u32) -> NetworkMessage {
    NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(
        1, nonce,
    )))
}

/// Builds an `inv` announcement carrying `hashes` distinct transaction hashes.
fn inv_with_hashes(hashes: usize, seed: u8) -> NetworkMessage {
    let hashes: Vec<UInt256> = (0..hashes)
        .map(|index| {
            let mut bytes = [seed; 32];
            bytes[8..12].copy_from_slice(&(index as u32).to_le_bytes());
            UInt256::from(bytes)
        })
        .collect();
    NetworkMessage::new(ProtocolMessage::Inv(InvPayload::create(
        InventoryType::Transaction,
        &hashes,
    )))
}

fn inv(seed: u8) -> NetworkMessage {
    inv_with_hashes(1, seed)
}

fn push(queues: &mut OutboundQueues, message: NetworkMessage) -> Result<(), QueuePushError> {
    let class = message.command().outbound_class();
    queues.push_back_as(message, class, QUEUE_LIMIT, BYTE_LIMIT)
}

fn push_with_limits(
    queues: &mut OutboundQueues,
    message: NetworkMessage,
    max_messages: usize,
    max_bytes: usize,
) -> Result<(), QueuePushError> {
    let class = message.command().outbound_class();
    queues.push_back_as(message, class, max_messages, max_bytes)
}

/// Drains everything releasable at `now` and records the write order.
fn drain(queues: &mut OutboundQueues, now: Instant) -> Vec<Action> {
    let mut actions = Vec::new();
    loop {
        match queues.next_drain_step(now) {
            DrainStep::Idle | DrainStep::Defer => break,
            DrainStep::SendImmediate(queued) => {
                actions.push(Action::Immediate(queued.message.command()));
            }
            DrainStep::SendBatch(taken) => actions.push(Action::Batch(
                taken.messages.iter().map(NetworkMessage::command).collect(),
                taken.reason,
            )),
        }
    }
    actions
}

#[test]
fn count_threshold_flushes_sixteen_messages_as_one_batch() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    for seed in 0..MAX_BATCH_MESSAGES as u8 {
        push(&mut queues, inv(seed)).expect("queue accepts inv");
    }

    let actions = drain(&mut queues, now);

    assert_eq!(
        actions,
        vec![Action::Batch(
            vec![MessageCommand::Inv; MAX_BATCH_MESSAGES],
            BatchFlushReason::CountThreshold
        )]
    );
    assert!(queues.pending_batch_is_empty());
    assert_eq!(queues.queued_bytes(), 0);
}

#[test]
fn count_cap_splits_twenty_messages_into_a_full_batch_and_a_remainder() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    for seed in 0..20u8 {
        push(&mut queues, inv(seed)).expect("queue accepts inv");
    }

    let actions = drain(&mut queues, now);

    assert_eq!(
        actions,
        vec![Action::Batch(
            vec![MessageCommand::Inv; MAX_BATCH_MESSAGES],
            BatchFlushReason::CountThreshold
        )]
    );
    assert_eq!(queues.pending_batch_len(), 20 - MAX_BATCH_MESSAGES);
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
}

#[test]
fn byte_threshold_flushes_before_the_budget_is_exceeded() {
    let now = Instant::now();
    let single = inv_with_hashes(500, 1);
    let wire_bytes = single.wire_size();
    assert!(
        4 * wire_bytes <= MAX_BATCH_BYTES,
        "four maximum-size announcements must fit the batch budget"
    );
    assert!(
        5 * wire_bytes > MAX_BATCH_BYTES,
        "a fifth announcement must not fit the batch budget"
    );

    let mut queues = OutboundQueues::default();
    for seed in 0..5u8 {
        push(&mut queues, inv_with_hashes(500, seed)).expect("queue accepts inv");
    }

    let actions = drain(&mut queues, now);

    assert_eq!(
        actions,
        vec![Action::Batch(
            vec![MessageCommand::Inv; 4],
            BatchFlushReason::ByteThreshold
        )]
    );
    // The message that did not fit starts the next pending batch.
    assert_eq!(queues.pending_batch_len(), 1);
    assert_eq!(queues.pending_batch_payload_bytes(), wire_bytes);
}

#[test]
fn byte_budget_counts_every_message_already_in_the_batch() {
    let now = Instant::now();
    let wire_bytes = inv_with_hashes(500, 0).wire_size();
    let mut queues = OutboundQueues::default();

    push(&mut queues, inv_with_hashes(500, 0)).expect("queue accepts inv");
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    assert_eq!(queues.pending_batch_payload_bytes(), wire_bytes);

    push(&mut queues, inv_with_hashes(500, 1)).expect("queue accepts inv");
    push(&mut queues, inv_with_hashes(500, 2)).expect("queue accepts inv");
    push(&mut queues, inv_with_hashes(500, 3)).expect("queue accepts inv");
    // Four messages still fit; the batch must not have been released early.
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    assert_eq!(queues.pending_batch_payload_bytes(), 4 * wire_bytes);
}

#[test]
fn pending_batch_accepts_one_oversized_message_and_flushes_it_alone() {
    let now = Instant::now();
    let mut batch = PendingBatch::default();

    // An empty batch always accepts: an oversized block still has to be written.
    assert!(batch.can_accept(MAX_BATCH_BYTES * 2));
    batch.push(inv(1), MAX_BATCH_BYTES * 2, 1024, now);
    assert_eq!(batch.readiness(), Some(BatchFlushReason::ByteThreshold));
    assert!(!batch.can_accept(1), "the budget is already spent");
}

#[test]
fn sparse_batch_defers_until_the_delay_window_closes() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");

    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    let deadline = queues.pending_batch_deadline().expect("deadline armed");
    assert_eq!(deadline, now + MAX_BATCH_DELAY);
    assert!(MAX_BATCH_DELAY <= Duration::from_millis(5));

    assert!(matches!(
        queues.next_drain_step(deadline - Duration::from_micros(1)),
        DrainStep::Defer
    ));

    let step = queues.next_drain_step(deadline);
    let DrainStep::SendBatch(taken) = step else {
        panic!("expected a timer flush, got {step:?}");
    };
    assert_eq!(taken.reason, BatchFlushReason::Timer);
    assert_eq!(taken.messages.len(), 1);
    assert_eq!(taken.messages[0].command(), MessageCommand::Inv);
    assert!(queues.pending_batch_is_empty());
    assert_eq!(queues.queued_bytes(), 0);
}

#[test]
fn urgent_message_flushes_the_older_batch_before_it_is_written() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");
    push(&mut queues, inv(2)).expect("queue accepts inv");
    push(&mut queues, ping(7)).expect("queue accepts ping");

    let actions = drain(&mut queues, now);

    assert_eq!(
        actions,
        vec![
            Action::Batch(vec![MessageCommand::Inv; 2], BatchFlushReason::Urgent),
            Action::Immediate(MessageCommand::Ping),
        ]
    );
    assert!(queues.pending_batch_is_empty());
}

#[test]
fn batchable_message_never_overtakes_a_queued_immediate() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, ping(1)).expect("queue accepts ping");
    push(&mut queues, inv(1)).expect("queue accepts inv");

    let actions = drain(&mut queues, now);

    assert_eq!(actions, vec![Action::Immediate(MessageCommand::Ping)]);
    assert_eq!(queues.pending_batch_len(), 1);
}

#[test]
fn strict_order_is_preserved_for_interleaved_classes() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");
    push(&mut queues, inv(2)).expect("queue accepts inv");
    push(&mut queues, ping(1)).expect("queue accepts ping");
    push(&mut queues, NetworkMessage::new(ProtocolMessage::Verack)).expect("queue accepts verack");

    let actions = drain(&mut queues, now);

    // Both announcements precede both immediate messages, and the message that
    // triggered the urgent flush precedes the one queued behind it.
    assert_eq!(
        actions,
        vec![
            Action::Batch(vec![MessageCommand::Inv; 2], BatchFlushReason::Urgent),
            Action::Immediate(MessageCommand::Ping),
            Action::Immediate(MessageCommand::Verack),
        ]
    );
}

#[test]
fn forced_immediate_class_writes_a_relay_payload_alone() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    queues
        .push_back_as(inv(1), OutboundClass::Immediate, QUEUE_LIMIT, BYTE_LIMIT)
        .expect("queue accepts inv");

    let actions = drain(&mut queues, now);

    assert_eq!(actions, vec![Action::Immediate(MessageCommand::Inv)]);
    assert!(queues.pending_batch_is_empty());
}

#[test]
fn ledger_keeps_the_pending_batch_reservation_until_it_is_written() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");
    push(&mut queues, inv(2)).expect("queue accepts inv");
    let reserved = queues.queued_bytes();
    assert!(reserved > 0);

    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    // Messages live in the pending batch now, but their retained serialized
    // buffers stay inside the peer byte quota.
    assert_eq!(queues.queued_bytes(), reserved);

    let deadline = queues.pending_batch_deadline().expect("deadline armed");
    assert!(matches!(
        queues.next_drain_step(deadline),
        DrainStep::SendBatch(_)
    ));
    assert_eq!(queues.queued_bytes(), 0);
}

#[test]
fn byte_quota_counts_messages_retained_in_the_pending_batch() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    let single = estimate_message_size(&inv(1));
    let quota = single + single / 2;

    push_with_limits(&mut queues, inv(1), QUEUE_LIMIT, quota).expect("first message fits");
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    assert_eq!(queues.queued_bytes(), single);

    assert_eq!(
        push_with_limits(&mut queues, inv(2), QUEUE_LIMIT, quota),
        Err(QueuePushError::MemoryLimit)
    );

    let deadline = queues.pending_batch_deadline().expect("deadline armed");
    assert!(matches!(
        queues.next_drain_step(deadline),
        DrainStep::SendBatch(_)
    ));
    assert_eq!(queues.queued_bytes(), 0);
    push_with_limits(&mut queues, inv(2), QUEUE_LIMIT, quota)
        .expect("quota released with the batch");
}

#[test]
fn shutdown_drain_takes_the_pending_batch_once() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");
    push(&mut queues, inv(2)).expect("queue accepts inv");
    push(&mut queues, inv(3)).expect("queue accepts inv");
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));

    let taken = queues.take_pending_batch().expect("pending batch drained");
    assert_eq!(taken.reason, BatchFlushReason::Shutdown);
    assert_eq!(taken.messages.len(), 3);
    assert_eq!(taken.payload_bytes, 3 * inv(1).wire_size());
    assert!(queues.take_pending_batch().is_none());
    assert!(matches!(queues.next_drain_step(now), DrainStep::Idle));
    assert_eq!(queues.queued_bytes(), 0);
}

#[test]
fn flushed_batch_leaves_nothing_to_replay_after_a_failed_write() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("queue accepts inv");
    push(&mut queues, inv(2)).expect("queue accepts inv");
    assert!(matches!(queues.next_drain_step(now), DrainStep::Defer));
    let deadline = queues.pending_batch_deadline().expect("deadline armed");

    let DrainStep::SendBatch(taken) = queues.next_drain_step(deadline) else {
        panic!("expected the delay window to close the batch");
    };
    assert_eq!(taken.messages.len(), 2);

    // A write failure tears the connection down; the batch was removed from the
    // queue before the write, so it can never be written twice.
    assert!(matches!(queues.next_drain_step(now), DrainStep::Idle));
    assert!(queues.take_pending_batch().is_none());
    assert_eq!(queues.queued_bytes(), 0);
}

#[test]
fn duplicate_single_command_is_detected() {
    let mut queues = OutboundQueues::default();
    push(&mut queues, ping(1)).expect("first ping");

    assert_eq!(push(&mut queues, ping(2)), Err(QueuePushError::Duplicate));
}

#[test]
fn non_single_command_is_not_deduplicated() {
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("first inv");

    push(&mut queues, inv(1)).expect("identical inv is still distinct work");
}

#[test]
fn popped_single_command_can_be_queued_again() {
    let now = Instant::now();
    let mut queues = OutboundQueues::default();
    push(&mut queues, ping(1)).expect("first ping");
    assert_eq!(
        drain(&mut queues, now),
        vec![Action::Immediate(MessageCommand::Ping)]
    );

    push(&mut queues, ping(2)).expect("the single-queued slot was released");
}

#[test]
fn command_bit_set_tracks_unknown_command_bytes() {
    let mut bits = CommandBitSet::default();
    let command = MessageCommand::Unknown(0x99);

    assert!(!bits.contains(command));
    bits.insert(command);
    assert!(bits.contains(command));
    assert!(bits.take(command));
    assert!(!bits.take(command));
}

#[test]
fn command_bit_sets_are_independent_boolean_trackers() {
    let mut first = CommandBitSet::default();
    let mut second = CommandBitSet::default();

    first.insert(MessageCommand::Ping);
    assert!(first.contains(MessageCommand::Ping));
    assert!(!second.contains(MessageCommand::Ping));

    second.insert(MessageCommand::Ping);
    first.remove(MessageCommand::Ping);
    assert!(!first.contains(MessageCommand::Ping));
    assert!(second.contains(MessageCommand::Ping));
}

#[test]
fn outbound_queues_pop_high_priority_before_low_priority_and_keep_lane_fifo() {
    let mut queues = OutboundQueues::default();
    push(&mut queues, inv(1)).expect("low priority inv");
    push(&mut queues, inv(2)).expect("low priority inv");
    push(&mut queues, NetworkMessage::new(ProtocolMessage::Mempool))
        .expect("high priority mempool");

    assert_eq!(
        queues
            .pop_front_queued()
            .expect("high lane first")
            .message
            .command(),
        MessageCommand::Mempool
    );
    assert_eq!(
        queues
            .pop_front_queued()
            .expect("low lane fifo")
            .message
            .command(),
        MessageCommand::Inv
    );
    assert_eq!(
        queues
            .pop_front_queued()
            .expect("low lane fifo")
            .message
            .command(),
        MessageCommand::Inv
    );
    assert!(queues.pop_front_queued().is_none());
}

#[test]
fn outbound_queues_capacity_is_per_lane() {
    let mut queues = OutboundQueues::default();
    push_with_limits(&mut queues, inv(1), 1, BYTE_LIMIT).expect("low lane has room");
    assert_eq!(
        push_with_limits(&mut queues, inv(2), 1, BYTE_LIMIT),
        Err(QueuePushError::Full)
    );

    push_with_limits(
        &mut queues,
        NetworkMessage::new(ProtocolMessage::Mempool),
        1,
        BYTE_LIMIT,
    )
    .expect("high lane quota is independent");
}

#[test]
fn duplicate_single_command_wins_over_full_queue() {
    let mut queues = OutboundQueues::default();
    push_with_limits(&mut queues, ping(1), 1, BYTE_LIMIT).expect("lane has room");
    assert_eq!(
        push_with_limits(&mut queues, ping(2), 1, BYTE_LIMIT),
        Err(QueuePushError::Duplicate)
    );
}

#[test]
fn non_duplicate_message_still_reports_full_queue() {
    let mut queues = OutboundQueues::default();
    push_with_limits(&mut queues, ping(1), 1, BYTE_LIMIT).expect("lane has room");
    assert_eq!(
        push_with_limits(&mut queues, inv(1), 1, BYTE_LIMIT),
        Err(QueuePushError::Full)
    );
}

#[test]
fn outbound_queues_memory_limit_rejects_without_mutating_state() {
    let mut queues = OutboundQueues::default();
    let single = estimate_message_size(&inv(1));
    push_with_limits(&mut queues, inv(1), QUEUE_LIMIT, single).expect("exactly at the limit");
    assert_eq!(queues.queued_bytes(), single);

    assert_eq!(
        push_with_limits(&mut queues, inv_with_hashes(500, 2), QUEUE_LIMIT, single),
        Err(QueuePushError::MemoryLimit)
    );
    assert_eq!(queues.queued_bytes(), single);
}

#[test]
fn duplicate_single_command_wins_over_memory_limit() {
    let mut queues = OutboundQueues::default();
    let single = estimate_message_size(&ping(1));
    push_with_limits(&mut queues, ping(1), QUEUE_LIMIT, single).expect("fits the byte quota");

    assert_eq!(
        push_with_limits(&mut queues, ping(2), QUEUE_LIMIT, single),
        Err(QueuePushError::Duplicate)
    );
}

#[test]
fn high_priority_messages_do_not_bypass_the_global_byte_limit() {
    let mut queues = OutboundQueues::default();
    let single = estimate_message_size(&inv(1));
    push_with_limits(&mut queues, inv(1), QUEUE_LIMIT, single).expect("fits the byte quota");

    assert_eq!(
        push_with_limits(
            &mut queues,
            NetworkMessage::new(ProtocolMessage::Mempool),
            QUEUE_LIMIT,
            single
        ),
        Err(QueuePushError::MemoryLimit)
    );
}

#[test]
fn ledger_reservation_covers_the_serialized_wire_size() {
    // The pending batch keeps the queue's ledger charge, so the reserved bytes
    // must dominate the frame that is eventually written.
    let messages = [
        inv(1),
        inv_with_hashes(500, 3),
        ping(1),
        NetworkMessage::new(ProtocolMessage::Verack),
    ];
    for message in messages {
        assert!(
            estimate_message_size(&message) >= message.wire_size(),
            "{:?} reservation must cover its wire size",
            message.command()
        );
    }
}

#[test]
fn inventory_message_estimate_scales_with_hash_count() {
    let single = estimate_message_size(&inv(1));
    let full = estimate_message_size(&inv_with_hashes(500, 1));

    assert_eq!(full - single, 499 * 32);
    assert!(full > single);
}

#[test]
fn batch_flush_reasons_classify_thresholds_and_render_stable_labels() {
    assert!(BatchFlushReason::CountThreshold.is_threshold());
    assert!(BatchFlushReason::ByteThreshold.is_threshold());
    for reason in [
        BatchFlushReason::Timer,
        BatchFlushReason::Urgent,
        BatchFlushReason::Shutdown,
    ] {
        assert!(!reason.is_threshold());
    }

    assert_eq!(BatchFlushReason::CountThreshold.as_str(), "count_threshold");
    assert_eq!(BatchFlushReason::ByteThreshold.as_str(), "byte_threshold");
    assert_eq!(BatchFlushReason::Timer.as_str(), "timer");
    assert_eq!(BatchFlushReason::Urgent.as_str(), "urgent");
    assert_eq!(BatchFlushReason::Shutdown.as_str(), "shutdown");
}
