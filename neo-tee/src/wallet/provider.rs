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
#[path = "../tests/wallet/provider.rs"]
mod tests;
