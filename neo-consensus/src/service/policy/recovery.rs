//! Recovery handling policy.
//!
//! When a node restarts it may find a durably stored consensus context. The
//! rules for deciding whether that record may resume the current round, what
//! the node should do immediately afterwards, and whether it still has to ask
//! its peers for recovery data are consensus decisions and live here. Reading
//! the record itself (store or durable file) stays with the node adapter.

use crate::context::ConsensusContext;

/// Decision for a durably loaded consensus context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryLoadOutcome {
    /// The record belongs to the round that is about to start and may resume it.
    Resume,
    /// The record belongs to another height and must be ignored.
    HeightMismatch {
        /// Height stored in the recovery record.
        recovered: u32,
        /// Height the node is about to run.
        expected: u32,
    },
}

/// Decides whether a loaded recovery context may resume `expected_block_index`.
///
/// A record from any other height is stale: resuming it would replay signatures
/// bound to a different block.
#[must_use]
pub fn evaluate_recovery_context(
    context: &ConsensusContext,
    expected_block_index: u32,
) -> RecoveryLoadOutcome {
    if context.block_index == expected_block_index {
        RecoveryLoadOutcome::Resume
    } else {
        RecoveryLoadOutcome::HeightMismatch {
            recovered: context.block_index,
            expected: expected_block_index,
        }
    }
}

/// First action after a resumed consensus service has been installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryResumeAction {
    /// The record holds no proposed transactions, so re-announce the empty
    /// proposal and continue from there.
    AnnounceEmptyProposal,
    /// The record already names the proposed transactions, so rebuild the
    /// local `PrepareResponse` from the recovered proposal.
    RebuildPrepareResponse,
}

/// Chooses the post-resume action for a recovered round.
#[must_use]
pub fn recovery_resume_action(context: &ConsensusContext) -> RecoveryResumeAction {
    if context.proposed_tx_hashes.is_empty() {
        RecoveryResumeAction::AnnounceEmptyProposal
    } else {
        RecoveryResumeAction::RebuildPrepareResponse
    }
}

/// Returns `true` when the node must ask peers for recovery data on start.
///
/// A round in which the local validator already committed is final from this
/// node's point of view; asking for recovery data again only adds traffic.
#[must_use]
pub fn should_request_recovery_on_start(context: &ConsensusContext) -> bool {
    let local_commit_sent = context
        .my_index
        .is_some_and(|index| context.commits.contains_key(&index));
    !local_commit_sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ValidatorInfo;
    use neo_primitives::UInt160;

    fn validators() -> Vec<ValidatorInfo> {
        vec![
            ValidatorInfo {
                index: 0,
                public_key: neo_crypto::ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
                script_hash: UInt160::zero(),
            },
            ValidatorInfo {
                index: 1,
                public_key: neo_crypto::ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
                script_hash: UInt160::zero(),
            },
        ]
    }

    #[test]
    fn stale_recovery_height_is_reported() {
        let context = ConsensusContext::new(41, validators(), Some(0), None);
        assert_eq!(
            evaluate_recovery_context(&context, 42),
            RecoveryLoadOutcome::HeightMismatch {
                recovered: 41,
                expected: 42,
            }
        );
        let context = ConsensusContext::new(42, validators(), Some(0), None);
        assert_eq!(
            evaluate_recovery_context(&context, 42),
            RecoveryLoadOutcome::Resume
        );
    }

    #[test]
    fn resume_action_follows_recovered_proposal() {
        let mut context = ConsensusContext::new(42, validators(), Some(0), None);
        assert_eq!(
            recovery_resume_action(&context),
            RecoveryResumeAction::AnnounceEmptyProposal
        );
        context.proposed_tx_hashes = vec![neo_primitives::UInt256::from([7u8; 32])];
        assert_eq!(
            recovery_resume_action(&context),
            RecoveryResumeAction::RebuildPrepareResponse
        );
    }

    #[test]
    fn recovery_request_is_skipped_once_the_local_commit_exists() {
        let mut context = ConsensusContext::new(42, validators(), Some(0), None);
        context.proposed_block_hash = Some(neo_primitives::UInt256::from([0x42; 32]));
        assert!(should_request_recovery_on_start(&context));

        context
            .add_commit(0, 0, vec![0xA5; 64])
            .expect("local commit");
        assert!(!should_request_recovery_on_start(&context));
    }
}
