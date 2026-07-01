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

use std::any::Any;
use std::sync::Arc;

use crate::{ApplicationExecuted, Block};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use tracing::debug;

/// Lightweight plugin event enum for internal event broadcasting.
/// Replaces the previous plugin system with simple logging.
pub enum PluginEvent {
    /// Node has started with system reference.
    NodeStarted {
        /// Reference to the NeoSystem, or another system implementation.
        system: Arc<dyn Any + Send + Sync>,
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

impl std::fmt::Debug for PluginEvent {
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

impl std::fmt::Display for PluginEvent {
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

impl PluginEvent {
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
    fn blockchain_committed_handler(&self, system: &dyn Any, block: &Block);
}

/// Implemented by services that need to react to a block being committed to
/// the snapshot. Mirrors the C# `ICommittingHandler` interface.
pub trait CommittingHandler: Send + Sync {
    /// Called when a block is about to be committed.
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    );
}

/// Implemented by services that need to react to wallet changes
/// (e.g. open/close/lock/unlock of accounts). Mirrors the C#
/// `IWalletChangedHandler` interface.
pub trait WalletChangedHandler: Send + Sync {
    /// Called when the active wallet changes.
    ///
    /// The `wallet` argument is a type-erased handle so this trait
    /// can live in a leaf crate without depending on the full
    /// `neo-wallets` API. Implementations should downcast to the
    /// concrete `Arc<dyn neo_wallets::Wallet>` they expect.
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &dyn Any,
        wallet: Option<Arc<dyn Any + Send + Sync>>,
    );
}

/// Minimal signing interface used by [`WalletChangedHandler`]. The trait is
/// intentionally narrow; the full wallet API lives in `neo-wallets` as
/// `WalletProvider`. This trait focuses only on account lookup and signing.
pub trait SignerProvider: Send + Sync {
    /// Get an account by script hash.
    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn AccountLike>>;
    /// Sign arbitrary data with the account's private key.
    fn sign(&self, data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>, String>;
    /// Whether the wallet holds the account identified by the script hash.
    fn contains(&self, script_hash: &UInt160) -> bool;
}

/// Minimal account interface; full account API lives in `neo-wallets`.
pub trait AccountLike: Send + Sync {
    /// Get the script hash.
    fn script_hash(&self) -> UInt160;
    /// Whether the account is locked.
    fn is_locked(&self) -> bool;
    /// Whether the account has a private key.
    fn has_key(&self) -> bool;
    /// Get the public key (if available).
    fn get_key(&self) -> Option<Vec<u8>>;
}

/// Implemented by services that need to react to P2P messages. Mirrors the
/// C# `IMessageReceivedHandler` interface.
pub trait MessageReceivedHandler: Send + Sync {
    /// Return `true` to keep the message flowing, `false` to drop it.
    fn remote_node_message_received_handler(
        &self,
        system: &dyn Any,
        message: &dyn MessageLike,
    ) -> bool;
}

/// Minimal P2P message interface used by [`MessageReceivedHandler`].
pub trait MessageLike: Send + Sync {
    /// Raw payload bytes.
    fn payload(&self) -> &[u8];
    /// Message command (e.g. transaction, block).
    fn command(&self) -> u8;
}
/// Convenience re-export for plugins that only need the witness type.
pub use crate::Witness as WitnessType;

#[cfg(test)]
#[path = "../tests/execution/event_handlers.rs"]
mod tests;
