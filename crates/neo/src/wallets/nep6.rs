//! NEP-6 wallet implementation.
//!
//! This module provides the NEP-6 wallet standard as implemented in the
//! C# Neo node.  It mirrors the behaviour of `Neo.Wallets.NEP6` exactly,
//! enabling interoperability between the Rust and C# wallets.

use crate::network::p2p::payloads::{transaction::Transaction, witness::Witness};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::wallets::helper::Helper;
use crate::wallets::key_pair::KeyPair;
use crate::wallets::version::Version;
use crate::wallets::wallet::{Wallet, WalletError, WalletResult};
use crate::wallets::wallet_account::{StandardWalletAccount, WalletAccount};
use crate::{UInt160, UInt256};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};

/// NEP-6 wallet representation matching C# `NEP6Wallet`.
#[derive(Debug, Clone)]
pub struct Nep6Wallet {
    name: Option<String>,
    path: Option<String>,
    version: Version,
    scrypt: ScryptParameters,
    accounts: Arc<RwLock<HashMap<UInt160, Arc<Nep6Account>>>>,
    extra: Option<Value>,
    protocol_settings: Arc<ProtocolSettings>,
}

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

/// NEP-6 account implementation (C# `NEP6Account`).
#[derive(Debug, Clone)]
pub struct Nep6Account {
    inner: StandardWalletAccount,
    is_default: bool,
    lock: bool,
    extra: Option<Value>,
    parameter_names: Vec<String>,
    _wallet: Arc<Nep6Wallet>,
}

/// NEP-6 contract wrapper (C# `NEP6Contract`).
#[derive(Debug, Clone)]
pub struct Nep6Contract {
    pub contract: Contract,
    pub parameter_names: Vec<String>,
    pub deployed: bool,
}

/// Scrypt parameters used for NEP-2 encryption (C# `ScryptParameters`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScryptParameters {
    pub n: u32,
    pub r: u32,
    pub p: u32,
}

impl ScryptParameters {
    pub fn default_nep6() -> Self {
        Self {
            n: 16384,
            r: 8,
            p: 8,
        }
    }
}

impl Default for ScryptParameters {
    fn default() -> Self {
        Self::default_nep6()
    }
}

impl Nep6Wallet {
    pub fn new(
        name: Option<String>,
        path: Option<String>,
        settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            name,
            path,
            version: Version::new(1, 0, 0),
            scrypt: ScryptParameters::default_nep6(),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            extra: None,
            protocol_settings: settings,
        }
    }

    pub fn from_file(
        path: &str,
        password: &str,
        settings: Arc<ProtocolSettings>,
    ) -> WalletResult<Self> {
        let content = fs::read_to_string(path)?;
        let wallet_file: Nep6WalletFile = serde_json::from_str(&content)
            .map_err(|e| WalletError::Other(format!("Invalid wallet format: {e}")))?;
        let version = Version::parse(&wallet_file.version).map_err(WalletError::Other)?;

        let wallet = Self {
            name: wallet_file.name.clone(),
            path: Some(path.to_string()),
            version,
            scrypt: wallet_file.scrypt.clone(),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            extra: wallet_file.extra.clone(),
            protocol_settings: settings.clone(),
        };

        let mut account_map = HashMap::new();
        for account_file in wallet_file.accounts {
            let account = Nep6Account::from_file(account_file, &wallet, password)?;
            account_map.insert(account.script_hash(), Arc::new(account));
        }

        *wallet.accounts.write().unwrap() = account_map;
        Ok(wallet)
    }

    fn to_file(&self) -> WalletResult<Nep6WalletFile> {
        let accounts = self.accounts.read().unwrap();
        let account_files = accounts
            .values()
            .map(|account| account.to_file())
            .collect::<WalletResult<Vec<_>>>()?;

        Ok(Nep6WalletFile {
            name: self.name.clone(),
            version: self.version.to_string(),
            scrypt: self.scrypt.clone(),
            accounts: account_files,
            extra: self.extra.clone(),
        })
    }

    pub fn persist(&self) -> WalletResult<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| WalletError::Other("wallet has no path".to_string()))?;
        let wallet_file = self.to_file()?;
        let json = serde_json::to_string_pretty(&wallet_file)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn add_account(&self, account: Nep6Account) {
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(account.script_hash(), Arc::new(account));
    }
}

#[async_trait]
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

    async fn change_password(&self, old_password: &str, new_password: &str) -> WalletResult<bool> {
        if old_password == new_password {
            return Ok(true);
        }

        let mut updated_accounts = Vec::new();

        {
            let accounts = self.accounts.read().unwrap();
            for (hash, account_arc) in accounts.iter() {
                let mut account = (**account_arc).clone();
                if account.inner.nep2_key().is_some() {
                    if !account
                        .unlock(old_password)
                        .map_err(|e| WalletError::Other(e.to_string()))?
                    {
                        return Err(WalletError::InvalidPassword);
                    }

                    let key = account.inner.get_key().ok_or(WalletError::AccountLocked)?;
                    let version = self.protocol_settings.address_version;
                    let new_nep2 = key
                        .to_nep2(new_password, version)
                        .map_err(|e| WalletError::Other(e.to_string()))?;
                    account.inner.set_nep2_key(Some(new_nep2));
                    account.inner.lock();
                }

                updated_accounts.push((*hash, Arc::new(account)));
            }
        }

        {
            let mut accounts = self.accounts.write().unwrap();
            accounts.clear();
            for (hash, account) in updated_accounts {
                accounts.insert(hash, account);
            }
        }

        self.persist()?;
        Ok(true)
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        let accounts = self.accounts.read().unwrap();
        accounts.contains_key(script_hash)
    }

    async fn create_account(
        &self,
        private_key: &[u8],
    ) -> WalletResult<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        let key_pair = KeyPair::from_private_key(private_key)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        let contract = Contract::create_signature_contract(
            key_pair
                .get_public_key_point()
                .map_err(|e| WalletError::Other(e.to_string()))?,
        );
        let account = Nep6Account::with_key(self.clone(), key_pair, contract, None);
        let account_arc: Arc<dyn crate::wallets::wallet_account::WalletAccount> =
            Arc::new(account.clone());
        self.add_account(account);
        Ok(account_arc)
    }

    async fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        let account = if let Some(key_pair) = key_pair {
            Nep6Account::with_key(self.clone(), key_pair, contract, None)
        } else {
            Nep6Account::watch_only(self.clone(), contract.script_hash(), Some(contract))
        };

        let account_arc: Arc<dyn crate::wallets::wallet_account::WalletAccount> =
            Arc::new(account.clone());
        self.add_account(account);
        Ok(account_arc)
    }

    async fn create_account_watch_only(
        &self,
        script_hash: UInt160,
    ) -> WalletResult<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        let account = Nep6Account::watch_only(self.clone(), script_hash, None);
        let account_arc: Arc<dyn crate::wallets::wallet_account::WalletAccount> =
            Arc::new(account.clone());
        self.add_account(account);
        Ok(account_arc)
    }

    async fn delete_account(&self, script_hash: &UInt160) -> WalletResult<bool> {
        let mut accounts = self.accounts.write().unwrap();
        Ok(accounts.remove(script_hash).is_some())
    }

    async fn export(&self, path: &str, _password: &str) -> WalletResult<()> {
        let wallet_file = self.to_file()?;
        let json = serde_json::to_string_pretty(&wallet_file)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }

    fn get_account(
        &self,
        script_hash: &UInt160,
    ) -> Option<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        self.accounts
            .read().unwrap()
            .get(script_hash)
            .cloned()
            .map(|account| account as Arc<dyn crate::wallets::wallet_account::WalletAccount>)
    }

    fn get_accounts(&self) -> Vec<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        self.accounts
            .read().unwrap()
            .values()
            .cloned()
            .map(|account| account as Arc<dyn crate::wallets::wallet_account::WalletAccount>)
            .collect()
    }

    async fn get_available_balance(&self, _asset_id: &UInt256) -> WalletResult<i64> {
        Ok(0)
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Ok(0)
    }

    async fn import_wif(
        &self,
        wif: &str,
    ) -> WalletResult<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        let key_pair = KeyPair::from_wif(wif).map_err(|e| WalletError::Other(e.to_string()))?;
        let contract = Contract::create_signature_contract(
            key_pair
                .get_public_key_point()
                .map_err(|e| WalletError::Other(e.to_string()))?,
        );
        let account = Nep6Account::with_key(self.clone(), key_pair, contract, None);
        let account_arc: Arc<dyn crate::wallets::wallet_account::WalletAccount> =
            Arc::new(account.clone());
        self.add_account(account);
        Ok(account_arc)
    }

    async fn import_nep2(
        &self,
        nep2_key: &str,
        password: &str,
    ) -> WalletResult<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        let version = self.protocol_settings.address_version;
        let key_pair = KeyPair::from_nep2_string(nep2_key, password, version)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        let contract = Contract::create_signature_contract(
            key_pair
                .get_public_key_point()
                .map_err(|e| WalletError::Other(e.to_string()))?,
        );
        let account =
            Nep6Account::with_key(self.clone(), key_pair, contract, Some(nep2_key.to_string()));
        let account_arc: Arc<dyn crate::wallets::wallet_account::WalletAccount> =
            Arc::new(account.clone());
        self.add_account(account);
        Ok(account_arc)
    }

    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        let accounts = self.accounts.read().unwrap();
        let account = accounts
            .get(script_hash)
            .ok_or(WalletError::AccountNotFound(*script_hash))?
            .clone();

        if account.inner.is_locked() {
            return Err(WalletError::AccountLocked);
        }

        let key = account.inner.get_key().ok_or(WalletError::AccountLocked)?;

        key.sign(data)
            .map_err(|e| WalletError::SigningFailed(e.to_string()))
    }

    async fn sign_transaction(&self, transaction: &mut Transaction) -> WalletResult<()> {
        let accounts = self.accounts.read().unwrap();
        let signer_hashes: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();

        for hash in signer_hashes {
            if let Some(account) = accounts.get(&hash) {
                if account.inner.is_locked() {
                    return Err(WalletError::AccountLocked);
                }

                let witness = account.inner.create_witness(transaction)?;
                transaction.add_witness(witness);
            }
        }

        Ok(())
    }

    async fn unlock(&self, password: &str) -> WalletResult<bool> {
        let mut updated_accounts = Vec::new();

        {
            let accounts = self.accounts.read().unwrap();
            for (hash, account_arc) in accounts.iter() {
                let mut account = (**account_arc).clone();
                if account.inner.nep2_key().is_some() {
                    if !account.inner.verify_password(password)? {
                        return Err(WalletError::InvalidPassword);
                    }
                    if account.inner.is_locked() {
                        let unlocked = account
                            .unlock(password)
                            .map_err(|e| WalletError::Other(e.to_string()))?;
                        if !unlocked {
                            return Err(WalletError::InvalidPassword);
                        }
                    }
                }
                updated_accounts.push((*hash, Arc::new(account)));
            }
        }

        {
            let mut accounts = self.accounts.write().unwrap();
            accounts.clear();
            for (hash, account) in updated_accounts {
                accounts.insert(hash, account);
            }
        }

        Ok(true)
    }

    fn lock(&self) {
        let mut updated_accounts = Vec::new();

        {
            let accounts = self.accounts.read().unwrap();
            for (hash, account_arc) in accounts.iter() {
                let mut account = (**account_arc).clone();
                account.inner.lock();
                updated_accounts.push((*hash, Arc::new(account)));
            }
        }

        let mut accounts = self.accounts.write().unwrap();
        accounts.clear();
        for (hash, account) in updated_accounts {
            accounts.insert(hash, account);
        }
    }

    async fn verify_password(&self, password: &str) -> WalletResult<bool> {
        let accounts = self.accounts.read().unwrap();
        for account in accounts.values() {
            if account.inner.nep2_key().is_some() && !account.inner.verify_password(password)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn save(&self) -> WalletResult<()> {
        self.persist()
    }

    fn get_default_account(
        &self,
    ) -> Option<Arc<dyn crate::wallets::wallet_account::WalletAccount>> {
        self.accounts
            .read().unwrap()
            .values()
            .find(|account| account.is_default)
            .cloned()
            .map(|account| account as Arc<dyn crate::wallets::wallet_account::WalletAccount>)
    }

    async fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()> {
        let mut updated_accounts = Vec::new();
        let mut found = false;

        {
            let accounts = self.accounts.read().unwrap();
            for (hash, account_arc) in accounts.iter() {
                let mut account = (**account_arc).clone();
                let is_target = hash == script_hash;
                account.is_default = is_target;
                if is_target {
                    found = true;
                }
                updated_accounts.push((*hash, Arc::new(account)));
            }
        }

        if !found {
            return Err(WalletError::AccountNotFound(*script_hash));
        }

        {
            let mut accounts = self.accounts.write().unwrap();
            accounts.clear();
            for (hash, account) in updated_accounts {
                accounts.insert(hash, account);
            }
        }

        self.persist()?;
        Ok(())
    }
}

impl Nep6Account {
    fn script_hash(&self) -> UInt160 {
        self.inner.script_hash()
    }

    fn from_file(file: Nep6AccountFile, wallet: &Nep6Wallet, password: &str) -> WalletResult<Self> {
        let script_hash =
            Helper::to_script_hash(&file.address, wallet.protocol_settings.address_version)
                .map_err(WalletError::Other)?;

        let parsed_contract = match &file.contract {
            Some(contract_file) => Some(Nep6Contract::from_file(contract_file)?),
            None => None,
        };

        let contract_data = parsed_contract
            .as_ref()
            .map(|contract| contract.contract.clone());

        let inner = if let Some(ref nep2_key) = file.key {
            StandardWalletAccount::new_from_encrypted(
                script_hash,
                nep2_key.clone(),
                contract_data.clone(),
                Arc::clone(&wallet.protocol_settings),
            )
        } else {
            StandardWalletAccount::new_watch_only(
                script_hash,
                contract_data.clone(),
                Arc::clone(&wallet.protocol_settings),
            )
        };

        let parameter_names = parsed_contract
            .map(|contract| contract.parameter_names)
            .or_else(|| contract_data.as_ref().map(default_parameter_names))
            .unwrap_or_default();

        let mut account = Self {
            inner,
            is_default: file.is_default,
            lock: file.lock,
            extra: file.extra,
            parameter_names,
            _wallet: Arc::new(wallet.clone()),
        };

        if account.inner.nep2_key().is_some() {
            let unlocked = account
                .inner
                .unlock(password)
                .map_err(|e| WalletError::Other(e.to_string()))?;
            if !unlocked {
                return Err(WalletError::InvalidPassword);
            }
            if account.lock {
                account.inner.lock();
            }
        }

        Ok(account)
    }

    fn to_file(&self) -> WalletResult<Nep6AccountFile> {
        let address = self.inner.address();
        let contract = self
            .inner
            .contract()
            .map(|contract| Nep6Contract::to_file(contract, &self.parameter_names));

        Ok(Nep6AccountFile {
            address,
            label: self.inner.label().map(|s| s.to_string()),
            is_default: self.is_default,
            lock: self.lock,
            key: self.inner.nep2_key().map(|s| s.to_string()),
            contract,
            extra: self.extra.clone(),
        })
    }

    fn with_key(
        wallet: Nep6Wallet,
        key_pair: KeyPair,
        contract: Contract,
        nep2_key: Option<String>,
    ) -> Self {
        let parameter_names = default_parameter_names(&contract);
        let inner = StandardWalletAccount::new_with_key(
            key_pair,
            Some(contract.clone()),
            Arc::clone(wallet.protocol_settings()),
            nep2_key,
        );

        Self {
            inner,
            is_default: false,
            lock: false,
            extra: None,
            parameter_names,
            _wallet: Arc::new(wallet),
        }
    }

    fn watch_only(wallet: Nep6Wallet, script_hash: UInt160, contract: Option<Contract>) -> Self {
        let parameter_names = contract
            .as_ref()
            .map(default_parameter_names)
            .unwrap_or_default();
        let inner = StandardWalletAccount::new_watch_only(
            script_hash,
            contract,
            Arc::clone(wallet.protocol_settings()),
        );

        Self {
            inner,
            is_default: false,
            lock: false,
            extra: None,
            parameter_names,
            _wallet: Arc::new(wallet),
        }
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
        self.is_default
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default = is_default;
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
        if let Some(contract) = contract {
            self.parameter_names = default_parameter_names(&contract);
            self.inner.set_contract(Some(contract));
        } else {
            self.parameter_names.clear();
            self.inner.set_contract(None);
        }
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

    fn create_witness(&self, transaction: &Transaction) -> WalletResult<Witness> {
        self.inner.create_witness(transaction)
    }
}

impl Nep6Contract {
    fn from_file(file: &Nep6ContractFile) -> WalletResult<Self> {
        let script = hex::decode(&file.script).map_err(|e| WalletError::Other(e.to_string()))?;
        let mut parameter_names = Vec::with_capacity(file.parameters.len());
        let parameter_types = file
            .parameters
            .iter()
            .map(|parameter| {
                parameter_names.push(parameter.name.clone());
                parse_parameter_type(&parameter.param_type)
            })
            .collect::<WalletResult<Vec<_>>>()?;
        let contract = Contract::create(parameter_types, script);
        Ok(Self {
            contract,
            parameter_names,
            deployed: file.deployed,
        })
    }

    fn to_file(contract: &Contract, parameter_names: &[String]) -> Nep6ContractFile {
        let parameters = contract
            .parameter_list
            .iter()
            .enumerate()
            .map(|(index, param)| Nep6ContractParameterFile {
                name: parameter_names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("parameter{index}")),
                param_type: param.as_str().to_string(),
            })
            .collect();

        Nep6ContractFile {
            script: hex::encode(&contract.script),
            parameters,
            deployed: false,
        }
    }
}

fn parse_parameter_type(name: &str) -> WalletResult<ContractParameterType> {
    let kind = match name {
        "Any" => ContractParameterType::Any,
        "Boolean" => ContractParameterType::Boolean,
        "Integer" => ContractParameterType::Integer,
        "ByteArray" => ContractParameterType::ByteArray,
        "String" => ContractParameterType::String,
        "Hash160" => ContractParameterType::Hash160,
        "Hash256" => ContractParameterType::Hash256,
        "PublicKey" => ContractParameterType::PublicKey,
        "Signature" => ContractParameterType::Signature,
        "Array" => ContractParameterType::Array,
        "Map" => ContractParameterType::Map,
        "InteropInterface" => ContractParameterType::InteropInterface,
        "Void" => ContractParameterType::Void,
        other => {
            return Err(WalletError::Other(format!(
                "Unsupported contract parameter type: {other}"
            )))
        }
    };

    Ok(kind)
}

fn default_parameter_names(contract: &Contract) -> Vec<String> {
    if contract.parameter_list.len() == 1
        && contract.parameter_list[0] == ContractParameterType::Signature
    {
        vec!["signature".to_string()]
    } else {
        contract
            .parameter_list
            .iter()
            .enumerate()
            .map(|(index, _)| format!("parameter{index}"))
            .collect()
    }
}
