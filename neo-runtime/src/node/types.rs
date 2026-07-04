//! NodeTypes and NodeComponents trait hierarchy.
//!
//! This module defines the type hierarchy for node composition. Inspired by
//! reth's `NodeTypes` / `NodeComponents` pattern, it provides:
//!
//! - **`NodeTypes`** — associated types for protocol primitives
//!   (block, transaction, payload). Sealed (ADR-021) — only `NeoNodeTypes`
//!   can implement it. A node that swaps out Neo's `Block` type for a custom
//!   one implements a new `NodeTypes`.
//!
//! - **`NodeComponents<Node: FullNodeTypes>`** — associated types for
//!   every long-running service a full node must provide. Sealed (ADR-021).
//!   Currently scaffolded — no concrete impl exists yet (ADR-023).
//!
//! - **Provider traits** (`BlockchainProvider`, `StoreProvider`,
//!   `ConfigProvider`, `TxAdmission`) — the active decoupling layer that
//!   lets L6 crates depend on traits in L3 instead of `neo_system::Node` (L5).
//!
//! - **`FullNodeTypes`** — a convenience alias bounding `NodeTypes + 'static`.
//!
//! # Status (ADR-023)
//!
//! The type-state composition (`NodeComponents` associated types, `FullNode`
//! trait) is **scaffolded but not functional**. The current `NodeBuilder` in
//! `neo-system` uses runtime `Option<Arc<dyn Trait>>` composition. The
//! type-state traits are kept as a future-proofing seam — they will become
//! functional once concrete service implementations (`BlockExecutor`,
//! `ConsensusApi`, `EngineApi`) exist.

use async_trait::async_trait;
use crate::service::services::{
    BlockExecutor, ConsensusApi, EngineApi, NetworkService,
};
use crate::ServiceError;
use crate::service::import_queue::ImportQueue;
use neo_config::ProtocolSettings;
use neo_payloads::{Block, Transaction};
use neo_primitives::UInt256;
use std::fmt::Debug;
use std::sync::Arc;

/// Sealed marker — prevents external implementations of [`NodeTypes`] and
/// [`NodeComponents`]. Matches the reth convention where composition traits
/// are sealed to lock the extension surface.
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

/// Provider trait for read-only blockchain state access.
///
/// This is the trait that `neo-rpc` (and other read-side consumers)
/// should depend on instead of `neo_system::Node`. It exposes only
/// the methods needed to serve RPC queries, with no write path.
///
/// Replaces the current pattern where `neo-rpc` holds
/// `Arc<neo_system::Node>` and calls arbitrary methods on it.
///
/// Returns `Result<Option<T>, ServiceError>` so RPC servers can
/// distinguish "not found" (`.is_ok() && .as_ref().unwrap().is_none()`)
/// from "storage error" (`.is_err()`).
#[async_trait]
pub trait BlockchainProvider: Send + Sync + Debug + 'static {
    /// Fetch a block by hash.
    async fn get_block_by_hash(
        &self,
        hash: UInt256,
    ) -> Result<Option<Block>, ServiceError>;

    /// Fetch a block by height.
    async fn get_block_by_height(
        &self,
        height: u32,
    ) -> Result<Option<Block>, ServiceError>;

    /// Fetch the current block height.
    async fn get_block_count(&self) -> Result<u32, ServiceError>;

    /// Fetch a transaction by hash.
    async fn get_transaction_by_hash(
        &self,
        hash: UInt256,
    ) -> Result<Option<Transaction>, ServiceError>;

    /// Get the state root at a given block height.
    async fn get_state_root(
        &self,
        height: u32,
    ) -> Result<Option<UInt256>, ServiceError>;
}

/// Provider trait for storage/cache access.
///
/// Needed by RPC session (for VM execution) and indexer.
/// Separated from `BlockchainProvider` because not all consumers
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
/// Separated from `BlockchainProvider` and `StoreProvider` because
/// not all consumers need configuration access.
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

/// Components every full Neo node must provide.
///
/// Generic over `N: NodeTypes` so the same component definitions work
/// with custom primitive types.
///
/// Replaces the `Option<Arc<dyn Trait>>` fields in `neo_system::Node`
/// with compile-time-checked associated types.
///
/// This trait is sealed — only types within the `neo-runtime` crate
/// can implement it. External consumers should use the concrete
/// `NeoNodeComponents` (when available) or the provider traits
/// ([`BlockchainProvider`], [`StoreProvider`], [`ConfigProvider`]).
pub trait NodeComponents<N: FullNodeTypes>: sealed::Sealed + Send + Sync + 'static {
    /// Block executor.
    type Executor: BlockExecutor;

    /// Network service.
    type Network: NetworkService;

    /// Consensus service.
    type Consensus: ConsensusApi;

    /// Engine API surface.
    type Engine: EngineApi;

    /// Block import queue.
    type ImportQueue: ImportQueue;

    /// Read-only blockchain state provider (for RPC, indexer, etc.).
    /// Implementors should return `Arc<Self::Provider>` since the
    /// provider is typically shared. The trait itself is object-safe.
    type Provider: BlockchainProvider;

    /// Storage/cache provider (for VM execution, session state).
    type Store: StoreProvider;
}

/// Convenience alias: `NodeTypes` + the bounds that every "full" node
/// must satisfy.  Currently only seals `NodeTypes`; future associated
/// types (e.g. `StateProvider`) can be added here without touching
/// every `NodeTypes` implementor.
pub trait FullNodeTypes: NodeTypes + 'static {}

impl<T> FullNodeTypes for T where T: NodeTypes + 'static {}

/// A fully-wired node.
///
/// Unlike the earlier design that put accessor methods on the trait,
/// this trait only carries the *type-level* information.  The concrete
/// `Node` struct in `neo-system` provides the runtime accessors, which
/// simply return `Arc<dyn Service>` handles stored in the struct.
///
/// This avoids the associated-type projection problems that make
/// `fn provider(&self) -> &Self::Components::Provider` unrepresentable
/// in stable Rust.
pub trait FullNode: Send + Sync + 'static {
    /// Primitive types in use.
    type Types: FullNodeTypes;

    /// Assembled components.
    type Components: NodeComponents<Self::Types>;
}

// =============================================================================
// Extension trait (reserved for future use)
//
// NOTE: The `FullNodeComponentsExt` trait that provides `executor()`,
// `network()`, etc. accessor methods is reserved here but not yet
// exported or implemented.  It will be enabled once a concrete
// `TypedNode` struct uses the `NodeComponents` associated types.
// =============================================================================

#[allow(dead_code)]
trait FullNodeComponentsExt: FullNode {
    fn executor(&self) -> Arc<<Self::Components as NodeComponents<Self::Types>>::Executor>;
    fn network(&self) -> Arc<<Self::Components as NodeComponents<Self::Types>>::Network>;
    fn consensus(&self) -> Arc<<Self::Components as NodeComponents<Self::Types>>::Consensus>;
    fn engine(&self) -> Arc<<Self::Components as NodeComponents<Self::Types>>::Engine>;
    fn provider(&self) -> Arc<<Self::Components as NodeComponents<Self::Types>>::Provider>;
}
