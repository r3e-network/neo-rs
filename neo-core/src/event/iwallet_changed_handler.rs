use crate::wallet::Wallet;

pub trait IWalletChangedHandler {
    /// The handler of WalletChanged event from the `IWalletProvider`.
    /// Triggered when a new wallet is assigned to the node.
    ///
    /// # Arguments
    ///
    /// * `sender` - The source of the event
    /// * `wallet` - The new wallet being assigned to the system.
    fn iwallet_provider_wallet_changed_handler(&self, sender: &dyn std::any::Any, wallet: &dyn Wallet<CreateError=()>);
}
