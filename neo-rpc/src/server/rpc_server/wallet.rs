//! Active-wallet state for RPC wallet methods.
//!
//! The RPC server keeps one optional wallet handle for wallet RPC endpoints
//! such as `openwallet`, `closewallet`, and transfer construction. This module
//! owns that state so the root server module stays a structural facade.

use neo_wallets::Nep6Wallet;
use parking_lot::RwLock;
use std::sync::Arc;

use super::RpcServer;

/// Shared active-wallet slot exposed to wallet RPC handlers.
pub(super) type WalletHandle = Arc<RwLock<Option<Arc<Nep6Wallet>>>>;

/// Create an empty active-wallet slot.
pub(super) fn new_wallet_handle() -> WalletHandle {
    Arc::new(RwLock::new(None))
}

impl RpcServer {
    /// Set or clear the wallet exposed to wallet RPC methods.
    pub fn set_wallet(&self, wallet: Option<Arc<Nep6Wallet>>) {
        *self.wallet.write() = wallet;
    }

    /// Return the wallet currently exposed to wallet RPC methods.
    #[must_use]
    pub fn wallet(&self) -> Option<Arc<Nep6Wallet>> {
        self.wallet.read().clone()
    }
}
