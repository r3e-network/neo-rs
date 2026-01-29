//! Wallet provider for propagating RPC wallet changes into NeoSystem.

use neo_core::wallets::{IWalletProvider, Wallet};
use parking_lot::{Mutex, RwLock};
use std::any::Any;
use std::sync::{mpsc, Arc};

/// Type alias for optional wallet reference.
type OptionalWallet = Option<Arc<dyn Wallet>>;

/// Simple wallet provider that forwards wallet changes through an mpsc channel.
pub struct NodeWalletProvider {
    sender: mpsc::Sender<OptionalWallet>,
    receiver: Mutex<Option<mpsc::Receiver<OptionalWallet>>>,
    current: RwLock<OptionalWallet>,
}

impl NodeWalletProvider {
    /// Creates a new provider with an empty wallet.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            current: RwLock::new(None),
        }
    }

    /// Updates the current wallet and notifies subscribers.
    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.current.write() = wallet.clone();
        let _ = self.sender.send(wallet);
    }
}

impl IWalletProvider for NodeWalletProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<dyn Wallet>>> {
        self.receiver
            .lock()
            .take()
            .expect("wallet changed receiver already taken")
    }

    fn get_wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.current.read().clone()
    }
}
