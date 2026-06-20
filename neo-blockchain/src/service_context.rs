//! System context trait used by [`crate::service::BlockchainService`].
//!
//! The trait is the seam between the blockchain service and the
//! rest of the node: it provides the service with access to the
//! protocol settings, the current chain height, and the storage /
//! mempool / network backends it needs to validate and persist
//! blocks.
//!
//! Concrete implementations live in node and test contexts. The production
//! daemon context in `neo-node` exposes the canonical store snapshot and commit
//! hook, while tests can provide smaller in-memory contexts.
//!
//! The trait surface is intentionally narrow: it covers the operations the
//! service actually uses and keeps node/application wiring outside this crate.

use std::sync::Arc;

use async_trait::async_trait;

use neo_config::ProtocolSettings;
use neo_payloads::{ApplicationExecuted, Block};

/// Trait object giving the [`crate::service::BlockchainService`]
/// access to the system it is orchestrating.
///
/// Implementations are expected to be cheap to clone (`Arc<dyn …>`
/// everywhere) and `Send + Sync`. The blockchain service is the
/// *only* owner of the canonical tip, so concurrent callers go
/// through the service's command channel rather than through this
/// trait directly.
pub trait SystemContext: Send + Sync + std::fmt::Debug {
    /// Returns the effective protocol settings.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the current canonical chain height.
    fn current_height(&self) -> u32;

    /// Returns a data-cache snapshot over the canonical store for block
    /// persistence, or `None` when the implementation exposes no store
    /// (e.g. lightweight test contexts). When this returns a snapshot,
    /// [`crate::service::BlockchainService`] runs the native-contract
    /// persistence pipeline ([`crate::native_persist::persist_block_natives`])
    /// against it for every persisted block; committing the snapshot to the
    /// backing store remains the implementation's responsibility.
    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        None
    }

    /// Called after a block's native persistence pipeline has produced its
    /// `ApplicationExecuted` records but before the canonical store is
    /// committed. This mirrors the C# `ICommittingHandler` plugin hook and lets
    /// stateful plugins consume the same snapshot that will be committed to the
    /// canonical store. Returning `false` aborts block persistence; handlers
    /// should reserve that for consensus-critical failures (for example
    /// StateService local MPT updates).
    fn block_committing(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) -> bool {
        true
    }

    /// Flushes the writes accumulated in [`SystemContext::store_snapshot`] (after
    /// a block's native-persist pipeline) through to the durable backing store.
    /// The blockchain service calls this once per successfully persisted block
    /// (mirroring C# `snapshot.Commit()` at the end of `Blockchain.Persist`).
    /// The default is a no-op for store-less / in-memory test contexts.
    fn commit_to_store(&self) {}

    /// Called after the block's writes have been committed to the canonical
    /// store. This mirrors the C# `ICommittedHandler` plugin hook.
    fn block_committed(&self, _block: &Block) {}
}

/// Async variant of [`SystemContext`]; the trait object behind the
/// blockchain service is allowed to be either the sync or the async
/// flavour. The async variant is needed by implementations that
/// touch the storage backend asynchronously (e.g. RocksDB).
#[async_trait]
pub trait AsyncSystemContext: Send + Sync + std::fmt::Debug {
    /// Returns the effective protocol settings.
    async fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the current canonical chain height.
    async fn current_height(&self) -> u32;
}
