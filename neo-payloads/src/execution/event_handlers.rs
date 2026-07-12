//! Event payloads and handler traits used by Neo plugins and services.
//!
//! The original `Neo.Core.Events.Handlers` namespace exposed a small set of
//! callbacks used by plugins (ApplicationLogs, TokensTracker, OracleService,
//! dBFT controller) to react to block/tx lifecycle events and to the
//! wallet-changed event broadcast. This crate also carries the lightweight
//! plugin event payloads formerly split into `neo-events`. Keeping those
//! event APIs together reduces workspace crate count without changing the
//! service-facing contracts. These now live beside the block execution payloads
//! in `neo-payloads`, the canonical home for ledger/payload data.

use std::sync::Arc;

use crate::{ApplicationExecuted, Block};
use neo_storage::persistence::DataCache;
use tracing::debug;

/// Lightweight plugin event enum for internal event broadcasting.
/// Replaces the previous plugin system with simple logging.
pub enum PluginEvent<System = ()> {
    /// Node has started with system reference.
    NodeStarted {
        /// Reference to the NeoSystem, or another system implementation.
        system: Arc<System>,
    },
    /// Node is stopping.
    NodeStopping,
    /// A block was received.
    BlockReceived {
        /// Block hash as a hex string.
        block_hash: String,
        /// Block height.
        block_height: u32,
    },
    /// A transaction was received.
    TransactionReceived {
        /// Transaction hash as a hex string.
        tx_hash: String,
    },
    /// Transaction added to mempool.
    MempoolTransactionAdded {
        /// Transaction hash as a hex string.
        tx_hash: String,
    },
    /// Transactions removed from mempool.
    MempoolTransactionRemoved {
        /// Transaction hashes as hex strings.
        tx_hashes: Vec<String>,
        /// Removal reason, stringified from the C# `TransactionRemovalReason`.
        reason: String,
    },
    /// A service was added.
    ServiceAdded {
        /// Service name.
        service_name: String,
    },
    /// Wallet changed.
    WalletChanged {
        /// Wallet name.
        wallet_name: String,
    },
}

impl<System> std::fmt::Debug for PluginEvent<System> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted {{ system: ... }}"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived {
                block_hash,
                block_height,
            } => f
                .debug_struct("BlockReceived")
                .field("block_hash", block_hash)
                .field("block_height", block_height)
                .finish(),
            PluginEvent::TransactionReceived { tx_hash } => f
                .debug_struct("TransactionReceived")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::MempoolTransactionAdded { tx_hash } => f
                .debug_struct("MempoolTransactionAdded")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => f
                .debug_struct("MempoolTransactionRemoved")
                .field("tx_hashes", tx_hashes)
                .field("reason", reason)
                .finish(),
            PluginEvent::ServiceAdded { service_name } => f
                .debug_struct("ServiceAdded")
                .field("service_name", service_name)
                .finish(),
            PluginEvent::WalletChanged { wallet_name } => f
                .debug_struct("WalletChanged")
                .field("wallet_name", wallet_name)
                .finish(),
        }
    }
}

impl<System> std::fmt::Display for PluginEvent<System> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived {
                block_hash,
                block_height,
            } => write!(f, "BlockReceived({}, height={})", block_hash, block_height),
            PluginEvent::TransactionReceived { tx_hash } => {
                write!(f, "TransactionReceived({})", tx_hash)
            }
            PluginEvent::MempoolTransactionAdded { tx_hash } => {
                write!(f, "MempoolTransactionAdded({})", tx_hash)
            }
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => {
                write!(f, "MempoolTransactionRemoved({:?}, {})", tx_hashes, reason)
            }
            PluginEvent::ServiceAdded { service_name } => {
                write!(f, "ServiceAdded({})", service_name)
            }
            PluginEvent::WalletChanged { wallet_name } => {
                write!(f, "WalletChanged({})", wallet_name)
            }
        }
    }
}

impl<System> PluginEvent<System> {
    /// Broadcasts a plugin event by logging it.
    #[inline]
    pub fn broadcast_plugin_event(&self) {
        debug!(target: "neo::events", event = %self, "plugin event");
    }
}

/// Implemented by services that need to react to a block being committed to
/// the canonical chain. Mirrors the C# `ICommittedHandler` interface.
pub trait CommittedHandler: Send + Sync {
    /// Called after a block has been committed.
    fn blockchain_committed_handler(&self, network: u32, block: &Block);
}

/// Implemented by services that need to react to a block being committed to
/// the snapshot. Mirrors the C# `ICommittingHandler` interface.
pub trait CommittingHandler: Send + Sync {
    /// Called when a block is about to be committed.
    fn blockchain_committing_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    );
}

/// High-level adapter for non-consensus projections derived from a finalized block.
///
/// The canonical node invokes this only after Ledger durability succeeds and
/// before it allows the next observer-visible block to mutate the supplied
/// snapshot. Implementors receive the same execution records as the legacy C#
/// committing hook, but their private store is prepared and committed as one
/// post-canonical operation.
pub trait FinalizedHandler: Send + Sync {
    /// Derives and commits one projection from a durably finalized block.
    fn blockchain_finalized_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    );
}

impl<T> FinalizedHandler for T
where
    T: CommittingHandler + CommittedHandler,
{
    fn blockchain_finalized_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        self.blockchain_committing_handler(network, block, snapshot, application_executed_list);
        self.blockchain_committed_handler(network, block);
    }
}

/// Implemented by services that need to react to wallet changes
/// (e.g. open/close/lock/unlock of accounts). Mirrors the C#
/// `IWalletChangedHandler` interface.
pub trait WalletChangedHandler: Send + Sync {
    /// Concrete event sender type selected by the dispatcher.
    type Sender: ?Sized;

    /// Concrete wallet handle selected by the dispatcher.
    type Wallet: Send + Sync + 'static;

    /// Called when the active wallet changes.
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &Self::Sender,
        wallet: Option<Arc<Self::Wallet>>,
    );
}

/// Convenience re-export for plugins that only need the witness type.
pub use crate::Witness as WitnessType;

#[cfg(test)]
#[path = "../tests/execution/event_handlers.rs"]
mod tests;
