use crate::Wallet;
use neo_config::ProtocolSettings;

/// Wallet factory interface matching C# WalletFactory exactly
pub trait WalletFactory {
    /// Determines whether the factory can handle the specified path.
    /// Matches C# Handle method
    fn handle(&self, path: &str) -> bool;

    /// Creates a new wallet.
    /// Matches C# CreateWallet method
    fn create_wallet(
        &self,
        name: &str,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String>;

    /// Opens a wallet.
    /// Matches C# OpenWallet method
    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String>;
}
