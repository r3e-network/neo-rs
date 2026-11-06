//! Transaction ingress (pre-verification submission) port.
//!
//! Replaces the session's call into the system transaction router
//! (`enqueue_preverify_from`). The port names only the canonical protocol
//! transaction and the opaque peer handle, so the P2P layer never reaches the
//! router, mempool internals, or the actor runtime directly.

use super::{PeerHandle, PortResult};
use neo_protocol::payloads::Transaction;

/// Submits a peer-delivered transaction for pre-verification.
///
/// # Fail-closed contract
///
/// A non-`Ok` result means the transaction was **not** accepted for
/// pre-verification. The caller must then drop its local dedup record and ask
/// the task manager to forget the hash, so a later announcement can retry the
/// same transaction instead of it being permanently suppressed by a failed
/// hand-off. An adapter must therefore return `Err` whenever the submission
/// cannot be guaranteed to reach the pre-verification worker.
pub trait TransactionIngressPort: Send + Sync {
    /// Enqueues `transaction` for pre-verification.
    ///
    /// * `relay` — request network relay once the transaction is accepted.
    /// * `source` — the peer that delivered the transaction, used for
    ///   reply/credit routing and for forgetting the hash on failure.
    fn enqueue_preverify(
        &self,
        transaction: Transaction,
        relay: bool,
        source: PeerHandle,
    ) -> PortResult<()>;
}
