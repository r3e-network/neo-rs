//! Result record of state-independent transaction pre-verification.

use neo_payloads::Transaction;
use neo_primitives::verify_result::VerifyResult;
use serde::{Deserialize, Serialize};

/// Pre-verification completion record.
///
/// Produced by the transaction router (or any equivalent component that
/// runs `verify_state_independent` ahead of the blockchain) and sent
/// to the blockchain service as a [`crate::BlockchainCommand::PreverifyCompleted`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreverifyCompleted {
    /// The transaction that was pre-verified.
    pub transaction: Transaction,
    /// Whether the transaction should be relayed to peers.
    pub relay: bool,
    /// The result of state-independent verification.
    pub result: VerifyResult,
}
