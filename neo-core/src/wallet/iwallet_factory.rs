use crate::protocol_settings::ProtocolSettings;
use crate::wallet::Wallet;

/// Trait defining the interface for a wallet factory in the Neo ecosystem.
pub trait IWalletFactory {
    /// Checks if the factory can handle the wallet at the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - A string slice that holds the path to the wallet file.
    ///
    /// # Returns
    ///
    /// Returns `true` if the factory can handle the wallet, `false` otherwise.
    fn handle(&self, path: &str) -> bool;

    /// Creates a new wallet.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the wallet.
    /// * `path` - The path where the wallet will be stored.
    /// * `password` - The password to encrypt the wallet.
    /// * `settings` - The protocol settings for the Neo network.
    ///
    /// # Returns
    ///
    /// Returns a `Result` which is `Ok` with the created `Wallet` if successful,
    /// or an `Err` with a descriptive error message if the creation fails.
    fn create_wallet(&self, name: &str, path: &str, password: &str, settings: &ProtocolSettings) -> Result<dyn Wallet<CreateError=()>, String>;

    /// Opens an existing wallet.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the existing wallet file.
    /// * `password` - The password to decrypt the wallet.
    /// * `settings` - The protocol settings for the Neo network.
    ///
    /// # Returns
    ///
    /// Returns a `Result` which is `Ok` with the opened `Wallet` if successful,
    /// or an `Err` with a descriptive error message if opening fails.
    fn open_wallet(&self, path: &str, password: &str, settings: &ProtocolSettings) -> Result<dyn Wallet<CreateError=()>, String>;
}
