//! Async service traits for the Neo node runtime.
//!
//! Long-running runtime components are modelled as `async_trait` *services*: a
//! `Send + Sync` value that exposes its capabilities as plain `async fn`s
//! behind a trait object (`Arc<dyn ServiceTrait>`). Concrete composition lives
//! above this crate in `neo-system` and the runnable `neo-node` daemon.
//!
//! Today the network stack is the one component wired through this vocabulary
//! ([`NetworkService`], backing primitive `Arc<dyn NetworkService>`). The
//! earlier reth-style `BlockExecutor` / `ConsensusApi` / `EngineApi` service
//! traits were removed in ADR-033 as never-instantiated scaffolding (zero
//! production implementations; the node wires block execution, consensus, and
//! the engine surface through concrete types). The transaction pool is
//! deliberately not a service trait either: the concrete mempool
//! (`neo-mempool`) is reached through the blockchain/node-service wiring.

use crate::error::ServiceError;
use crate::outcome::NetworkEvent;
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

#[cfg(test)]
#[path = "../tests/service/services.rs"]
mod tests;
