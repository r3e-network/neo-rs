//! NEP-6 wallet implementation.
//!
//! File-backed wallet matching the C# `Neo.Wallets.NEP6.NEP6Wallet` JSON layout.
//! Ported from `neo-core`'s `wallets::nep6`, re-based onto the standalone
//! dependency set with the following scope reductions (task 89 alignment items):
//!
//! * Ledger-backed operations (balances, unclaimed GAS, transfer/transaction
//!   building and signing) are not ported — they require forbidden crates.
//! * Account handles returned from the [`Wallet`] trait are shared handles: they
//!   share key material and the wallet-wide unlock flag with the wallet, so
//!   locking the wallet revokes secret access through handles obtained earlier.
//! * Key-backed accounts created from raw key material or a WIF are NEP-2
//!   encrypted with the wallet password when the wallet is saved or exported.
//!   Without a password the write is refused instead of silently dropping keys.

use crate::account::{Contract, StandardWalletAccount, UnlockState, WalletAccount};
use crate::helper::Helper;
use crate::key_pair::KeyPair;
use crate::nep2::ScryptParameters;
use crate::wallet::{Version, Wallet, WalletError, WalletFactory, WalletResult};
use neo_primitives::{ContractParameterType, UInt160};
use neo_protocol::ProtocolSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::sync::{Arc, RwLock};

/// Password retained by a wallet so key-backed accounts that only hold
/// transient key material can be NEP-2 encrypted before they are persisted.
///
/// The hand-written [`fmt::Debug`] implementation keeps the secret out of logs
/// and error reports.
#[derive(Clone, Default)]
struct WalletPassword(Option<String>);

impl fmt::Debug for WalletPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "WalletPassword(<redacted>)"
        } else {
            "WalletPassword(<unset>)"
        })
    }
}

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
    /// Wallet-wide unlock flag shared with every account and every handle.
    unlock_state: Arc<UnlockState>,
    /// Password used to encrypt persisted key material (never logged).
    password: RwLock<WalletPassword>,
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
    ///
    /// A wallet created here starts unlocked and without a password. Callers
    /// that persist key-backed accounts must supply one first (through
    /// [`Wallet::unlock`], the factory, or the password argument of
    /// [`Wallet::export`]); otherwise writing the file fails with
    /// [`WalletError::MissingPassword`] instead of dropping the keys.
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
            // A fresh in-memory wallet owns its raw keys and may sign with
            // them; locking it clears this flag again.
            unlock_state: Arc::new(UnlockState::new(true)),
            password: RwLock::new(WalletPassword::default()),
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
        Self::from_file(path, password, settings)
    }

    /// Opens an existing NEP-6 wallet from a file (primary implementation).
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::WalletFileNotFound`], [`WalletError::InvalidWalletFormat`],
    /// or [`WalletError::InvalidPassword`].
    pub fn from_file(
        path: &str,
        password: &str,
        settings: Arc<ProtocolSettings>,
    ) -> WalletResult<Self> {
        if !std::path::Path::new(path).exists() {
            return Err(WalletError::WalletFileNotFound(path.to_string()));
        }
        let contents = fs::read_to_string(path)?;
        let file: Nep6WalletFile =
            serde_json::from_str(&contents).map_err(|_| WalletError::InvalidWalletFormat)?;

        let version = parse_version(&file.version);
        let unlock_state = Arc::new(UnlockState::new(false));
        let mut accounts = HashMap::new();
        for account_file in &file.accounts {
            let mut account = build_account(account_file, &settings)?;
            // Handles created from this wallet share its unlock flag, so the
            // accounts stored here (and every handle later produced from them)
            // can all be revoked by a single `lock` call.
            account.inner.set_unlock_state(Arc::clone(&unlock_state));
            // Opening with a password authenticates and unlocks this wallet
            // session. Keep the decrypted key in the account instead of
            // validating it and immediately discarding it: dumpprivkey,
            // importprivkey and transaction signing all operate on the opened
            // session, while `lock` still clears the key material explicitly.
            if account.inner.nep2_key().is_some() && !account.inner.unlock(password)? {
                return Err(WalletError::InvalidPassword);
            }
            accounts.insert(account.inner.script_hash(), account);
        }
        // Every encrypted account was decrypted above, so the wallet session is
        // unlocked from now on.
        unlock_state.set_unlocked(true);

        Ok(Self {
            name: file.name,
            path: Some(path.to_string()),
            version,
            scrypt: file.scrypt,
            accounts: RwLock::new(accounts),
            extra: file.extra,
            protocol_settings: settings,
            unlock_state,
            // The password authenticated this session; keep it so accounts
            // added later can be encrypted before they are written.
            password: RwLock::new(WalletPassword(Some(password.to_string()))),
        })
    }

    /// Serializes the wallet to its NEP-6 JSON file representation.
    ///
    /// `password` is required for accounts that only hold transient key
    /// material (created from a raw private key or a WIF). NEP-6 persists keys
    /// exclusively as the `key` (NEP-2) field, so writing such an account
    /// without one silently turns it into a watch-only account on the next
    /// load. See [`persisted_nep2_key`].
    fn to_file(&self, password: Option<&str>) -> WalletResult<Nep6WalletFile> {
        let accounts = self
            .accounts
            .read()
            .map_err(|_| WalletError::WalletLocked)?;
        let account_files = accounts
            .values()
            .map(|account| {
                account_to_file(account, self.protocol_settings.address_version, password)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Nep6WalletFile {
            name: self.name.clone(),
            version: self.version.to_string(),
            scrypt: self.scrypt,
            accounts: account_files,
            extra: self.extra.clone(),
        })
    }

    fn write_file(&self, path: &str, password: Option<&str>) -> WalletResult<()> {
        let file = self.to_file(password)?;
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| WalletError::Other(format!("failed to serialize wallet: {e}")))?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Returns the password this wallet encrypts persisted keys with, if any.
    fn stored_password(&self) -> Option<String> {
        self.password.read().ok().and_then(|guard| guard.0.clone())
    }

    /// Remembers the password used to protect persisted key material.
    fn remember_password(&self, password: &str) {
        if let Ok(mut guard) = self.password.write() {
            guard.0 = Some(password.to_string());
        }
    }

    fn insert_account(&self, mut account: Nep6Account) -> WalletResult<Arc<dyn WalletAccount>> {
        let hash = account.inner.script_hash();
        // The stored entry and the handle returned below share key material and
        // the wallet-wide unlock flag; locking the wallet revokes both.
        account
            .inner
            .set_unlock_state(Arc::clone(&self.unlock_state));
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
        // A key-backed NEP-6 account needs its canonical single-signature
        // contract for fee calculation, witness construction and persistence.
        // Leaving it as `None` makes the account usable for WIF export but
        // unusable for transfers after it is reopened.
        let contract = Contract::create_signature_contract(key_pair.get_public_key_point()?);
        let inner = StandardWalletAccount::new_with_key(
            key_pair,
            Some(contract),
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
            .is_ok_and(|a| a.contains_key(script_hash))
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
        // The handle shares its key material and the wallet unlock flag with the
        // stored entry, so `Wallet::lock` revokes it even after it was handed
        // out.
        accounts
            .get(script_hash)
            .map(|a| Arc::new(a.clone()) as Arc<dyn WalletAccount>)
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

    fn export(&self, path: &str, password: &str) -> WalletResult<()> {
        // An empty password falls back to the one this wallet was opened or
        // unlocked with. Accounts that already carry an encrypted key keep it;
        // transient key material is encrypted here rather than written in the
        // clear. Without any password the write is refused.
        let password = if password.is_empty() {
            self.stored_password()
        } else {
            Some(password.to_string())
        };
        self.write_file(path, password.as_deref())
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
            if account.inner.nep2_key().is_none() {
                // Watch-only account, or raw key material that is encrypted on
                // save with whatever password the wallet then holds.
                continue;
            }
            // Re-encryption works from the decrypted key, so make sure it is
            // present even if the account was locked in the meantime.
            if account.inner.key_for_persistence().is_none() {
                let _ = account.inner.unlock(old_password)?;
            }
            let key = account
                .inner
                .key_for_persistence()
                .ok_or(WalletError::AccountLocked)?;
            let new_nep2 = key
                .to_nep2(new_password, version)
                .map_err(|e| WalletError::SigningFailed(e.to_string()))?;
            account.inner.set_nep2_key(Some(new_nep2));
        }
        drop(accounts);
        // Later writes must use the new password for accounts that only hold
        // transient key material.
        self.remember_password(new_password);
        Ok(true)
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        if !self.unlock_state.is_unlocked() {
            return Err(WalletError::WalletLocked);
        }
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
        // Verify across every encrypted account first: a half-applied unlock
        // would leave the shared flag inconsistent with the key material.
        for account in accounts.values() {
            if account.inner.nep2_key().is_some() && !account.inner.verify_password(password)? {
                return Ok(false);
            }
        }
        for account in accounts.values_mut() {
            if account.inner.nep2_key().is_some() {
                let _ = account.inner.unlock(password)?;
            }
        }
        drop(accounts);
        self.unlock_state.set_unlocked(true);
        // Keep the password: accounts added later may only hold transient key
        // material and must be encrypted before they can be persisted.
        self.remember_password(password);
        Ok(true)
    }

    fn lock(&self) {
        if let Ok(mut accounts) = self.accounts.write() {
            for account in accounts.values_mut() {
                account.inner.lock();
            }
        }
        // Revoke secret access for handles that outlived this wallet call. They
        // share both the key material above and this flag, so a decrypted key
        // they still hold is unreachable from now on.
        self.unlock_state.set_unlocked(false);
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
        let password = self.stored_password();
        self.write_file(&path, password.as_deref())
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
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> Result<Box<dyn Wallet>, String> {
        let wallet = Nep6Wallet::create(
            Some(name.to_string()),
            Some(path.to_string()),
            settings.clone(),
        );
        // Retain the password so accounts added to this wallet can be NEP-2
        // encrypted before they are written to disk.
        wallet.remember_password(password);
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

    // Convert NEP-6 parameter names to ContractParameterType
    let parameter_list = file
        .parameters
        .iter()
        .map(|p| match p.param_type.as_str() {
            "Signature" => ContractParameterType::Signature,
            "Boolean" => ContractParameterType::Boolean,
            "Integer" => ContractParameterType::Integer,
            "Hash160" => ContractParameterType::Hash160,
            "Hash256" => ContractParameterType::Hash256,
            "ByteArray" => ContractParameterType::ByteArray,
            "PublicKey" => ContractParameterType::PublicKey,
            "String" => ContractParameterType::String,
            "Array" => ContractParameterType::Array,
            "Map" => ContractParameterType::Map,
            "InteropInterface" => ContractParameterType::InteropInterface,
            _ => ContractParameterType::Any,
        })
        .collect();

    Ok(Contract::create(parameter_list, script))
}

fn account_to_file(
    account: &Nep6Account,
    version: u8,
    password: Option<&str>,
) -> WalletResult<Nep6AccountFile> {
    let inner = &account.inner;
    let contract_file = inner.contract().map(contract_to_file);
    Ok(Nep6AccountFile {
        address: Helper::to_address(&inner.script_hash(), version),
        label: inner.label().map(str::to_string),
        is_default: inner.is_default(),
        lock: inner.is_locked(),
        key: persisted_nep2_key(inner, version, password)?,
        contract: contract_file,
        extra: account.extra.clone(),
    })
}

/// Returns the NEP-2 payload to persist for `inner`.
///
/// NEP-6 stores private keys only as the encrypted `key` field. An account
/// created from a raw private key or a WIF has none yet, so the decrypted key is
/// encrypted with `password` here rather than being dropped from the file.
///
/// # Errors
///
/// Returns [`WalletError::MissingPassword`] when key material exists but no
/// password is available to encrypt it, and [`WalletError::AccountLocked`] when
/// the key material cannot be recovered either.
fn persisted_nep2_key(
    inner: &StandardWalletAccount,
    version: u8,
    password: Option<&str>,
) -> WalletResult<Option<String>> {
    if let Some(nep2) = inner.nep2_key() {
        return Ok(Some(nep2));
    }
    if !inner.has_key() {
        // Watch-only account: there is nothing to protect.
        return Ok(None);
    }
    let Some(password) = password else {
        return Err(WalletError::MissingPassword);
    };
    let key = inner
        .key_for_persistence()
        .ok_or(WalletError::AccountLocked)?;
    Ok(Some(key.to_nep2(password, version)?))
}

fn contract_to_file(contract: &Contract) -> Nep6ContractFile {
    Nep6ContractFile {
        script: hex::encode(&contract.script),
        parameters: contract
            .parameter_list
            .iter()
            .enumerate()
            .map(|(i, param_type)| {
                let type_name = match param_type {
                    ContractParameterType::Signature => "Signature",
                    ContractParameterType::Boolean => "Boolean",
                    ContractParameterType::Integer => "Integer",
                    ContractParameterType::Hash160 => "Hash160",
                    ContractParameterType::Hash256 => "Hash256",
                    ContractParameterType::ByteArray => "ByteArray",
                    ContractParameterType::PublicKey => "PublicKey",
                    ContractParameterType::String => "String",
                    ContractParameterType::Array => "Array",
                    ContractParameterType::Map => "Map",
                    ContractParameterType::InteropInterface => "InteropInterface",
                    ContractParameterType::Any => "Any",
                    ContractParameterType::Void => "Void",
                };
                Nep6ContractParameterFile {
                    name: format!("parameter{i}"),
                    param_type: type_name.to_string(),
                }
            })
            .collect(),
        deployed: false, // Wallet contracts are typically not deployed
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
        assert_eq!(
            reopened.get_account(&hash).unwrap().export_wif().unwrap(),
            key.to_wif()
        );
        reopened.lock();
        assert!(reopened.get_account(&hash).unwrap().export_wif().is_err());
        assert!(Nep6Wallet::open(&path_str, "wrong", settings()).is_err());

        let _ = fs::remove_file(&path);
    }

    /// C-5: handles issued before `lock` must lose their secret access.
    #[test]
    fn lock_revokes_previously_issued_account_handles() {
        let wallet = Nep6Wallet::create(None, None, settings());
        let key = KeyPair::generate().unwrap();
        let handle = wallet.create_account(key.private_key()).unwrap();
        let hash = handle.script_hash();
        assert_eq!(handle.export_wif().unwrap(), key.to_wif());
        assert_eq!(wallet.sign(b"message", &hash).unwrap().len(), 64);

        wallet.lock();

        // Every secret-revealing path fails closed on the handle obtained before
        // the wallet was locked...
        assert!(matches!(
            handle.export_wif(),
            Err(WalletError::WalletLocked)
        ));
        assert!(matches!(
            handle.create_witness(b"message"),
            Err(WalletError::WalletLocked)
        ));
        assert!(handle.get_key().is_none());
        assert!(handle.is_locked());
        // ...and on the wallet itself.
        assert!(matches!(
            wallet.sign(b"message", &hash),
            Err(WalletError::WalletLocked)
        ));
        // The account still knows it owns key material; it just cannot use it.
        assert!(handle.has_key());
    }

    /// C-6: a raw-key account must be encrypted before it is persisted so that a
    /// save/load round trip keeps its key material.
    #[test]
    fn save_encrypts_raw_keys_before_persisting() {
        let path =
            std::env::temp_dir().join(format!("neo-wallets-nep6-raw-{}.json", std::process::id()));
        let path_str = path.to_string_lossy().to_string();

        let wallet = Nep6Wallet::create(Some("raw".into()), Some(path_str.clone()), settings());
        let key = KeyPair::generate().unwrap();
        let account = wallet.create_account(key.private_key()).unwrap();
        let hash = account.script_hash();

        // Without a password the wallet refuses to write (and writes nothing)
        // instead of silently turning the account into a watch-only entry.
        assert!(matches!(wallet.save(), Err(WalletError::MissingPassword)));
        assert!(!path.exists());

        assert!(wallet.unlock("pw").unwrap());
        wallet.save().unwrap();

        let reopened = Nep6Wallet::open(&path_str, "pw", settings()).unwrap();
        let reloaded = reopened
            .get_account(&hash)
            .expect("account survives reload");
        assert!(reloaded.has_key());
        assert_eq!(reloaded.export_wif().unwrap(), key.to_wif());
        assert!(Nep6Wallet::open(&path_str, "wrong", settings()).is_err());

        let _ = fs::remove_file(&path);
    }

    /// C-6: `export` encrypts raw keys with the password it is given.
    #[test]
    fn export_encrypts_raw_keys_with_the_supplied_password() {
        let path = std::env::temp_dir().join(format!(
            "neo-wallets-nep6-export-{}.json",
            std::process::id()
        ));
        let path_str = path.to_string_lossy().to_string();

        let wallet = Nep6Wallet::create(None, None, settings());
        let key = KeyPair::generate().unwrap();
        let account = wallet.create_account(key.private_key()).unwrap();
        let hash = account.script_hash();

        // No stored password and none supplied: refuse rather than drop the key.
        assert!(matches!(
            wallet.export(&path_str, ""),
            Err(WalletError::MissingPassword)
        ));
        assert!(!path.exists());

        wallet.export(&path_str, "pw").unwrap();
        let reopened = Nep6Wallet::open(&path_str, "pw", settings()).unwrap();
        assert_eq!(
            reopened.get_account(&hash).unwrap().export_wif().unwrap(),
            key.to_wif()
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn change_password_reencrypts_and_keeps_wallet_openable() {
        let path = std::env::temp_dir().join(format!(
            "neo-wallets-nep6-change-pw-{}.json",
            std::process::id()
        ));
        let path_str = path.to_string_lossy().to_string();

        let wallet = Nep6Wallet::create(Some("cp".into()), Some(path_str.clone()), settings());
        let key = KeyPair::generate().unwrap();
        let nep2 = key.to_nep2("old", settings().address_version).unwrap();
        let handle = wallet.import_nep2(&nep2, "old").unwrap();
        let hash = handle.script_hash();
        wallet.save().unwrap();

        assert!(wallet.change_password("old", "new").unwrap());
        wallet.save().unwrap();

        assert!(Nep6Wallet::open(&path_str, "old", settings()).is_err());
        let reopened = Nep6Wallet::open(&path_str, "new", settings()).unwrap();
        assert_eq!(
            reopened.get_account(&hash).unwrap().export_wif().unwrap(),
            key.to_wif()
        );

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
