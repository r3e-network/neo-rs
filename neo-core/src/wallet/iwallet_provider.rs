use std::sync::Arc;
use crate::wallet::wallet::Wallet;

/// A provider for obtaining wallet instance.
pub trait IWalletProvider {
    /// Triggered when a wallet is opened or closed.
    ///
    /// This is represented as a method in Rust, as Rust doesn't have direct equivalents to C# events.
    /// Users of this trait should implement this method to handle wallet changes.
    fn on_wallet_changed(&self, wallet: Option<Arc<Wallet>>);

    /// Get the currently opened Wallet instance.
    ///
    /// # Returns
    ///
    /// An Option containing a reference to the opened wallet, or None if no wallet is opened.
    fn get_wallet(&self) -> Option<Arc<Wallet>>;
}
