use std::sync::{Arc, mpsc};

/// A provider for obtaining wallet instance.
/// Matches C# WalletProvider exactly
pub trait WalletProvider: Send + Sync {
    /// Concrete wallet type managed by this provider.
    type Wallet: crate::Wallet + 'static;

    /// Triggered when a wallet is opened or closed.
    /// Matches C# WalletChanged event
    fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<Self::Wallet>>>;

    /// Get the currently opened Wallet instance.
    /// Matches C# GetWallet method
    fn wallet(&self) -> Option<Arc<Self::Wallet>>;
}
