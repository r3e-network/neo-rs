use crate::Wallet;

use std::any::Any;
use std::sync::{Arc, mpsc};

/// A provider for obtaining wallet instance.
/// Matches C# WalletProvider exactly
pub trait WalletProvider: Send + Sync + Any {
    /// Returns a type-erased view of the provider for event dispatch.
    fn as_any(&self) -> &dyn Any;

    /// Triggered when a wallet is opened or closed.
    /// Matches C# WalletChanged event
    fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<dyn Wallet>>>;

    /// Get the currently opened Wallet instance.
    /// Matches C# GetWallet method
    fn wallet(&self) -> Option<Arc<dyn Wallet>>;
}
