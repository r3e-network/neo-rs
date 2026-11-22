//! Canonical error surface for the ledger block-persistence operation.
//!
//! `LedgerError` is the only error type produced by [`super::persist_block`]. It is
//! deliberately schema-free: the ledger never constructs the system/rpc diagnostic
//! observation schema, and it never depends on `neo-core`/`neo-system` error types.
//!
//! Two `Display` literals are contractual hard constraints (task-102 Part A):
//! * [`LedgerError::ParticipantCommit`] embeds the participant's own message verbatim
//!   so an injected commit failure (e.g. `"injected committing failure"`) is preserved
//!   through the error surface.
//! * [`LedgerError::StoreCommit`] carries the canonical store commit prefix
//!   (`"failed to commit store cache for block {index}: {message}"`).
//!
//! The `#[error(...)]` attributes below are the single source of truth for those
//! literals; changing them changes the contractual `Display` output.

use neo_primitives::UInt256;
use neo_vm::VmState;
use thiserror::Error;

/// Result alias for the ledger persistence operation.
pub type LedgerResult<T> = Result<T, LedgerError>;

/// Errors raised by the canonical block-persistence pipeline.
#[derive(Error, Debug)]
pub enum LedgerError {
    /// A transaction engine settled on a VM state other than `HALT` or `FAULT`
    /// (step 10c). This aborts persistence before any commit side effect.
    #[error("unexpected transaction VM state {vm_state:?} for hash {transaction_hash}")]
    UnexpectedVmState {
        /// Hash of the offending transaction.
        transaction_hash: UInt256,
        /// The unexpected terminal VM state.
        vm_state: VmState,
    },
    /// An engine factory or a native/application engine port failed while opening
    /// or running an engine (steps 6, 10, 11).
    #[error("{message}")]
    Engine {
        /// Engine-owned failure message.
        message: String,
    },
    /// A transaction hash could not be computed during pre-hashing or the
    /// per-transaction loop (steps 5, 10).
    #[error("{message}")]
    TransactionHash {
        /// Hashing failure message.
        message: String,
    },
    /// A commit participant aborted the precommit gate (step 12).
    ///
    /// `Display` reproduces `message` verbatim so the participant's own error text
    /// (the injected participant message) survives the lift into `LedgerError`.
    #[error("{message}")]
    ParticipantCommit {
        /// Participant-owned failure message, embedded verbatim in `Display`.
        message: String,
    },
    /// The canonical store commit failed (step 13).
    ///
    /// `Display` carries the store commit prefix followed by the underlying message.
    #[error("failed to commit store cache for block {index}: {message}")]
    StoreCommit {
        /// Height of the block whose store commit failed.
        index: u32,
        /// Underlying store failure message.
        message: String,
    },
    /// The runtime cache / tip update failed after a successful store commit
    /// (step 14). Staged participants are discarded in reverse order.
    #[error("failed to update runtime block cache for block {index}: {message}")]
    RuntimeCache {
        /// Height of the block whose runtime cache update failed.
        index: u32,
        /// Underlying runtime cache failure message.
        message: String,
    },
    /// The store commit **succeeded** and only the runtime cache / tip update
    /// failed (step 14).
    ///
    /// This is deliberately distinct from [`LedgerError::RuntimeCache`] and from
    /// [`LedgerError::StoreCommit`]: the block state is durable, so no rollback is
    /// possible or attempted, and a caller must not treat the block as uncommitted
    /// (retrying it would replay an already-committed height). The local runtime
    /// view is stale until the process reloads or the cache is repaired.
    #[error("block {index} was durably committed but the runtime cache update failed: {message}")]
    PostCommitCacheFailure {
        /// Height of the durably committed block whose cache update failed.
        index: u32,
        /// Underlying runtime cache failure message.
        message: String,
    },
    /// Fast-path execution validation failed, triggering fallback to full VM.
    #[error("fast-path validation failed: {message}")]
    FastPathError {
        /// Validation failure message.
        message: String,
    },
    /// A transaction faulted by raising `Policy.activateHardfork` with an
    /// unknown hardfork name (C# v3.11.2 `UnknownHardforkException`).
    ///
    /// C# rethrows the exception out of `Blockchain.Persist`, halting the node;
    /// this variant aborts persistence with the same fatal effect before any
    /// commit side effect. The fault is typed on the engine side, so contract
    /// `THROW` text can never trigger it.
    #[error(
        "transaction {transaction_hash} faulted with unknown hardfork {hardfork_name}; update node software to the latest version to continue following the chain"
    )]
    UnknownHardforkFault {
        /// Hash of the faulted transaction.
        transaction_hash: UInt256,
        /// The unrecognized hardfork name passed to `activateHardfork`.
        hardfork_name: String,
    },
}

impl LedgerError {
    /// Returns whether this error is the C# v3.11.2 unknown-hardfork halt.
    #[must_use]
    pub const fn is_unknown_hardfork_fault(&self) -> bool {
        matches!(self, Self::UnknownHardforkFault { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_hardfork_fault_display_and_predicate() {
        let error = LedgerError::UnknownHardforkFault {
            transaction_hash: UInt256::from_bytes(&[7u8; 32]).expect("hash"),
            hardfork_name: "HF_Basilisk".to_string(),
        };
        assert!(error.is_unknown_hardfork_fault());
        assert!(error
            .to_string()
            .contains("faulted with unknown hardfork HF_Basilisk; update node software to the latest version to continue following the chain"));
    }
}
