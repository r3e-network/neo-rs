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
//! The trait surface is intentionally narrow — it covers the
//! operations the service *actually* uses. New methods can be added
//! in later stages as the actor's legacy handler logic is
//! progressively ported over.

use std::sync::Arc;

use async_trait::async_trait;

use neo_config::ProtocolSettings;
use neo_state_service::StateStore;

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

    /// Returns the state-service store, when the node has enabled the
    /// StateService plugin. The blockchain persistence loop uses this to feed
    /// the MPT state-root store from the same tracked storage changes it just
    /// applied to the canonical snapshot.
    fn state_store(&self) -> Option<Arc<StateStore>> {
        None
    }

    /// Flushes the writes accumulated in [`SystemContext::store_snapshot`] (after
    /// a block's native-persist pipeline) through to the durable backing store.
    /// The blockchain service calls this once per successfully persisted block
    /// (mirroring C# `snapshot.Commit()` at the end of `Blockchain.Persist`).
    /// The default is a no-op for store-less / in-memory test contexts.
    fn commit_to_store(&self) {}
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
