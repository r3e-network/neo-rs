//! Node-facing runtime capability traits.
//!
//! This module owns the narrow storage and transaction-admission contracts
//! consumed by upper-layer services. Immutable chain configuration is provided
//! by `neo_config::ChainSpecProvider`, its canonical owner.

use std::fmt::Debug;
use std::sync::Arc;

use neo_payloads::Transaction;

/// Provider trait for storage/cache access.
///
/// RPC session execution and other upper-layer services use this capability
/// without depending on the concrete node composition root.
pub trait StoreProvider: Send + Sync + Debug + 'static {
    /// Concrete storage backend exposed by this provider.
    type Store: neo_storage::persistence::store::Store + 'static;

    /// Returns the storage backend.
    fn store(&self) -> Arc<Self::Store>;

    /// Returns a snapshot of the current state.
    fn store_cache(&self) -> neo_storage::persistence::store_cache::StoreCache<Self::Store>;
}

/// Provider trait for transaction admission (mempool + relay).
///
/// The oracle service and other upper-layer services use this capability to
/// submit transactions without depending on the concrete node composition
/// root.
pub trait TxAdmission: Send + Sync + Debug + 'static {
    /// Canonical origin type owned by the concrete transaction pool.
    type Origin: Copy + Debug + Send + Sync + 'static;

    /// Typed admission outcome owned by the concrete transaction pool.
    type Outcome: Debug + Send + 'static;

    /// Submit `tx` through the node's single transaction-admission boundary.
    ///
    /// Implementations freeze their own point-in-time view of canonical state.
    /// Callers cannot supply a stale cache or a cache backed by another node.
    fn submit_transaction(&self, origin: Self::Origin, tx: Transaction) -> Self::Outcome;
}
