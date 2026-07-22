//! Transaction origin and propagation policy for mempool admission.

/// Where an unconfirmed transaction entered the node.
///
/// Origin is pool policy, not a consensus field. It is nevertheless explicit so
/// P2P, RPC, and node-generated transactions cannot silently acquire different
/// admission or propagation behavior at individual call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionOrigin {
    /// Transaction received from a remote peer.
    External,
    /// Transaction produced or submitted locally and eligible for propagation.
    Local,
    /// Locally submitted transaction that must not be propagated.
    Private,
}

impl TransactionOrigin {
    /// Returns whether an accepted transaction may be announced to peers.
    #[must_use]
    pub const fn should_propagate(self) -> bool {
        !matches!(self, Self::Private)
    }
}
