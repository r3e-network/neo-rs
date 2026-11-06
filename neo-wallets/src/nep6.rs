//! NEP-6 wallet implementation.
//!
//! File-backed wallet matching the C# `Neo.Wallets.NEP6.NEP6Wallet` JSON layout.
//! Ported from `neo-core`'s `wallets::nep6`, re-based onto the standalone
//! dependency set with the following scope reductions (task 89 alignment items):
//!
//! * Ledger-backed operations (balances, unclaimed GAS, transfer/transaction
//!   building and signing) are not ported — they require forbidden crates.
//! * The on-chain `Contract` is represented by the lightweight
//!   [`crate::account::Contract`] placeholder.
//! * Account handles returned from the [`Wallet`] trait are clones/snapshots;
//!   mutation flows through the wallet under an internal lock.

use crate::account::{Contract, StandardWalletAccount, WalletAccount};
use crate::helper::Helper;
use crate::key_pair::KeyPair;
use crate::nep2::ScryptParameters;
use crate::wallet::{Version, Wallet, WalletError, WalletFactory, WalletResult};
use neo_primitives::UInt160;
use neo_protocol::ProtocolSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};

/// NEP-6 wallet representation matching C# `NEP6Wallet`.
#[derive(Debug)]
pub struct Nep6Wallet {
    name: Option<String>,
    path: Option<String>,
    version: Version,
    scrypt: ScryptParameters,
    accounts: RwLock<HashMap<UInt160, Nep6Account>>,
    extra: Option<Value>,
    protocol_settings: Arc<ProtocolSettings>,
}

/// NEP-6 account implementation (C# `NEP6Account`).
#[derive(Debug, Clone)]
pub struct Nep6Account {
    inner: StandardWalletAccount,
    extra: Option<Value>,
}

impl Nep6Account {
    /// Wraps a standard account with NEP-6 extra metadata.
    #[must_use]
    pub fn new(inner: StandardWalletAccount, extra: Option<Value>) -> Self {
        Self { inner, extra }
    }

    /// Returns the NEP-6 `extra` field.
    #[must_use]
    pub fn extra(&self) -> Option<&Value> {
        self.extra.as_ref()
    }

    /// Returns a reference to the underlying standard account.
    #[must_use]
    pub fn inner(&self) -> &StandardWalletAccount {
        &self.inner
    }
}

impl WalletAccount for Nep6Account {
    fn script_hash(&self) -> UInt160 {
        self.inner.script_hash()
    }
    fn address(&self) -> String {
        self.inner.address()
    }
    fn label(&self) -> Option<&str> {
        self.inner.label()
    }
    fn set_label(&mut self, label: Option<String>) {
        self.inner.set_label(label);
    }
    fn is_default(&self) -> bool {
        self.inner.is_default()
    }
    fn set_is_default(&mut self, is_default: bool) {
        self.inner.set_is_default(is_default);
    }
    fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
    fn has_key(&self) -> bool {
        self.inner.has_key()
    }
    fn get_key(&self) -> Option<KeyPair> {
        self.inner.get_key()
    }
    fn contract(&self) -> Option<&Contract> {
        self.inner.contract()
    }
    fn set_contract(&mut self, contract: Option<Contract>) {
        self.inner.set_contract(contract);
    }
    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        self.inner.protocol_settings()
    }
    fn unlock(&mut self, password: &str) -> WalletResult<bool> {
        self.inner.unlock(password)
    }
    fn lock(&mut self) {
        self.inner.lock();
    }
    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        self.inner.verify_password(password)
    }
    fn export_wif(&self) -> WalletResult<String> {
        self.inner.export_wif()
    }
    fn export_nep2(&self, password: &str) -> WalletResult<String> {
        self.inner.export_nep2(password)
    }
    fn create_witness(&self, sign_data: &[u8]) -> WalletResult<crate::account::Witness> {
        self.inner.create_witness(sign_data)
    }
}

// === NEP-6 JSON file schema (C# NEP6Wallet serialization) ===

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Nep6WalletFile {
    name: Option<String>,
    version: String,
    scrypt: ScryptParameters,
    accounts: Vec<Nep6AccountFile>,
    extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Nep6AccountFile {
    address: String,
    label: Option<String>,
    #[serde(rename = "isDefault")]
    is_default: bool,
    lock: bool,
    key: Option<String>,
    contract: Option<Nep6ContractFile>,
    extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Nep6ContractParameterFile {
    name: String,
    #[serde(rename = "type")]
    param_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Nep6ContractFile {
    script: String,
    parameters: Vec<Nep6ContractParameterFile>,
    deployed: bool,
}

impl Nep6Wallet {
    /// Creates a new, empty NEP-6 wallet with the given name and file path.
    #[must_use]
    pub fn create(
        name: Option<String>,
        path: Option<String>,
        settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            name,
            path,
            version: Version::nep6(),
            scrypt: ScryptParameters::default_nep6(),
            accounts: RwLock::new(HashMap::new()),
            extra: None,
            protocol_settings: settings,
        }
    }

    /// Opens an existing NEP-6 wallet file, verifying the password against any
    /// encrypted accounts.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::WalletFileNotFound`], [`WalletError::InvalidWalletFormat`],
    /// or [`WalletError::InvalidPassword`].
    pub fn open(path: &str, password: &str, settings: Arc<ProtocolSettings>) -> WalletResult<Self> {
        if !std::path::Path::new(path).exists() {
            return Err(WalletError::WalletFileNotFound(path.to_string()));
        }
        let contents = fs::read_to_string(path)?;
        let file: Nep6WalletFile =
            serde_json::from_str(&contents).map_err(|_| WalletError::InvalidWalletFormat)?;

        let version = parse_version(&file.version);
        let mut accounts = HashMap::new();
        for account_file in &file.accounts {
            let account = build_account(account_file, &settings)?;
            // Verify the password against encrypted accounts.
            if account.inner.nep2_key().is_some() && !account.inner.verify_password(password)? {
                return Err(WalletError::InvalidPassword);
            }
            accounts.insert(account.inner.script_hash(), account);
        }

        Ok(Self {
            name: file.name,
            path: Some(path.to_string()),
            version,
            scrypt: file.scrypt,
            accounts: RwLock::new(accounts),
            extra: file.extra,
            protocol_settings: settings,
        })
    }

    /// Serializes the wallet to its NEP-6 JSON file representation.
    fn to_file(&self) -> WalletResult<Nep6WalletFile> {
        let accounts = self
            .accounts
            .read()
            .map_err(|_| WalletError::WalletLocked)?;
        let mut account_files = Vec::with_capacity(accounts.len());
        for account in accounts.values() {
            account_files.push(account_to_file(
                account,
                self.protocol_settings.address_version,
            )?);
        }
        Ok(Nep6WalletFile {
            name: self.name.clone(),
            version: self.version.to_string(),
            scrypt: self.scrypt,
            accounts: account_files,
            extra: self.extra.clone(),
        })
    }

    fn write_file(&self, path: &str) -> WalletResult<()> {
        let file = self.to_file()?;
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| WalletError::Other(format!("failed to serialize wallet: {e}")))?;
        fs::write(path, json)?;
        Ok(())
    }

    fn insert_account(&self, account: Nep6Account) -> WalletResult<Arc<dyn WalletAccount>> {
        let hash = account.inner.script_hash();
        let mut accounts = self
            .accounts
            .write()
            .map_err(|_| WalletError::WalletLocked)?;
        accounts.insert(hash, account.clone());
        Ok(Arc::new(account))
    }

    fn account_from_key(
        &self,
        key_pair: KeyPair,
        nep2_key: Option<String>,
    ) -> WalletResult<Nep6Account> {
        let inner = StandardWalletAccount::new_with_key(
            key_pair,
            None,
            self.protocol_settings.clone(),
            nep2_key,
        );
        Ok(Nep6Account::new(inner, None))
    }
}

impl Wallet for Nep6Wallet {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }

    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    fn version(&self) -> &Version {
        &self.version
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        self.accounts
            .read()
            .map(|a| a.contains_key(script_hash))
            .unwrap_or(false)
    }

    fn create_account(&self, private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        let key_pair = KeyPair::from_private_key(private_key)?;
        let account = self.account_from_key(key_pair, None)?;
        self.insert_account(account)
    }

    fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        let inner = match key_pair {
            Some(key) => StandardWalletAccount::new_with_key(
                key,
                Some(contract),
                self.protocol_settings.clone(),
                None,
            ),
            None => StandardWalletAccount::new_watch_only(
                contract.script_hash(),
                Some(contract),
                self.protocol_settings.clone(),
            ),
        };
        self.insert_account(Nep6Account::new(inner, None))
    }

    fn create_account_watch_only(
        &self,
        script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        let inner = StandardWalletAccount::new_watch_only(
            script_hash,
            None,
            self.protocol_settings.clone(),
        );
        self.insert_account(Nep6Account::new(inner, None))
    }

    fn delete_account(&self, script_hash: &UInt160) -> WalletResult<bool> {
        let mut accounts = self
            .accounts
            .write()
            .map_err(|_| WalletError::WalletLocked)?;
        Ok(accounts.remove(script_hash).is_some())
    }

    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        let accounts = self.accounts.read().ok()?;
        accounts
            .get(script_hash)
            .cloned()
            .map(|a| Arc::new(a) as Arc<dyn WalletAccount>)
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        self.accounts
            .read()
            .map(|a| {
                a.values()
                    .cloned()
                    .map(|acc| Arc::new(acc) as Arc<dyn WalletAccount>)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        let accounts = self.accounts.read().ok()?;
        accounts
            .values()
            .find(|a| a.is_default())
            .cloned()
            .map(|a| Arc::new(a) as Arc<dyn WalletAccount>)
    }

    fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()> {
        let mut accounts = self
            .accounts
            .write()
            .map_err(|_| WalletError::WalletLocked)?;
        if !accounts.contains_key(script_hash) {
            return Err(WalletError::AccountNotFound(*script_hash));
        }
        for (hash, account) in accounts.iter_mut() {
            account.inner.set_is_default(hash == script_hash);
        }
        Ok(())
    }

    fn import_wif(&self, wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        let key_pair = KeyPair::from_wif(wif)?;
        let account = self.account_from_key(key_pair, None)?;
        self.insert_account(account)
    }

    fn import_nep2(&self, nep2_key: &str, password: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        let key_pair =
            KeyPair::from_nep2_string(nep2_key, password, self.protocol_settings.address_version)?;
        let account = self.account_from_key(key_pair, Some(nep2_key.to_string()))?;
        self.insert_account(account)
    }

    fn export(&self, path: &str, _password: &str) -> WalletResult<()> {
        self.write_file(path)
    }

    fn change_password(&self, old_password: &str, new_password: &str) -> WalletResult<bool> {
        if !self.verify_password(old_password)? {
            return Err(WalletError::InvalidPassword);
        }
        let version = self.protocol_settings.address_version;
        let mut accounts = self
            .accounts
            .write()
            .map_err(|_| WalletError::WalletLocked)?;
        for account in accounts.values_mut() {
            if let Some(key) = account.inner.get_key() {
                let new_nep2 = key
                    .to_nep2(new_password, version)
                    .map_err(|e| WalletError::SigningFailed(e.to_string()))?;
                account.inner.set_nep2_key(Some(new_nep2));
            }
        }
        Ok(true)
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        let accounts = self
            .accounts
            .read()
            .map_err(|_| WalletError::WalletLocked)?;
        let account = accounts
            .get(script_hash)
            .ok_or(WalletError::AccountNotFound(*script_hash))?;
        let key = account.inner.get_key().ok_or(WalletError::AccountLocked)?;
        key.sign(data)
    }

    fn unlock(&self, password: &str) -> WalletResult<bool> {
        let mut accounts = self
            .accounts
            .write()
            .map_err(|_| WalletError::WalletLocked)?;
        let mut ok = true;
        for account in accounts.values_mut() {
            if account.inner.nep2_key().is_some() && !account.inner.unlock(password)? {
                ok = false;
            }
        }
        Ok(ok)
    }

    fn lock(&self) {
        if let Ok(mut accounts) = self.accounts.write() {
            for account in accounts.values_mut() {
                account.inner.lock();
            }
        }
    }

    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        let accounts = self
            .accounts
            .read()
            .map_err(|_| WalletError::WalletLocked)?;
        for account in accounts.values() {
            if account.inner.nep2_key().is_some() && !account.inner.verify_password(password)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn save(&self) -> WalletResult<()> {
        let Some(path) = self.path.clone() else {
            return Err(WalletError::Other("wallet has no path".to_string()));
        };
        self.write_file(&path)
    }
}

/// Factory that opens/creates NEP-6 (`.json`) wallets.
#[derive(Debug, Default, Clone, Copy)]
pub struct Nep6WalletFactory;

impl WalletFactory for Nep6WalletFactory {
    fn handle(&self, path: &str) -> bool {
        path.to_ascii_lowercase().ends_with(".json")
    }

    fn create_wallet(
        &self,
        name: &str,
        path: &str,
        _password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> Result<Box<dyn Wallet>, String> {
        let wallet = Nep6Wallet::create(
            Some(name.to_string()),
            Some(path.to_string()),
            settings.clone(),
        );
        wallet.save().map_err(|e| e.to_string())?;
        Ok(Box::new(wallet))
    }

    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> Result<Box<dyn Wallet>, String> {
        Nep6Wallet::open(path, password, settings.clone())
            .map(|w| Box::new(w) as Box<dyn Wallet>)
            .map_err(|e| e.to_string())
    }
}

fn parse_version(text: &str) -> Version {
    let mut parts = text.split('.');
    let major = parts.next().and_then(|p| p.parse::<u8>().ok()).unwrap_or(1);
    let minor = parts.next().and_then(|p| p.parse::<u8>().ok()).unwrap_or(0);
    let build = parts.next().and_then(|p| p.parse::<u8>().ok()).unwrap_or(0);
    Version::new(major, minor, build)
}

fn build_account(
    file: &Nep6AccountFile,
    settings: &Arc<ProtocolSettings>,
) -> WalletResult<Nep6Account> {
    let version = settings.address_version;
    let script_hash = Helper::to_script_hash(&file.address, version).map_err(WalletError::Other)?;
    let contract = match &file.contract {
        Some(c) => Some(contract_from_file(c)?),
        None => None,
    };

    let mut inner = match &file.key {
        Some(nep2_key) => StandardWalletAccount::new_from_encrypted(
            script_hash,
            nep2_key.clone(),
            contract,
            settings.clone(),
        ),
        None => StandardWalletAccount::new_watch_only(script_hash, contract, settings.clone()),
    };
    inner.set_label(file.label.clone());
    inner.set_is_default(file.is_default);
    Ok(Nep6Account::new(inner, file.extra.clone()))
}

fn contract_from_file(file: &Nep6ContractFile) -> WalletResult<Contract> {
    let script = hex::decode(&file.script)
        .map_err(|e| WalletError::Other(format!("bad script hex: {e}")))?;
    Ok(Contract {
        script,
        parameter_names: file.parameters.iter().map(|p| p.name.clone()).collect(),
        deployed: file.deployed,
    })
}

fn account_to_file(account: &Nep6Account, version: u8) -> WalletResult<Nep6AccountFile> {
    let inner = &account.inner;
    let contract_file = inner.contract().map(contract_to_file);
    Ok(Nep6AccountFile {
        address: Helper::to_address(&inner.script_hash(), version),
        label: inner.label().map(str::to_string),
        is_default: inner.is_default(),
        lock: inner.is_locked(),
        key: inner.nep2_key().map(str::to_string),
        contract: contract_file,
        extra: account.extra.clone(),
    })
}

fn contract_to_file(contract: &Contract) -> Nep6ContractFile {
    Nep6ContractFile {
        script: hex::encode(&contract.script),
        parameters: contract
            .parameter_names
            .iter()
            .map(|name| Nep6ContractParameterFile {
                name: name.clone(),
                // The concrete parameter type is an 89-alignment item; the
                // single-signature placeholder uses the Signature type name.
                param_type: "Signature".to_string(),
            })
            .collect(),
        deployed: contract.deployed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Arc<ProtocolSettings> {
        Arc::new(ProtocolSettings::default_settings())
    }

    #[test]
    fn create_and_manage_accounts() {
        let wallet = Nep6Wallet::create(Some("test".into()), None, settings());
        assert_eq!(wallet.name(), "test");
        assert!(wallet.get_accounts().is_empty());

        let key = KeyPair::generate().unwrap();
        let account = wallet.create_account(key.private_key()).unwrap();
        let hash = account.script_hash();
        assert!(wallet.contains(&hash));
        assert_eq!(wallet.get_accounts().len(), 1);

        wallet.set_default_account(&hash).unwrap();
        assert_eq!(wallet.get_default_account().unwrap().script_hash(), hash);

        assert!(wallet.delete_account(&hash).unwrap());
        assert!(!wallet.contains(&hash));
    }

    #[test]
    fn sign_with_stored_account() {
        let wallet = Nep6Wallet::create(None, None, settings());
        let key = KeyPair::generate().unwrap();
        let account = wallet.create_account(key.private_key()).unwrap();
        let sig = wallet.sign(b"message", &account.script_hash()).unwrap();
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn save_and_open_round_trip() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("neo-wallets-nep6-{}.json", std::process::id()));
        let path_str = path.to_string_lossy().to_string();

        let wallet = Nep6Wallet::create(Some("rt".into()), Some(path_str.clone()), settings());
        let key = KeyPair::generate().unwrap();
        let nep2 = key.to_nep2("pw", settings().address_version).unwrap();
        let imported = wallet.import_nep2(&nep2, "pw").unwrap();
        let hash = imported.script_hash();
        wallet.save().unwrap();

        let reopened = Nep6Wallet::open(&path_str, "pw", settings()).unwrap();
        assert!(reopened.contains(&hash));
        assert_eq!(reopened.name(), "rt");
        assert!(Nep6Wallet::open(&path_str, "wrong", settings()).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn factory_handles_json_paths() {
        let factory = Nep6WalletFactory;
        assert!(factory.handle("wallet.json"));
        assert!(factory.handle("WALLET.JSON"));
        assert!(!factory.handle("wallet.db3"));
    }
}
