//! Thread-safe wallet provider for the [`crate::Node`].
//!
//! A [`WalletProvider`] wraps an `Arc<tokio::sync::RwLock<Option<Arc<dyn neo_wallets::Wallet>>>>`
//! so the rest of the node can read the current wallet without
//! blocking the wallet-management task that mutates it. The shape
//! is identical to the legacy `NeoSystem`'s wallet slot.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe holder for the optional node wallet.
#[derive(Clone, Default)]
pub struct WalletProvider {
    /// Inner `RwLock` over the current wallet, if any.
    inner: Arc<RwLock<Option<Arc<dyn neo_wallets::Wallet>>>>,
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
    pub fn with_wallet(wallet: Arc<dyn neo_wallets::Wallet>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Some(wallet))),
        }
    }

    /// Returns a read guard to the current wallet (if any).
    pub async fn read(
        &self,
    ) -> tokio::sync::RwLockReadGuard<'_, Option<Arc<dyn neo_wallets::Wallet>>> {
        self.inner.read().await
    }

    /// Returns `true` if a wallet is currently loaded.
    pub async fn is_loaded(&self) -> bool {
        self.inner.read().await.is_some()
    }

    /// Install a new wallet, replacing the previous one.
    pub async fn install(&self, wallet: Arc<dyn neo_wallets::Wallet>) {
        *self.inner.write().await = Some(wallet);
    }

    /// Drop the current wallet, if any.
    pub async fn clear(&self) {
        *self.inner.write().await = None;
    }

    /// Returns a non-async peek at the current wallet's presence.
    /// Useful for `Debug` formatting.
    fn peek(&self) -> Option<Arc<dyn neo_wallets::Wallet>> {
        self.inner.try_read().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_provider_is_unloaded() {
        let p = WalletProvider::new();
        assert!(!p.is_loaded().await);
    }

    #[tokio::test]
    async fn clear_is_idempotent() {
        let p = WalletProvider::new();
        p.clear().await;
        assert!(!p.is_loaded().await);
    }
}
