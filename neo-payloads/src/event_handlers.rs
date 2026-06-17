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
mod tests {
    use super::*;

    #[test]
    fn debug_does_not_leak_system_reference() {
        let event = PluginEvent::NodeStarted {
            system: Arc::new(()) as Arc<dyn Any + Send + Sync>,
        };
        let formatted = format!("{:?}", event);
        assert!(formatted.contains("NodeStarted"));
        assert!(!formatted.contains("()"));
    }

    #[test]
    fn plugin_event_display_includes_block_context() {
        let event = PluginEvent::BlockReceived {
            block_hash: "0xabcd".to_string(),
            block_height: 42,
        };
        let formatted = format!("{}", event);
        assert!(formatted.contains("BlockReceived"));
        assert!(formatted.contains("0xabcd"));
        assert!(formatted.contains("42"));
    }

    struct MockAccount {
        hash: UInt160,
        locked: bool,
    }
    impl AccountLike for MockAccount {
        fn script_hash(&self) -> UInt160 {
            self.hash
        }
        fn is_locked(&self) -> bool {
            self.locked
        }
        fn has_key(&self) -> bool {
            true
        }
        fn get_key(&self) -> Option<Vec<u8>> {
            Some(vec![1, 2, 3])
        }
    }

    #[test]
    fn account_like_dispatches_through_dyn() {
        let hash = UInt160::from_bytes(&[4u8; 20]).unwrap();
        let acct: Arc<dyn AccountLike> = Arc::new(MockAccount { hash, locked: true });
        assert_eq!(acct.script_hash(), hash);
        assert!(acct.is_locked());
        assert!(acct.has_key());
        assert_eq!(acct.get_key(), Some(vec![1, 2, 3]));
    }

    struct MockMessage {
        cmd: u8,
        data: Vec<u8>,
    }
    impl MessageLike for MockMessage {
        fn payload(&self) -> &[u8] {
            &self.data
        }
        fn command(&self) -> u8 {
            self.cmd
        }
    }

    struct DropEverything;
    impl MessageReceivedHandler for DropEverything {
        fn remote_node_message_received_handler(
            &self,
            _system: &dyn Any,
            _message: &dyn MessageLike,
        ) -> bool {
            false
        }
    }

    #[test]
    fn message_handler_dispatches_through_dyn() {
        let handler: Arc<dyn MessageReceivedHandler> = Arc::new(DropEverything);
        let msg = MockMessage {
            cmd: 0x2b,
            data: vec![9, 9],
        };
        assert_eq!(msg.command(), 0x2b);
        assert_eq!(msg.payload(), &[9, 9]);
        // The handler drops the message (returns false), invoked via the trait
        // object with a type-erased `&dyn Any` system handle.
        assert!(!handler.remote_node_message_received_handler(&(), &msg));
    }

    struct MockWallet {
        account: UInt160,
    }
    impl SignerProvider for MockWallet {
        fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn AccountLike>> {
            (*script_hash == self.account).then(|| {
                Arc::new(MockAccount {
                    hash: self.account,
                    locked: false,
                }) as Arc<dyn AccountLike>
            })
        }
        fn sign(&self, data: &[u8], _script_hash: &UInt160) -> Result<Vec<u8>, String> {
            Ok(data.to_vec())
        }
        fn contains(&self, script_hash: &UInt160) -> bool {
            *script_hash == self.account
        }
    }

    #[test]
    fn wallet_provider_lookup_and_sign() {
        let acct = UInt160::from_bytes(&[5u8; 20]).unwrap();
        let other = UInt160::from_bytes(&[6u8; 20]).unwrap();
        let wallet: Arc<dyn SignerProvider> = Arc::new(MockWallet { account: acct });
        assert!(wallet.contains(&acct));
        assert!(!wallet.contains(&other));
        assert!(wallet.get_account(&acct).is_some());
        assert!(wallet.get_account(&other).is_none());
        assert_eq!(wallet.sign(b"hi", &acct).unwrap(), b"hi");
    }

    struct NoopCommitted;
    impl CommittedHandler for NoopCommitted {
        fn blockchain_committed_handler(&self, _system: &dyn Any, _block: &Block) {}
    }
    struct NoopWalletChanged;
    impl WalletChangedHandler for NoopWalletChanged {
        fn wallet_provider_wallet_changed_handler(
            &self,
            _sender: &dyn Any,
            _wallet: Option<Arc<dyn Any + Send + Sync>>,
        ) {
        }
    }

    #[test]
    fn lifecycle_handler_traits_are_object_safe() {
        // Constructing the trait objects confirms object-safety (these are used
        // as `dyn` handlers by plugins/services).
        let _committed: Arc<dyn CommittedHandler> = Arc::new(NoopCommitted);
        let _wallet_changed: Arc<dyn WalletChangedHandler> = Arc::new(NoopWalletChanged);
    }
}
