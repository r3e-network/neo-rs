//! Pure consensus policy decisions.
//!
//! Everything in here is deterministic and infrastructure-free: proposal
//! selection, backup-side validation, commit-egress authorization,
//! `PrepareRequest` screening and recovery handling. The node adapter supplies
//! the outside world through narrow ports:
//!
//! * [`ProposalCandidateSource`] — ordered candidate hashes (mempool order,
//!   privacy-mode TEE ordering, isolation view),
//! * [`TransactionSummarySource`] — the transaction facts behind those hashes
//!   (hash, encoded size, system fee),
//! * [`CommitPersistencePort`] — durable storage for the local Commit recovery
//!   record.
//!
//! Concrete wallet, actor, network and ledger code stays in the node.

mod egress;
mod proposal_acceptance;
mod recovery;
mod selection;
mod validation;

pub use egress::{CommitEgressGuard, CommitPersistencePort};
pub use proposal_acceptance::{
    PrepareRequestPrecheck, PrepareRequestRejection, precheck_prepare_request,
};
pub use recovery::{
    RecoveryLoadOutcome, RecoveryResumeAction, evaluate_recovery_context, recovery_resume_action,
    should_request_recovery_on_start,
};
pub use selection::{
    ProposalCandidateSource, ProposalLimits, ProposalSelection, ProposalStop, TransactionSummary,
    TransactionSummarySource, block_size_with_transactions, select_proposal_transactions,
    select_proposal_transactions_from_source,
};
pub use validation::{
    BlockLimitVerdict, ProposalPolicy, ProposalTransactionCheck, ProposalTransactionVerdict,
    RejectionCause, VerificationOutcome, evaluate_block_limits, evaluate_proposal_transaction,
};
