//! SQLite wallet loader for DB3 migration.
//!
//! Implements the C# `SQLiteWallet` surface sufficient to open existing `.db3`
//! files, validate the password, recover NEP-2 keys, and expose accounts for
//! migration to NEP-6. Contracts are reconstructed from stored scripts when
//! present, ensuring contract hashes remain consistent during migration.

use aes::Aes256;
use async_trait::async_trait;
use bs58;
use cipher::{block_padding::NoPadding, BlockDecrypt, BlockDecryptMut, KeyInit, KeyIvInit};
use neo_core::{
    cryptography::crypto_utils::NeoHash,
    io::BinaryReader,
    neo_config::MAX_SCRIPT_SIZE,
    protocol_settings::ProtocolSettings,
    smart_contract::{contract::Contract, contract_parameter_type::ContractParameterType},
    wallets::{
        wallet::WalletError, wallet::WalletResult, wallet_account::WalletAccount, Version, Wallet,
    },
    KeyPair, Transaction, UInt160, UInt256,
};
use rusqlite::Connection;
use scrypt::Params;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

#[derive(Clone)]
pub struct SQLiteWallet {
    path: String,
    _settings: Arc<ProtocolSettings>,
    version: Version,
    _master_key: Vec<u8>,
    salt: Vec<u8>,
    _scrypt_params: Params,
    password_hash: Vec<u8>,
    accounts: Vec<Arc<SQLiteWalletAccount>>,
}

#[derive(Clone)]
struct SQLiteWalletAccount {
    script_hash: UInt160,
    key: Option<KeyPair>,
    contract: Option<Contract>,
    protocol_settings: Arc<ProtocolSettings>,
}

impl SQLiteWalletAccount {
    fn new(
        script_hash: UInt160,
        key: Option<KeyPair>,
        contract: Option<Contract>,
        settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            script_hash,
            key,
            contract,
            protocol_settings: settings,
        }
    }
}

impl WalletAccount for SQLiteWalletAccount {
    fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    fn address(&self) -> String {
        neo_core::wallets::helper::Helper::to_address(
            &self.script_hash,
            self.protocol_settings.address_version,
        )
    }

    fn label(&self) -> Option<&str> {
        None
    }

    fn set_label(&mut self, _label: Option<String>) {}

    fn is_default(&self) -> bool {
        false
    }

    fn set_is_default(&mut self, _is_default: bool) {}

    fn is_locked(&self) -> bool {
        false
    }

    fn has_key(&self) -> bool {
        self.key.is_some()
    }

    fn get_key(&self) -> Option<KeyPair> {
        self.key.clone()
    }

    fn contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }

    fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn unlock(&mut self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn lock(&mut self) {}

    fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn export_wif(&self) -> WalletResult<String> {
        Err(WalletError::Other(
            "export_wif is not supported for SQLite wallets".to_string(),
        ))
    }

    fn export_nep2(&self, _password: &str) -> WalletResult<String> {
        Err(WalletError::Other(
            "export_nep2 is not supported for SQLite wallets".to_string(),
        ))
    }

    fn create_witness(
        &self,
        _transaction: &Transaction,
    ) -> WalletResult<neo_core::network::p2p::payloads::witness::Witness> {
        Err(WalletError::Other(
            "create_witness is not supported for SQLite wallets".to_string(),
        ))
    }
}

impl SQLiteWallet {
    pub fn create(
        path: &str,
        _password: &str,
        _settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        Err(format!(
            "SQLite wallets cannot be created by neo-rs yet (requested path '{}')",
            path
        ))
    }

    pub fn open(path: &str, password: &str, settings: &ProtocolSettings) -> Result<Self, String> {
        if !Path::new(path).exists() {
            return Err(format!("Wallet file '{}' not found", path));
        }
        let conn = Connection::open(path).map_err(|err| err.to_string())?;
        let keys = Self::load_keys(&conn)?;

        let salt = keys
            .get("Salt")
            .ok_or_else(|| "Salt was not found".to_string())?
            .clone();
        let password_hash = keys
            .get("PasswordHash")
            .ok_or_else(|| "PasswordHash was not found".to_string())?;
        let iv = keys
            .get("IV")
            .ok_or_else(|| "IV was not found".to_string())?;
        let scrypt_n = Self::read_int(&keys, "ScryptN", 16384)?;
        let scrypt_r = Self::read_int(&keys, "ScryptR", 8)?;
        let scrypt_p = Self::read_int(&keys, "ScryptP", 8)?;
        let log_n = (32 - scrypt_n.leading_zeros() - 1) as u8;
        let scrypt_params =
            Params::new(log_n, scrypt_r, scrypt_p, 64).map_err(|err| err.to_string())?;

        let password_key = Self::to_aes_key(password);
        let expected = Sha256::digest([password_key.as_slice(), salt.as_slice()].concat());
        if expected.as_slice() != password_hash.as_slice() {
            return Err("Invalid password".to_string());
        }

        let master_key_encrypted = keys
            .get("MasterKey")
            .ok_or_else(|| "MasterKey was not found".to_string())?;
        let master_key = Self::decrypt_master_key(master_key_encrypted, &password_key, iv)?;

        let addresses = Self::load_addresses(&conn)?;
        let mut accounts = Vec::new();
        let contract_rows = Self::load_contracts(&conn)?;

        for contract_row in contract_rows {
            let key_pair = if let Some(nep2) = contract_row.nep2_key {
                Some(Self::private_key_from_nep2(
                    &nep2,
                    &master_key,
                    settings.address_version,
                    &scrypt_params,
                )?)
            } else {
                None
            };

            let contract = if !contract_row.raw_data.is_empty() {
                if let Some(contract) = Self::parse_verification_contract(&contract_row.raw_data) {
                    Some(contract)
                } else if let Some(ref key_pair) = key_pair {
                    let point = key_pair
                        .get_public_key_point()
                        .map_err(|err| err.to_string())?;
                    Some(Contract::create_signature_contract(point))
                } else {
                    contract_row
                        .script_hash
                        .map(|hash| Contract::create_with_hash(hash, Vec::new()))
                }
            } else if let Some(ref key_pair) = key_pair {
                let point = key_pair
                    .get_public_key_point()
                    .map_err(|err| err.to_string())?;
                Some(Contract::create_signature_contract(point))
            } else {
                contract_row
                    .script_hash
                    .map(|hash| Contract::create_with_hash(hash, Vec::new()))
            };

            let script_hash = contract
                .as_ref()
                .map(|c| c.script_hash())
                .unwrap_or_else(|| {
                    key_pair
                        .as_ref()
                        .map(|kp| kp.get_script_hash())
                        .unwrap_or_else(UInt160::zero)
                });

            accounts.push(Arc::new(SQLiteWalletAccount::new(
                script_hash,
                key_pair,
                contract,
                Arc::new(settings.clone()),
            )));
        }

        for script_hash in addresses {
            if accounts.iter().any(|acct| acct.script_hash == script_hash) {
                continue;
            }
            accounts.push(Arc::new(SQLiteWalletAccount::new(
                script_hash,
                None,
                None,
                Arc::new(settings.clone()),
            )));
        }

        let version = keys
            .get("Version")
            .and_then(Self::read_version)
            .unwrap_or_default();

        Ok(Self {
            path: path.to_string(),
            _settings: Arc::new(settings.clone()),
            version,
            _master_key: master_key,
            salt,
            _scrypt_params: scrypt_params,
            password_hash: password_hash.clone(),
            accounts,
        })
    }

    fn load_keys(conn: &Connection) -> Result<HashMap<String, Vec<u8>>, String> {
        let mut stmt = conn
            .prepare("SELECT Name, Value FROM Key")
            .map_err(|err| err.to_string())?;
        let mut rows = stmt.query([]).map_err(|err| err.to_string())?;
        let mut map = HashMap::new();
        while let Some(row) = rows.next().map_err(|err| err.to_string())? {
            let name: String = row.get(0).map_err(|err| err.to_string())?;
            let value: Vec<u8> = row.get(1).map_err(|err| err.to_string())?;
            map.insert(name, value);
        }
        Ok(map)
    }

    fn load_addresses(conn: &Connection) -> Result<Vec<UInt160>, String> {
        let mut stmt = conn
            .prepare("SELECT ScriptHash FROM Address")
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let bytes: Vec<u8> = row.get(0)?;
                UInt160::from_bytes(&bytes).map_err(|_| rusqlite::Error::InvalidQuery)
            })
            .map_err(|err| err.to_string())?;
        let mut hashes = Vec::new();
        for hash in rows {
            hashes.push(hash.map_err(|err| err.to_string())?);
        }
        Ok(hashes)
    }

    fn load_contracts(conn: &Connection) -> Result<Vec<ContractRow>, String> {
        let mut stmt = conn.prepare(
            "SELECT c.ScriptHash, c.RawData, a.Nep2key FROM Contract c LEFT JOIN Account a ON c.PublicKeyHash = a.PublicKeyHash",
        ).map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let script_hash_bytes: Vec<u8> = row.get(0)?;
                let script_hash = UInt160::from_bytes(&script_hash_bytes)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let raw_data: Option<Vec<u8>> = row.get(1).ok();
                let nep2: Option<String> = row.get(2).ok();
                Ok(ContractRow {
                    script_hash: Some(script_hash),
                    raw_data: raw_data.unwrap_or_default(),
                    nep2_key: nep2,
                })
            })
            .map_err(|err| err.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|err| err.to_string())?);
        }
        Ok(result)
    }

    fn read_int(keys: &HashMap<String, Vec<u8>>, name: &str, default: u32) -> Result<u32, String> {
        keys.get(name)
            .map(|bytes| {
                if bytes.len() >= 4 {
                    let mut buf = [0u8; 4];
                    buf.copy_from_slice(&bytes[..4]);
                    Ok(u32::from_le_bytes(buf))
                } else {
                    Err(format!("{} was not found", name))
                }
            })
            .unwrap_or(Ok(default))
    }

    fn decrypt_master_key(
        encrypted: &[u8],
        password_key: &[u8],
        iv: &[u8],
    ) -> Result<Vec<u8>, String> {
        let cipher =
            Aes256CbcDec::new_from_slices(password_key, iv).map_err(|err| err.to_string())?;
        let mut buf = encrypted.to_vec();
        let decrypted = cipher
            .decrypt_padded_mut::<NoPadding>(&mut buf)
            .map_err(|err| err.to_string())?;
        Ok(decrypted.to_vec())
    }

    fn read_version(bytes: &Vec<u8>) -> Option<Version> {
        if bytes.len() < 16 {
            return None;
        }
        let major = i32::from_le_bytes(bytes[0..4].try_into().ok()?).max(0) as u32;
        let minor = i32::from_le_bytes(bytes[4..8].try_into().ok()?).max(0) as u32;
        let build = i32::from_le_bytes(bytes[8..12].try_into().ok()?).max(0) as u32;
        Some(Version::new(major, minor, build))
    }

    fn to_aes_key(password: &str) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let first = hasher.finalize_reset();
        hasher.update(first);
        hasher.finalize().to_vec()
    }

    fn private_key_from_nep2(
        nep2: &str,
        master_key: &[u8],
        address_version: u8,
        params: &Params,
    ) -> Result<KeyPair, String> {
        let decoded = bs58::decode(nep2)
            .into_vec()
            .map_err(|err| format!("invalid NEP2 key: {}", err))?;
        if decoded.len() != 43 || decoded[0] != 0x01 || decoded[1] != 0x42 || decoded[2] != 0xe0 {
            return Err("invalid NEP2 key".to_string());
        }
        let (payload, checksum) = decoded.split_at(39);
        let expected_checksum = NeoHash::hash256(payload);
        if expected_checksum[..4] != checksum[..4] {
            return Err("invalid NEP2 checksum".to_string());
        }
        let address_hash = &payload[3..7];
        let encrypted = &payload[7..39];

        let mut derived = vec![0u8; 64];
        scrypt::scrypt(master_key, address_hash, params, &mut derived)
            .map_err(|err| err.to_string())?;
        let (derived_half1, derived_half2) = derived.split_at(32);

        let cipher = Aes256::new_from_slice(derived_half2).map_err(|err| err.to_string())?;
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(encrypted);
        cipher.decrypt_block(&mut block);
        let mut private_key = Vec::with_capacity(32);
        for (a, b) in block.iter().zip(derived_half1.iter()) {
            private_key.push(a ^ b);
        }

        let key_pair = KeyPair::from_private_key(&private_key).map_err(|err| err.to_string())?;

        // Validate address hash
        let script_hash = key_pair.get_script_hash();
        let mut data = Vec::with_capacity(1 + script_hash.to_array().len() + 4);
        data.push(address_version);
        data.extend_from_slice(&script_hash.to_array());
        let checksum = NeoHash::hash256(&data);
        if &checksum[..4] != address_hash {
            return Err("address check failed for NEP2 key".to_string());
        }
        Ok(key_pair)
    }

    fn parse_verification_contract(raw: &[u8]) -> Option<Contract> {
        let mut cursor = Cursor::new(raw);
        let param_bytes = BinaryReader::read_var_bytes(&mut cursor, MAX_SCRIPT_SIZE).ok()?;
        let mut parameters = Vec::with_capacity(param_bytes.len());
        for byte in param_bytes {
            let param = match byte {
                0x00 => ContractParameterType::Any,
                0x10 => ContractParameterType::Boolean,
                0x11 => ContractParameterType::Integer,
                0x12 => ContractParameterType::ByteArray,
                0x13 => ContractParameterType::String,
                0x14 => ContractParameterType::Hash160,
                0x15 => ContractParameterType::Hash256,
                0x16 => ContractParameterType::PublicKey,
                0x17 => ContractParameterType::Signature,
                0x20 => ContractParameterType::Array,
                0x22 => ContractParameterType::Map,
                0x30 => ContractParameterType::InteropInterface,
                0xff => ContractParameterType::Void,
                _ => return None,
            };
            parameters.push(param);
        }
        let script =
            BinaryReader::read_var_bytes(&mut cursor, MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE).ok()?;
        Some(Contract::create(parameters, script))
    }
}

struct ContractRow {
    script_hash: Option<UInt160>,
    raw_data: Vec<u8>,
    nep2_key: Option<String>,
}

#[async_trait]
impl Wallet for SQLiteWallet {
    fn name(&self) -> &str {
        &self.path
    }

    fn path(&self) -> Option<&str> {
        Some(&self.path)
    }

    fn version(&self) -> &Version {
        &self.version
    }

    async fn change_password(
        &self,
        _old_password: &str,
        _new_password: &str,
    ) -> WalletResult<bool> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        self.accounts
            .iter()
            .any(|acct| &acct.script_hash == script_hash)
    }

    async fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        self.accounts
            .iter()
            .find(|acct| &acct.script_hash == script_hash)
            .cloned()
            .map(|acct| acct as Arc<dyn WalletAccount>)
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        self.accounts
            .iter()
            .cloned()
            .map(|acct| acct as Arc<dyn WalletAccount>)
            .collect()
    }

    async fn get_available_balance(&self, _asset_id: &UInt256) -> WalletResult<i64> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn import_nep2(
        &self,
        _nep2_key: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn sign(&self, _data: &[u8], _script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn sign_transaction(&self, _transaction: &mut Transaction) -> WalletResult<()> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }

    async fn unlock(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn lock(&self) {}

    async fn verify_password(&self, password: &str) -> WalletResult<bool> {
        let derived = Self::to_aes_key(password);
        let candidate = Sha256::digest([derived.as_slice(), self.salt.as_slice()].concat());
        Ok(candidate.as_slice() == self.password_hash.as_slice())
    }

    async fn save(&self) -> WalletResult<()> {
        Ok(())
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        self.accounts
            .first()
            .cloned()
            .map(|acct| acct as Arc<dyn WalletAccount>)
    }

    async fn set_default_account(&self, _script_hash: &UInt160) -> WalletResult<()> {
        Err(WalletError::Other(
            "SQLite wallets are read-only".to_string(),
        ))
    }
}
