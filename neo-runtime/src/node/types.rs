//! NodeTypes and the active provider-trait decoupling layer.
//!
//! This module defines:
//!
//! - **`NodeTypes`** — associated types for protocol primitives
//!   (block, transaction, payload). Sealed (ADR-021) — only `NeoNodeTypes`
//!   can implement it. A node that swaps out Neo's `Block` type for a custom
//!   one implements a new `NodeTypes`.
//!
//! - **Provider traits** (`StoreProvider`, `ConfigProvider`, `TxAdmission`) —
//!   the active decoupling layer that lets L6 crates (RPC, oracle, indexer)
//!   depend on traits in L3 instead of the concrete `neo_system::Node` (L5).
//!
//! # History (ADR-032)
//!
//! An earlier reth-inspired type-state seam (`NodeComponents`, `FullNode`,
//! `FullNodeTypes`, `FullNodeComponentsExt`, and a `BlockchainProvider`
//! trait) was scaffolded here but never became functional — it had zero
//! implementations and zero `dyn` consumers, and the `BlockchainProvider`
//! impl silently returned `Ok(None)`. ADR-032 deleted that dead scaffolding;
//! the current `NodeBuilder` in `neo-system` uses runtime
//! `Option<Arc<dyn Trait>>` composition over the live provider traits below.

use crate::ServiceError;
use neo_config::ProtocolSettings;
use neo_payloads::{Block, Transaction};
use neo_primitives::UInt256;
use std::fmt::Debug;
use std::sync::Arc;

/// Sealed marker — prevents external implementations of [`NodeTypes`].
/// Matches the reth convention where composition traits are sealed to lock
/// the extension surface.
mod sealed {
    /// Private supertrait that gates trait implementation.
    pub trait Sealed {}
}

/// Protocol primitive types used throughout the node.
///
/// Implement this trait (or use the provided `NeoNodeTypes` default)
/// to swap out primitive types without touching service code. In Neo
/// v3 the types are fixed; this exists for the same reason reth has
/// `NodeTypes`: to make the node *generically* correct and ready for
/// future protocol upgrades or side-chains.
///
/// This trait is sealed — only types within the `neo-runtime` crate
/// (currently just [`NeoNodeTypes`]) can implement it.
pub trait NodeTypes: sealed::Sealed + Send + Sync + Debug + 'static {
    /// Block type.
    type Block: Send + Sync + Clone + Debug + 'static;

    /// Transaction type.
    type Transaction: Send + Sync + Clone + Debug + 'static;

    /// Payload type (the result of executing a block).
    type Payload: Send + Sync + Clone + Debug + 'static;

    /// Block hash type.
    type BlockHash: Send + Sync + Clone + Debug + Eq + 'static;
}

/// Default [`NodeTypes`] for Neo N3 mainnet.
#[derive(Debug, Clone)]
pub struct NeoNodeTypes;

impl sealed::Sealed for NeoNodeTypes {}

impl NodeTypes for NeoNodeTypes {
    type Block = Block;
    type Transaction = Transaction;
    type Payload = crate::outcome::ExecutionPayload;
    type BlockHash = UInt256;
}

/// Provider trait for storage/cache access.
///
/// Needed by RPC session (for VM execution) and indexer.
/// Separated from `ConfigProvider` because not all consumers
/// need cache access.
pub trait StoreProvider: Send + Sync + Debug + 'static {
    /// Returns the storage backend.
    fn store(&self) -> Arc<dyn neo_storage::persistence::store::Store>;

    /// Returns a snapshot of the current state.
    fn store_cache(&self) -> neo_storage::persistence::store_cache::StoreCache;
}

/// Provider trait for node configuration access.
///
/// Needed by RPC session (for protocol settings and block increment).
/// Separated from `StoreProvider` because not all consumers need
/// configuration access.
pub trait ConfigProvider: Send + Sync + Debug + 'static {
    /// Returns the protocol settings.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the maximum valid-until block increment.
    fn max_valid_until_block_increment(&self) -> u32;
}

/// Provider trait for transaction admission (mempool + relay).
///
/// Needed by the oracle service (and future plugin crates) to submit
/// response transactions into the node's mempool and broadcast them.
/// Separated from the concrete `TxRouterHandle` in `neo-system` so that
/// Layer 6 crates can depend on the trait without pulling in the
/// Layer 5 composition root.
pub trait TxAdmission: Send + Sync + Debug + 'static {
    /// Admit `tx` into the mempool against `snapshot` and, when `relay`
    /// is set, best-effort broadcast it to peers.
    ///
    /// Returns `Ok(())` only when the mempool accepts the transaction;
    /// any other verdict is surfaced as an error so the caller can log
    /// and retain the work.
    fn try_enqueue_preverify(
        &self,
        tx: Transaction,
        relay: bool,
        snapshot: &neo_storage::persistence::DataCache,
    ) -> Result<(), ServiceError>;
}
