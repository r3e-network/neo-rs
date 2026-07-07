//! Active-wallet state for RPC wallet methods.
//!
//! The RPC server keeps one optional wallet handle for wallet RPC endpoints
//! such as `openwallet`, `closewallet`, and transfer construction. This module
//! owns that state and its change callback so the root server module stays a
//! structural facade.

use neo_wallets::Wallet;
use parking_lot::RwLock;
use std::sync::Arc;

use super::RpcServer;

/// Shared active-wallet slot exposed to wallet RPC handlers.
pub(super) type WalletHandle = Arc<RwLock<Option<Arc<dyn Wallet>>>>;

/// Callback invoked after the active wallet changes.
pub type WalletChangeCallback = Arc<dyn Fn(Option<Arc<dyn Wallet>>) + Send + Sync>;

/// Create an empty active-wallet slot.
pub(super) fn new_wallet_handle() -> WalletHandle {
    Arc::new(RwLock::new(None))
}

impl RpcServer {
    /// Set or clear the wallet exposed to wallet RPC methods.
    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.wallet.write() = wallet;
        if let Some(callback) = &self.wallet_change_callback {
            callback(self.wallet.read().clone());
        }
    }

    /// Return the wallet currently exposed to wallet RPC methods.
    #[must_use]
    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    /// Install a callback invoked whenever the active wallet changes.
    pub fn set_wallet_change_callback(&mut self, callback: Option<WalletChangeCallback>) {
        self.wallet_change_callback = callback;
    }
}
