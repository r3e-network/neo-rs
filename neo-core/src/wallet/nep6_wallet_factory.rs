use std::path::Path;
use crate::protocol_settings::ProtocolSettings;
use crate::wallet::{ NEP6Wallet, WalletError};
use crate::wallet::iwallet_factory::IWalletFactory;

pub struct NEP6WalletFactory;

impl NEP6WalletFactory {
    pub const INSTANCE: Self = Self;

    pub fn handle(&self, path: &str) -> bool {
        Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase() == "json")
            .unwrap_or(false)
    }

    pub fn create_wallet(&self, name: &str, path: &str, password: &str, settings: &ProtocolSettings) -> Result<NEP6Wallet, WalletError> {
        if Path::new(path).exists() {
            return Err(WalletError::InvalidOperation("The wallet file already exists.".into()));
        }
        let wallet = NEP6Wallet::new(path, password, settings, name)?;
        wallet.save()?;
        Ok(wallet)
    }

    pub fn open_wallet(&self, path: &str, password: &str, settings: &ProtocolSettings) -> Result<NEP6Wallet, WalletError> {
        NEP6Wallet::open(path, password, settings)
    }
}

impl IWalletFactory for NEP6WalletFactory {
    fn handle(&self, path: &str) -> bool {
        self.handle(path)
    }

    fn create_wallet(&self, name: &str, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, WalletError> {
        self.create_wallet(name, path, password, settings).map(|w| Box::new(w) as Box<dyn Wallet>)
    }

    fn open_wallet(&self, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, WalletError> {
        self.open_wallet(path, password, settings).map(|w| Box::new(w) as Box<dyn Wallet>)
    }
}
