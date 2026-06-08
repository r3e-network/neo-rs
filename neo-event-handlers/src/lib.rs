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
use neo_payloads::{Block, Transaction, Witness};
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
