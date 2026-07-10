//! Thread-safe wallet provider for the [`crate::Node`].
//!
//! A [`WalletProvider`] wraps the optional NEP-6 wallet used by node services.
//! The handle is concrete so wallet operations stay statically typed instead
//! of routing every account access through erased wallet dispatch.

use neo_wallets::Nep6Wallet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe holder for the optional node wallet.
#[derive(Clone, Default)]
pub struct WalletProvider {
    /// Inner `RwLock` over the current wallet, if any.
    inner: Arc<RwLock<Option<Arc<Nep6Wallet>>>>,
}

impl std::fmt::Debug for WalletProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletProvider")
            .field("has_wallet", &self.peek().is_some())
            .finish()
    }
}

impl WalletProvider {
    /// Construct an empty [`WalletProvider`] (no wallet loaded).
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a [`WalletProvider`] that starts with `wallet`
    /// already loaded.
    pub fn with_wallet(wallet: Arc<Nep6Wallet>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Some(wallet))),
        }
    }

    /// Returns a read guard to the current wallet (if any).
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, Option<Arc<Nep6Wallet>>> {
        self.inner.read().await
    }

    /// Returns `true` if a wallet is currently loaded.
    pub async fn is_loaded(&self) -> bool {
        self.inner.read().await.is_some()
    }

    /// Install a new wallet, replacing the previous one.
    pub async fn install(&self, wallet: Arc<Nep6Wallet>) {
        *self.inner.write().await = Some(wallet);
    }

    /// Drop the current wallet, if any.
    pub async fn clear(&self) {
        *self.inner.write().await = None;
    }

    /// Returns a non-async peek at the current wallet's presence.
    /// Useful for `Debug` formatting.
    fn peek(&self) -> Option<Arc<Nep6Wallet>> {
        self.inner.try_read().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
#[path = "../tests/composition/wallet_provider.rs"]
mod tests;
