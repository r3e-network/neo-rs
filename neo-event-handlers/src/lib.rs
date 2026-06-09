//! Event handler traits used by Neo plugins and services.
//!
//! The original `Neo.Core.Events.Handlers` namespace exposed a small set of
//! callbacks used by plugins (ApplicationLogs, TokensTracker, OracleService,
//! dBFT controller) to react to block/tx lifecycle events and to the
//! wallet-changed event broadcast. They were lifted out of `neo-core` so
//! leaf plugins can depend on them without depending on the full
//! `neo-core` runtime.

use std::any::Any;
use std::sync::Arc;

use neo_block::ApplicationExecuted;
use neo_primitives::UInt160;
use neo_payloads::Block;
use neo_storage::persistence::DataCache;

/// Implemented by services that need to react to a block being committed to
/// the canonical chain. Mirrors the C# `ICommittedHandler` interface.
pub trait CommittedHandler: Send + Sync {
    /// Called after a block has been committed.
    fn blockchain_committed_handler(
        &self,
        system: &dyn Any,
        block: &Block,
    );
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

/// Minimal wallet interface used by [`WalletChangedHandler`]. The trait is
/// intentionally narrow; the full wallet API lives in `neo-wallets`.
pub trait WalletProvider: Send + Sync {
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
pub use neo_payloads::Witness as WitnessType;

#[cfg(test)]
mod tests {
    use super::*;

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
        let msg = MockMessage { cmd: 0x2b, data: vec![9, 9] };
        assert_eq!(msg.command(), 0x2b);
        assert_eq!(msg.payload(), &[9, 9]);
        // The handler drops the message (returns false), invoked via the trait
        // object with a type-erased `&dyn Any` system handle.
        assert!(!handler.remote_node_message_received_handler(&(), &msg));
    }

    struct MockWallet {
        account: UInt160,
    }
    impl WalletProvider for MockWallet {
        fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn AccountLike>> {
            (*script_hash == self.account).then(|| {
                Arc::new(MockAccount { hash: self.account, locked: false }) as Arc<dyn AccountLike>
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
        let wallet: Arc<dyn WalletProvider> = Arc::new(MockWallet { account: acct });
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
