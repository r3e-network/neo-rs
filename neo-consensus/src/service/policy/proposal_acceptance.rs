//! `PrepareRequest` acceptance policy.
//!
//! Before a received `PrepareRequest` reaches the dBFT state machine, the node
//! screens it against the round it is currently running: height, previous
//! hash, timestamp monotonicity, clock skew and the transaction-count cap.
//! These are consensus rules, not transport concerns, so they live here and
//! the node only supplies the current round facts.

use crate::ConsensusMessageType;
use crate::context::MAX_PREPARE_REQUEST_FUTURE_MS_FACTOR;
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use neo_primitives::UInt256;

/// Facts about the round that is currently running on this node.
#[derive(Debug, Clone, Copy)]
pub struct PrepareRequestPrecheck<'a> {
    /// Payload received from the network.
    pub payload: &'a ConsensusPayload,
    /// Block index of the current round, if one has been started.
    pub expected_block_index: Option<u32>,
    /// Previous block hash of the current round, when known.
    pub expected_prev_hash: Option<UInt256>,
    /// Timestamp of the previous block.
    pub prev_timestamp: u64,
    /// Local wall-clock time in milliseconds.
    pub now_ms: u64,
    /// Expected block time in milliseconds.
    pub time_per_block_ms: u64,
    /// Maximum number of transactions allowed in one block.
    pub max_transactions_per_block: usize,
}

/// Reason a `PrepareRequest` was screened out before dBFT processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareRequestRejection {
    /// No round is running, so no proposal can be accepted.
    NoActiveRound,
    /// The proposal targets a different height.
    HeightMismatch,
    /// The proposal body could not be decoded for the claimed round.
    Malformed,
    /// The proposal carries more transactions than the block allows.
    TooManyTransactions,
    /// The proposal does not extend the current tip.
    PreviousHashMismatch,
    /// The proposal timestamp does not advance past the previous block.
    TimestampNotIncreasing,
    /// The proposal timestamp is too far ahead of the local clock.
    TimestampTooFarInFuture,
}

/// Screens a received payload before it reaches the consensus state machine.
///
/// Payloads other than `PrepareRequest` are always accepted (`Ok(())`); the
/// message-level handler performs their validation.
///
/// # Errors
///
/// Returns the first [`PrepareRequestRejection`] that applies.
pub fn precheck_prepare_request(
    check: &PrepareRequestPrecheck<'_>,
) -> Result<(), PrepareRequestRejection> {
    let payload = check.payload;
    if payload.message_type != ConsensusMessageType::PrepareRequest {
        return Ok(());
    }

    let Some(current_index) = check.expected_block_index else {
        return Err(PrepareRequestRejection::NoActiveRound);
    };
    if payload.block_index != current_index {
        return Err(PrepareRequestRejection::HeightMismatch);
    }

    let message = PrepareRequestMessage::deserialize_body(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    )
    .map_err(|_| PrepareRequestRejection::Malformed)?;

    if message.transaction_hashes.len() > check.max_transactions_per_block {
        return Err(PrepareRequestRejection::TooManyTransactions);
    }

    if let Some(prev_hash) = check.expected_prev_hash
        && message.prev_hash != prev_hash
    {
        return Err(PrepareRequestRejection::PreviousHashMismatch);
    }

    if message.timestamp <= check.prev_timestamp {
        return Err(PrepareRequestRejection::TimestampNotIncreasing);
    }

    let max_future = check
        .time_per_block_ms
        .saturating_mul(MAX_PREPARE_REQUEST_FUTURE_MS_FACTOR);
    if message.timestamp > check.now_ms.saturating_add(max_future) {
        return Err(PrepareRequestRejection::TimestampTooFarInFuture);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETWORK: u32 = 860_833_102;
    const BLOCK_INDEX: u32 = 42;
    const PREV_TIMESTAMP: u64 = 1_000_000;
    const NOW_MS: u64 = 1_015_000;
    const TIME_PER_BLOCK_MS: u64 = 15_000;

    fn prev_hash() -> UInt256 {
        UInt256::from([0x11; 32])
    }

    fn tx_hash(index: u8) -> UInt256 {
        UInt256::from([index; 32])
    }

    fn prepare_request(timestamp: u64, prev: UInt256, transactions: usize) -> ConsensusPayload {
        let hashes = (0..transactions)
            .map(|index| tx_hash(index as u8))
            .collect();
        prepare_request_with_hashes(timestamp, prev, hashes)
    }

    fn prepare_request_with_hashes(
        timestamp: u64,
        prev: UInt256,
        hashes: Vec<UInt256>,
    ) -> ConsensusPayload {
        let message = PrepareRequestMessage::new(BLOCK_INDEX, 0, 0, 0, prev, timestamp, 1, hashes);
        ConsensusPayload::new(
            NETWORK,
            BLOCK_INDEX,
            0,
            0,
            ConsensusMessageType::PrepareRequest,
            message.serialize(),
        )
    }

    fn check<'a>(payload: &'a ConsensusPayload) -> PrepareRequestPrecheck<'a> {
        PrepareRequestPrecheck {
            payload,
            expected_block_index: Some(BLOCK_INDEX),
            expected_prev_hash: Some(prev_hash()),
            prev_timestamp: PREV_TIMESTAMP,
            now_ms: NOW_MS,
            time_per_block_ms: TIME_PER_BLOCK_MS,
            max_transactions_per_block: 4,
        }
    }

    #[test]
    fn accepts_a_proposal_that_matches_the_running_round() {
        let payload = prepare_request(PREV_TIMESTAMP + 1, prev_hash(), 4);
        assert_eq!(precheck_prepare_request(&check(&payload)), Ok(()));
    }

    #[test]
    fn rejects_proposals_without_a_running_round() {
        let payload = prepare_request(PREV_TIMESTAMP + 1, prev_hash(), 1);
        let mut facts = check(&payload);
        facts.expected_block_index = None;

        assert_eq!(
            precheck_prepare_request(&facts),
            Err(PrepareRequestRejection::NoActiveRound)
        );
    }

    #[test]
    fn rejects_proposals_for_another_height() {
        let payload = prepare_request(PREV_TIMESTAMP + 1, prev_hash(), 1);
        let mut facts = check(&payload);
        facts.expected_block_index = Some(BLOCK_INDEX + 1);

        assert_eq!(
            precheck_prepare_request(&facts),
            Err(PrepareRequestRejection::HeightMismatch)
        );
    }

    #[test]
    fn rejects_oversized_proposals() {
        let payload = prepare_request(PREV_TIMESTAMP + 1, prev_hash(), 5);

        assert_eq!(
            precheck_prepare_request(&check(&payload)),
            Err(PrepareRequestRejection::TooManyTransactions)
        );
    }

    #[test]
    fn rejects_bodies_that_do_not_decode() {
        // A repeated transaction hash makes the body invalid before any round
        // rule applies, so the screen reports it as malformed.
        let payload = prepare_request_with_hashes(
            PREV_TIMESTAMP + 1,
            prev_hash(),
            vec![tx_hash(1), tx_hash(1)],
        );

        assert_eq!(
            precheck_prepare_request(&check(&payload)),
            Err(PrepareRequestRejection::Malformed)
        );
    }

    #[test]
    fn rejects_proposals_that_do_not_extend_the_tip() {
        let payload = prepare_request(PREV_TIMESTAMP + 1, UInt256::from([0x33; 32]), 1);

        assert_eq!(
            precheck_prepare_request(&check(&payload)),
            Err(PrepareRequestRejection::PreviousHashMismatch)
        );
    }

    #[test]
    fn rejects_non_increasing_timestamps() {
        let payload = prepare_request(PREV_TIMESTAMP, prev_hash(), 1);

        assert_eq!(
            precheck_prepare_request(&check(&payload)),
            Err(PrepareRequestRejection::TimestampNotIncreasing)
        );
    }

    #[test]
    fn rejects_timestamps_beyond_the_future_tolerance() {
        let tolerated = prepare_request(
            NOW_MS + TIME_PER_BLOCK_MS * MAX_PREPARE_REQUEST_FUTURE_MS_FACTOR,
            prev_hash(),
            1,
        );
        assert_eq!(precheck_prepare_request(&check(&tolerated)), Ok(()));

        let beyond = prepare_request(
            NOW_MS + TIME_PER_BLOCK_MS * MAX_PREPARE_REQUEST_FUTURE_MS_FACTOR + 1,
            prev_hash(),
            1,
        );
        assert_eq!(
            precheck_prepare_request(&check(&beyond)),
            Err(PrepareRequestRejection::TimestampTooFarInFuture)
        );
    }

    #[test]
    fn screens_only_prepare_requests() {
        let payload = ConsensusPayload::new(
            NETWORK,
            BLOCK_INDEX,
            0,
            0,
            ConsensusMessageType::Commit,
            vec![0xA5; 64],
        );
        let mut facts = check(&payload);
        // Even with every fact wrong, a non-PrepareRequest payload passes the
        // screen: its own handler validates it.
        facts.expected_block_index = None;
        facts.expected_prev_hash = None;

        assert_eq!(precheck_prepare_request(&facts), Ok(()));
    }
}
