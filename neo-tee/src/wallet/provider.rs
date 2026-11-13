//! TEE Wallet Provider - integrates with neo-core wallet system

use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use crate::wallet::TeeWallet;
use std::path::Path;
use std::sync::Arc;

/// Provider for TEE-protected wallets
pub struct TeeWalletProvider {
    enclave: Arc<TeeEnclave>,
}

impl TeeWalletProvider {
    /// Create a new TEE wallet provider
    pub fn new(enclave: Arc<TeeEnclave>) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        Ok(Self { enclave })
    }

    /// Create a new TEE wallet
    pub fn create_wallet(&self, name: &str, path: &Path) -> TeeResult<TeeWallet> {
        TeeWallet::create(self.enclave.clone(), name, path)
    }

    /// Open an existing TEE wallet
    pub fn open_wallet(&self, path: &Path) -> TeeResult<TeeWallet> {
        TeeWallet::open(self.enclave.clone(), path)
    }

    /// Check if a path contains a TEE wallet
    pub fn is_tee_wallet(path: &Path) -> bool {
        path.join("wallet.json").exists()
    }

    /// Get the enclave reference
    pub fn enclave(&self) -> &Arc<TeeEnclave> {
        &self.enclave
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use tempfile::tempdir;

    #[test]
    fn test_wallet_provider() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().join("enclave"),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();

        let provider = TeeWalletProvider::new(enclave).unwrap();

        let wallet_path = temp.path().join("my_wallet");
        let wallet = provider.create_wallet("test", &wallet_path).unwrap();

        assert!(TeeWalletProvider::is_tee_wallet(&wallet_path));

        // Reopen wallet
        drop(wallet);
        let _reopened = provider.open_wallet(&wallet_path).unwrap();
    }
}
