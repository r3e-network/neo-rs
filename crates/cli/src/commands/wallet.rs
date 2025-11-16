use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::{anyhow, bail};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{KeyPair, Nep6Wallet, Wallet, WalletAccount};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::runtime::Handle;

/// Wallet management (`MainService.Wallet`).
pub struct WalletCommands {
    settings: Arc<ProtocolSettings>,
    current_wallet: Mutex<Option<WalletHandle>>,
}

#[derive(Clone)]
struct WalletHandle {
    wallet: Nep6Wallet,
    path: PathBuf,
}

impl WalletCommands {
    pub fn new(settings: Arc<ProtocolSettings>) -> Self {
        Self {
            settings,
            current_wallet: Mutex::new(None),
        }
    }

    /// Returns `true` when a wallet session is active.
    pub fn is_wallet_open(&self) -> bool {
        self.current_wallet
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Returns the currently loaded wallet (cloned).
    pub fn current_wallet(&self) -> Option<Nep6Wallet> {
        self.current_wallet
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| handle.wallet.clone()))
    }

    /// Returns the filesystem path of the opened wallet, if any.
    pub fn wallet_path(&self) -> Option<PathBuf> {
        self.current_wallet
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| handle.path.clone()))
    }

    /// Opens a wallet file (mirrors `MainService.OpenWallet`).
    pub fn open_wallet(&self, path: impl AsRef<Path>, password: &str) -> CommandResult {
        let path = path.as_ref();
        if !path.exists() {
            bail!("File does not exist: {}", path.display());
        }

        if password.is_empty() {
            bail!("wallet password cannot be empty");
        }

        if is_db3_wallet(path) {
            bail!("DB3 wallets are not supported yet; please migrate to a NEP-6 (.json) wallet.");
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("wallet path contains invalid UTF-8: {}", path.display()))?;

        let wallet = Nep6Wallet::from_file(path_str, password, self.settings.clone())
            .map_err(|err| anyhow!("failed to open wallet '{}': {err}", path.display()))?;

        let mut guard = self.lock_state()?;
        *guard = Some(WalletHandle {
            wallet,
            path: path.to_path_buf(),
        });

        Ok(())
    }

    /// Closes the current wallet session (mirrors `MainService.OnCloseWalletCommand`).
    pub fn close_wallet(&self) -> CommandResult {
        let mut guard = self.lock_state()?;
        if guard.take().is_none() {
            bail!("You have to open the wallet first.");
        }
        Ok(())
    }

    /// Creates a new NEP-6 wallet and opens it (mirrors `CreateWallet`).
    pub fn create_wallet(&self, path: impl AsRef<Path>, password: &str) -> CommandResult {
        let path = path.as_ref();
        if path.exists() {
            bail!("wallet file already exists: {}", path.display());
        }

        if password.is_empty() {
            bail!("wallet password cannot be empty");
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "wallet".to_string());
        let wallet = self.create_nep6_wallet(&name, path);
        let mut guard = self.lock_state()?;
        *guard = Some(WalletHandle {
            wallet,
            path: path.to_path_buf(),
        });
        Ok(())
    }

    fn create_nep6_wallet(&self, name: &str, path: &Path) -> Nep6Wallet {
        Nep6Wallet::new(
            Some(name.to_string()),
            Some(path.to_string_lossy().to_string()),
            self.settings.clone(),
        )
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, Option<WalletHandle>>, anyhow::Error> {
        self.current_wallet
            .lock()
            .map_err(|_| anyhow!("wallet state lock poisoned"))
    }

    pub fn list_addresses(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        for account in wallet.get_accounts() {
            self.print_account(&account);
        }
        Ok(())
    }

    fn print_account(&self, account: &Arc<dyn WalletAccount>) {
        let address = account.address();
        ConsoleHelper::info(["Address: ", address.as_str()]);
    }

    pub fn list_assets(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        ConsoleHelper::warning(
            "Asset listing is not fully implemented yet; showing addresses only.",
        );
        for account in wallet.get_accounts() {
            ConsoleHelper::info(["Address: ", account.address().as_str()]);
        }
        Ok(())
    }

    pub fn list_keys(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        for account in wallet
            .get_accounts()
            .into_iter()
            .filter(|acct| acct.has_key())
        {
            let address = account.address();
            let script_hash = account.script_hash().to_string();
            ConsoleHelper::info(["   Address: ", address.as_str()]);
            ConsoleHelper::info(["ScriptHash: ", script_hash.as_str()]);
            if let Some(key) = account.get_key() {
                let public_key = hex::encode(key.public_key());
                ConsoleHelper::info([" PublicKey: ", public_key.as_str()]);
            }
            ConsoleHelper::info([""]);
        }
        Ok(())
    }

    pub fn create_addresses(&self, count: u16) -> CommandResult {
        if count == 0 {
            bail!("count must be greater than zero");
        }

        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let handle = Handle::current();
        let mut addresses = Vec::new();
        for _ in 0..count {
            let key = KeyPair::generate().map_err(|err| anyhow!("{}", err))?;
            let private_key = key.private_key();
            let account = handle
                .block_on(wallet_handle.wallet.create_account(&private_key))
                .map_err(|err| anyhow!("{}", err))?;
            addresses.push(account.address());
        }

        let _ = handle.block_on(wallet_handle.wallet.save());
        fs::write("address.txt", addresses.join("\n"))?;
        ConsoleHelper::info(["Exported addresses to address.txt"]);
        Ok(())
    }

    pub fn delete_address(&self, address: &str) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let script_hash = WalletHelper::to_script_hash(address, self.settings.address_version)
            .map_err(|err| anyhow!(err))?;
        let handle = Handle::current();
        let removed = handle
            .block_on(wallet_handle.wallet.delete_account(&script_hash))
            .map_err(|err| anyhow!(err))?;
        if removed {
            let _ = handle.block_on(wallet_handle.wallet.save());
            ConsoleHelper::info(["Address deleted: ", address]);
        } else {
            ConsoleHelper::warning("Address not found in wallet.");
        }
        Ok(())
    }
}

fn is_db3_wallet(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("db3"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_wallet_requires_session() {
        let commands = WalletCommands::new(Arc::new(ProtocolSettings::default()));
        let err = commands.close_wallet().unwrap_err();
        assert!(err
            .to_string()
            .contains("You have to open the wallet first"));
    }

    #[test]
    fn open_wallet_requires_existing_file() {
        let commands = WalletCommands::new(Arc::new(ProtocolSettings::default()));
        let err = commands
            .open_wallet("missing.json", "password")
            .unwrap_err();
        assert!(err.to_string().contains("File does not exist"));
    }
}
