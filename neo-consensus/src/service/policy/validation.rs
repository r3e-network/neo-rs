//! Backup-side proposal validation policy.
//!
//! A backup validator must decide, per proposed transaction and for the block
//! as a whole, whether it can send a `PrepareResponse` or has to request a
//! view change. Those decisions depend only on the transaction summary, the
//! ledger/proposal conflict flags resolved by the node and the configured
//! limits — exactly the facts captured by [`ProposalTransactionCheck`].

use super::selection::{
    ProposalCandidateSource, ProposalLimits, ProposalSelection, block_size_with_transactions,
    select_proposal_transactions_from_source,
};
use crate::change_view_reason::ChangeViewReason;
use neo_primitives::UInt256;
use std::collections::HashSet;

/// Result of the deterministic verification the node performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// Verification succeeded (also used for transactions already verified by
    /// the mempool).
    Succeeded,
    /// Verification failed the node's policy rules.
    RejectedByPolicy,
    /// Verification failed as invalid.
    Invalid,
}

/// Facts about one proposed transaction, resolved by the node adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalTransactionCheck {
    /// The transaction is already present in the ledger.
    pub already_on_chain: bool,
    /// The transaction conflicts with a ledger transaction of the same sender.
    pub conflicts_with_ledger: bool,
    /// The transaction conflicts with a transaction accepted earlier in this
    /// proposal.
    pub conflicts_with_proposal: bool,
    /// System fee charged by the transaction.
    pub system_fee: i64,
    /// Deterministic verification result.
    pub verification: VerificationOutcome,
}

impl ProposalTransactionCheck {
    /// Creates a check for an already-verified mempool transaction.
    #[must_use]
    pub fn verified(system_fee: i64) -> Self {
        Self {
            already_on_chain: false,
            conflicts_with_ledger: false,
            conflicts_with_proposal: false,
            system_fee,
            verification: VerificationOutcome::Succeeded,
        }
    }
}

/// Concrete reason a proposed transaction was rejected, used for node logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionCause {
    /// The transaction is already on chain.
    AlreadyOnChain,
    /// The transaction conflicts with the ledger.
    LedgerConflict,
    /// The transaction's system fee exceeds the per-block cap.
    SystemFeeExceeded,
    /// The transaction conflicts with an earlier transaction in the proposal.
    ProposalConflict,
    /// Deterministic verification rejected the transaction by policy.
    VerificationPolicy,
    /// Deterministic verification found the transaction invalid.
    VerificationInvalid,
}

/// Verdict for one proposed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalTransactionVerdict {
    /// The transaction can be accepted into the proposal.
    Accept,
    /// The transaction must be rejected and a view change requested.
    Reject {
        /// View-change reason to broadcast.
        reason: ChangeViewReason,
        /// Node-facing cause of the rejection.
        cause: RejectionCause,
    },
}

impl ProposalTransactionVerdict {
    /// Returns `true` when the transaction can be accepted.
    #[must_use]
    pub fn is_accept(&self) -> bool {
        matches!(self, Self::Accept)
    }
}

fn reject(reason: ChangeViewReason, cause: RejectionCause) -> ProposalTransactionVerdict {
    ProposalTransactionVerdict::Reject { reason, cause }
}

/// Decides whether a proposed transaction survives backup validation.
///
/// Checks run in the order mandated by the reference implementation: ledger
/// presence, ledger conflict, fee cap, in-proposal conflict and finally the
/// deterministic verification result. The first failing check wins.
#[must_use]
pub fn evaluate_proposal_transaction(
    check: &ProposalTransactionCheck,
    limits: &ProposalLimits,
) -> ProposalTransactionVerdict {
    if check.already_on_chain {
        return reject(ChangeViewReason::TxInvalid, RejectionCause::AlreadyOnChain);
    }
    if check.conflicts_with_ledger {
        return reject(ChangeViewReason::TxInvalid, RejectionCause::LedgerConflict);
    }
    if check.system_fee > limits.max_block_system_fee {
        return reject(
            ChangeViewReason::TxInvalid,
            RejectionCause::SystemFeeExceeded,
        );
    }
    if check.conflicts_with_proposal {
        return reject(
            ChangeViewReason::TxInvalid,
            RejectionCause::ProposalConflict,
        );
    }
    match check.verification {
        VerificationOutcome::Succeeded => ProposalTransactionVerdict::Accept,
        VerificationOutcome::RejectedByPolicy => reject(
            ChangeViewReason::TxRejectedByPolicy,
            RejectionCause::VerificationPolicy,
        ),
        VerificationOutcome::Invalid => reject(
            ChangeViewReason::TxInvalid,
            RejectionCause::VerificationInvalid,
        ),
    }
}

/// Verdict for the assembled proposal block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLimitVerdict {
    /// The block fits every limit.
    Accept,
    /// The block must be rejected with a view change.
    Reject {
        /// Always `BlockRejectedByPolicy`.
        reason: ChangeViewReason,
    },
}

impl BlockLimitVerdict {
    /// Returns `true` when the block fits the limits.
    #[must_use]
    pub fn is_accept(&self) -> bool {
        matches!(self, Self::Accept)
    }
}

/// Applies the aggregate fee cap and the block-size cap to a full proposal.
///
/// `block_overhead_size` is the node's header/witness estimate and
/// `transaction_bytes` the summed encoded size of the accepted transactions.
#[must_use]
pub fn evaluate_block_limits(
    block_overhead_size: usize,
    transaction_count: usize,
    transaction_bytes: usize,
    total_system_fee: i64,
    limits: &ProposalLimits,
) -> BlockLimitVerdict {
    if total_system_fee > limits.max_block_system_fee {
        return BlockLimitVerdict::Reject {
            reason: ChangeViewReason::BlockRejectedByPolicy,
        };
    }
    let block_size =
        block_size_with_transactions(block_overhead_size, transaction_count, transaction_bytes);
    if block_size > limits.max_block_size {
        return BlockLimitVerdict::Reject {
            reason: ChangeViewReason::BlockRejectedByPolicy,
        };
    }
    BlockLimitVerdict::Accept
}

/// Proposal policy service.
///
/// Binds the configured block limits and the current round's header overhead to
/// the pure selection and validation decisions, so the node adapter only has to
/// supply the candidate stream and resolved transaction facts.
#[derive(Debug, Clone, Copy)]
pub struct ProposalPolicy {
    limits: ProposalLimits,
    block_overhead_size: usize,
}

impl ProposalPolicy {
    /// Creates a policy for one round.
    ///
    /// `block_overhead_size` is the node's header/witness size estimate for the
    /// current validator set.
    #[must_use]
    pub fn new(limits: ProposalLimits, block_overhead_size: usize) -> Self {
        Self {
            limits,
            block_overhead_size,
        }
    }

    /// Configured limits.
    #[must_use]
    pub fn limits(&self) -> ProposalLimits {
        self.limits
    }

    /// Header/witness overhead the block-size accounting starts from.
    #[must_use]
    pub fn block_overhead_size(&self) -> usize {
        self.block_overhead_size
    }

    /// Pulls candidates from `source` and selects the proposal subset.
    ///
    /// The source is asked for at most `min(limit, max_transactions)` ordered
    /// hashes; their summaries are resolved through the same source's summary
    /// port, so the policy never proposes more than the configured cap.
    pub fn select_from(
        &self,
        source: &mut dyn ProposalCandidateSource,
        limit: usize,
        rejected_above_f: &HashSet<UInt256>,
    ) -> ProposalSelection {
        select_proposal_transactions_from_source(
            source,
            &self.limits,
            self.block_overhead_size,
            limit,
            rejected_above_f,
        )
    }

    /// Decides whether a proposed transaction survives backup validation.
    #[must_use]
    pub fn evaluate_transaction(
        &self,
        check: &ProposalTransactionCheck,
    ) -> ProposalTransactionVerdict {
        evaluate_proposal_transaction(check, &self.limits)
    }

    /// Applies the aggregate fee cap and block-size cap to the whole proposal.
    #[must_use]
    pub fn evaluate_block(
        &self,
        transaction_count: usize,
        transaction_bytes: usize,
        total_system_fee: i64,
    ) -> BlockLimitVerdict {
        evaluate_block_limits(
            self.block_overhead_size,
            transaction_count,
            transaction_bytes,
            total_system_fee,
            &self.limits,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::selection::TransactionSummary;
    use super::super::selection::TransactionSummarySource;
    use super::*;

    fn limits() -> ProposalLimits {
        ProposalLimits::new(512, 1_024, 1_000)
    }

    #[test]
    fn accepts_clean_verified_transaction() {
        let check = ProposalTransactionCheck::verified(10);
        assert!(evaluate_proposal_transaction(&check, &limits()).is_accept());
    }

    #[test]
    fn on_chain_transaction_is_invalid() {
        let mut check = ProposalTransactionCheck::verified(10);
        check.already_on_chain = true;
        assert_eq!(
            evaluate_proposal_transaction(&check, &limits()),
            ProposalTransactionVerdict::Reject {
                reason: ChangeViewReason::TxInvalid,
                cause: RejectionCause::AlreadyOnChain,
            }
        );
    }

    #[test]
    fn fee_cap_wins_over_verification_outcome() {
        let mut check = ProposalTransactionCheck::verified(1_001);
        check.verification = VerificationOutcome::RejectedByPolicy;
        assert_eq!(
            evaluate_proposal_transaction(&check, &limits()),
            ProposalTransactionVerdict::Reject {
                reason: ChangeViewReason::TxInvalid,
                cause: RejectionCause::SystemFeeExceeded,
            }
        );
    }

    #[test]
    fn policy_failure_maps_to_policy_reason() {
        let mut check = ProposalTransactionCheck::verified(10);
        check.verification = VerificationOutcome::RejectedByPolicy;
        assert_eq!(
            evaluate_proposal_transaction(&check, &limits()),
            ProposalTransactionVerdict::Reject {
                reason: ChangeViewReason::TxRejectedByPolicy,
                cause: RejectionCause::VerificationPolicy,
            }
        );
    }

    #[test]
    fn block_limits_reject_oversized_and_overpriced_blocks() {
        assert!(evaluate_block_limits(0, 2, 100, 500, &limits()).is_accept());
        assert_eq!(
            evaluate_block_limits(0, 2, 100, 1_001, &limits()),
            BlockLimitVerdict::Reject {
                reason: ChangeViewReason::BlockRejectedByPolicy,
            }
        );
        assert_eq!(
            evaluate_block_limits(1_000, 2, 100, 0, &limits()),
            BlockLimitVerdict::Reject {
                reason: ChangeViewReason::BlockRejectedByPolicy,
            }
        );
    }

    struct FixedSource(Vec<TransactionSummary>);

    impl TransactionSummarySource for FixedSource {
        fn transaction_summary(&self, hash: &UInt256) -> Option<TransactionSummary> {
            self.0.iter().find(|summary| summary.hash == *hash).cloned()
        }
    }

    impl ProposalCandidateSource for FixedSource {
        fn proposal_candidate_hashes(&mut self, limit: usize) -> Vec<UInt256> {
            self.0
                .iter()
                .take(limit)
                .map(|summary| summary.hash)
                .collect()
        }
    }

    #[test]
    fn policy_service_selects_from_port_and_evaluates_block() {
        let policy = ProposalPolicy::new(ProposalLimits::new(2, 1_024, 100), 40);
        let mut source = FixedSource(vec![
            TransactionSummary::new(UInt256::from([1u8; 32]), 10, 40),
            TransactionSummary::new(UInt256::from([2u8; 32]), 10, 40),
            TransactionSummary::new(UInt256::from([3u8; 32]), 10, 40),
        ]);

        let selection = policy.select_from(&mut source, 10, &HashSet::new());

        assert_eq!(selection.len(), 2, "max_transactions caps the selection");
        assert_eq!(selection.total_system_fee, 80);
        assert!(policy.evaluate_block(2, 20, 80).is_accept());
        assert!(!policy.evaluate_block(2, 2_000, 80).is_accept());
    }
}
