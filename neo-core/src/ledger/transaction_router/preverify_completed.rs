//! Result record of state-independent transaction pre-verification.

use crate::ledger::VerifyResult;
use crate::network::p2p::payloads::Transaction;
use serde::{Deserialize, Serialize};

/// public record PreverifyCompleted(Transaction Transaction, bool Relay, VerifyResult Result);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreverifyCompleted {
    pub transaction: Transaction,
    pub relay: bool,
    pub result: VerifyResult,
}
