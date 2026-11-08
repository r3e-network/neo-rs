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

use neo_primitives::UInt256;
use neo_vm::VmState;
use std::fmt;

/// Result alias for the ledger persistence operation.
pub type LedgerResult<T> = Result<T, LedgerError>;

/// Errors raised by the canonical block-persistence pipeline.
#[derive(Debug)]
pub enum LedgerError {
    /// A transaction engine settled on a VM state other than `HALT` or `FAULT`
    /// (step 10c). This aborts persistence before any commit side effect.
    UnexpectedVmState {
        /// Hash of the offending transaction.
        transaction_hash: UInt256,
        /// The unexpected terminal VM state.
        vm_state: VmState,
    },
    /// An engine factory or a native/application engine port failed while opening
    /// or running an engine (steps 6, 10, 11).
    Engine {
        /// Engine-owned failure message.
        message: String,
    },
    /// A transaction hash could not be computed during pre-hashing or the
    /// per-transaction loop (steps 5, 10).
    TransactionHash {
        /// Hashing failure message.
        message: String,
    },
    /// A commit participant aborted the precommit gate (step 12).
    ///
    /// `Display` reproduces `message` verbatim so the participant's own error text
    /// (the injected participant message) survives the lift into `LedgerError`.
    ParticipantCommit {
        /// Participant-owned failure message, embedded verbatim in `Display`.
        message: String,
    },
    /// The canonical store commit failed (step 13).
    ///
    /// `Display` carries the store commit prefix followed by the underlying message.
    StoreCommit {
        /// Height of the block whose store commit failed.
        index: u32,
        /// Underlying store failure message.
        message: String,
    },
    /// The runtime cache / tip update failed after a successful store commit
    /// (step 14). Staged participants are discarded in reverse order.
    RuntimeCache {
        /// Height of the block whose runtime cache update failed.
        index: u32,
        /// Underlying runtime cache failure message.
        message: String,
    },
    /// Fast-path execution validation failed, triggering fallback to full VM.
    FastPathError {
        /// Validation failure message.
        message: String,
    },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::UnexpectedVmState {
                transaction_hash,
                vm_state,
            } => write!(
                f,
                "unexpected transaction VM state {vm_state:?} for hash {transaction_hash}"
            ),
            LedgerError::Engine { message } => f.write_str(message),
            LedgerError::TransactionHash { message } => f.write_str(message),
            // Contractual: the injected participant message is reproduced verbatim.
            LedgerError::ParticipantCommit { message } => f.write_str(message),
            // Contractual: the canonical store commit prefix is preserved.
            LedgerError::StoreCommit { index, message } => {
                write!(
                    f,
                    "failed to commit store cache for block {index}: {message}"
                )
            }
            LedgerError::RuntimeCache { index, message } => {
                write!(
                    f,
                    "failed to update runtime block cache for block {index}: {message}"
                )
            }
            LedgerError::FastPathError { message } => {
                write!(f, "fast-path validation failed: {message}")
            }
        }
    }
}

impl std::error::Error for LedgerError {}
