//! Wallet event handler types.
//!
//! Distributed from `neo-core::events::handlers`.

use crate::Wallet;
use std::any::Any;
use std::sync::Arc;

/// Handler of WalletChanged event from the WalletProvider.
/// Triggered when a new wallet is assigned to the node.
pub trait WalletChangedHandler {
    /// Called when the node's active wallet changes.
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &dyn Any,
        wallet: Option<Arc<dyn Wallet>>,
    );
}
