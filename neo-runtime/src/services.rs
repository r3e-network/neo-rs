//! Reth-style async service traits for the Neo node runtime.
//!
//! Every long-running component of a Neo node (block executor, network
//! stack, consensus, engine API, blockchain orchestrator) is modelled as
//! an `async_trait` *service*. A service is a `Send + Sync` value that
//! exposes its capabilities as plain `async fn`s on a trait object
//! (`Arc<dyn ServiceTrait>`). The concrete node that wires these services
//! together lives in `neo-system` (`neo_system::Node`).
//!
//! The choice of trait objects (vs. generics) is deliberate: it matches
//! the reth convention, makes each service cheap to clone behind an
//! `Arc`, and lets tests swap in a mock for any single service without
//! recompiling the rest of the graph. None of the traits require
//! `'static` beyond what `Arc<dyn Trait>` already implies, so individual
//! concrete services can hold any state they like behind the trait.
//!
//! The transaction pool is intentionally *not* modelled as a service
//! trait here: the concrete mempool (`neo-mempool`) is reached through
//! the transaction-router handle on `neo_system::Node`, not an
//! `Arc<dyn MempoolService>`.
//!
//! ## Pattern cheat-sheet
//!
//! | Reth trait              | Neo trait (this crate)        | Backing primitive      |
//! |-------------------------|-------------------------------|------------------------|
//! | `BlockExecutor`         | [`BlockExecutor`]             | `Arc<dyn BlockExecutor>` |
//! | `NetworkManager`        | [`NetworkService`]            | `Arc<dyn NetworkService>` |
//! | `Consensus`             | [`ConsensusService`]          | `Arc<dyn ConsensusService>` |
//! | `Engine`                | [`NeoEngine`]                 | `Arc<dyn NeoEngine>`     |

use crate::errors::ServiceError;
use crate::outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
use async_trait::async_trait;
use neo_payloads::{Block, Transaction};
use neo_primitives::UInt256;
use tokio::sync::broadcast;

/// Hash of a transaction. Currently a type alias for `UInt256`; the alias
/// exists so service signatures read naturally (`TxHash`) and so a future
/// change to a richer transaction-id type does not cascade through the
/// service traits.
pub type TxHash = UInt256;

/// Marker trait implemented by every Neo runtime service.
///
/// `Service` exists to give every component a single bound to satisfy and
/// a uniform way to print / log a description of itself when held behind a
/// trait object. There is no required method beyond the auto-trait bounds.
pub trait Service: Send + Sync + std::fmt::Debug + 'static {
    /// Short, human-readable name of the service implementation.
    ///
    /// Used in log lines, metrics labels, and `Debug` output. Should be
    /// stable per implementation (e.g. `"RocksDbExecutor"`,
    /// `"LocalNetworkService"`).
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

// =============================================================================
// BlockExecutor
// =============================================================================

/// Executes and validates blocks against the current state.
///
/// Mirrors reth's `BlockExecutor`: stateless with respect to the
/// blockchain, and *does* need read access to the current state. The
/// service is expected to be safe to call concurrently from many tasks
/// (e.g. one per RPC request, plus the consensus driver).
#[async_trait]
pub trait BlockExecutor: Service {
    /// Apply the block's transactions to the state and return the
    /// resulting [`ExecutionOutcome`].
    ///
    /// Implementations that perform the work synchronously should still
    /// return `async` (e.g. by `tokio::task::spawn_blocking` or by yielding
    /// with `tokio::task::yield_now`) so the runtime's worker pool is not
    /// monopolised.
    async fn execute(&self, block: &Block) -> Result<ExecutionOutcome, ServiceError>;

    /// Cheap, *consensus-level* validation of a block: header shape,
    /// merkle root, witness envelopes, etc. Does **not** execute
    /// transactions; for full state-transition validation use
    /// [`Self::execute`] and inspect [`ExecutionOutcome::ok`].
    async fn validate(&self, block: &Block) -> Result<(), ServiceError>;
}

// =============================================================================
// NetworkService
// =============================================================================

/// P2P networking surface.
///
/// The trait is the "outside" of the network stack: it broadcasts
/// blocks / transactions and reports peer counts. Receiving blocks /
/// transactions is delivered through the
/// [`broadcast::Receiver<NetworkEvent>`] returned by
/// [`NetworkService::subscribe_events`].
#[async_trait]
pub trait NetworkService: Service {
    /// Broadcast a freshly persisted block to all connected peers.
    async fn broadcast_block(&self, block: &Block) -> Result<(), ServiceError>;

    /// Broadcast a transaction to all connected peers (typically
    /// immediately after it is accepted into the mempool).
    async fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), ServiceError>;

    /// Current number of connected peers.
    async fn peer_count(&self) -> usize;

    /// Subscribe to [`NetworkEvent`]s. Each call returns an *independent*
    /// receiver; dropping the receiver automatically unregisters the
    /// subscription. Broadcasts are best-effort: if a subscriber falls
    /// behind it will observe a [`tokio::sync::broadcast::error::RecvError::Lagged`]
    /// rather than block the publisher.
    fn subscribe_events(&self) -> broadcast::Receiver<NetworkEvent>;
}

// =============================================================================
// ConsensusService
// =============================================================================

/// Drives the dBFT consensus loop.
///
/// In reth there is no consensus service in the same sense (consensus is
/// external); in neo-rs the dBFT plugin ships as a service so the same
/// `Arc<dyn ConsensusService>` is reachable from both the node binary
/// and the test harness.
#[async_trait]
pub trait ConsensusService: Service {
    /// Start the consensus loop. Idempotent: a second call while
    /// already running is a no-op.
    async fn start(&self) -> Result<(), ServiceError>;

    /// Stop the consensus loop. Idempotent: a second call while
    /// already stopped is a no-op.
    async fn stop(&self) -> Result<(), ServiceError>;

    /// `true` if the consensus loop is currently running.
    async fn is_running(&self) -> bool;
}

// =============================================================================
// NeoEngine
// =============================================================================

/// Engine-API surface.
///
/// Equivalent to reth's `Engine` trait: the typed entry point used by
/// the consensus driver to ask the execution layer to apply a block and
/// return the resulting payload. The trait is distinct from
/// [`BlockExecutor`] so the two surfaces can evolve independently: the
/// engine API is shaped around the consensus protocol's request/response
/// model, the block executor is shaped around synchronous RPC use.
#[async_trait]
pub trait NeoEngine: Service {
    /// Apply a block and return the resulting [`ExecutionPayload`].
    async fn execute_block(&self, block: &Block) -> Result<ExecutionPayload, ServiceError>;

    /// Cheap validation of a block (header shape, merkle root, witness
    /// envelopes). Distinct from [`BlockExecutor::validate`] so the
    /// engine layer can add protocol-specific checks (e.g. consensus
    /// payload format) without those checks leaking into the executor.
    async fn validate_block(&self, block: &Block) -> Result<ValidationResult, ServiceError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// No-op service used to verify the trait is object-safe and can be
    /// held behind an `Arc<dyn ...>`.
    #[derive(Debug)]
    struct DummyExecutor;

    impl Service for DummyExecutor {}

    #[async_trait]
    impl BlockExecutor for DummyExecutor {
        async fn execute(&self, _block: &Block) -> Result<ExecutionOutcome, ServiceError> {
            Ok(ExecutionOutcome::default())
        }

        async fn validate(&self, _block: &Block) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    #[test]
    fn traits_are_object_safe() {
        fn _executor(_: &dyn BlockExecutor) {}
        fn _network(_: &dyn NetworkService) {}
        fn _consensus(_: &dyn ConsensusService) {}
        fn _engine(_: &dyn NeoEngine) {}
    }

    #[tokio::test]
    async fn dummy_executor_runs() {
        let exec: Arc<dyn BlockExecutor> = Arc::new(DummyExecutor);
        let block = Block::new();
        exec.execute(&block).await.expect("execute");
        exec.validate(&block).await.expect("validate");
    }
}
