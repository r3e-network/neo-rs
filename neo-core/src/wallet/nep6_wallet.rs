use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use neo_json::json_convert_trait::JsonConvertibleTrait;
use neo_type::H160;

use crate::contract::Contract;
use crate::neo_contract::contract_parameters_context::ContractParametersContext;
use crate::payload::Version;
use crate::protocol_settings::ProtocolSettings;
use crate::wallet::key_pair::KeyPair;
use crate::wallet::{NEP6Account, ScryptParameters, WalletError};
use crate::wallet::wallet::Wallet;

use serde::{Serialize, Deserialize};

/// An implementation of the NEP-6 wallet standard.
///
/// See: https://github.com/neo-project/proposals/blob/master/nep-6.mediawiki
use getset::{Getters, Setters};

#[derive(Serialize, Deserialize, Getters, Setters)]
pub struct NEP6Wallet {
    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    protocol_settings: Arc<ProtocolSettings>,

    #[getset(get = "pub", set = "pub")]
    name: String,

    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    path: PathBuf,

    #[getset(get = "pub", set = "pub")]
    version: Version,

    #[serde(skip)]
    #[getset(get = "pub", set = "pub")]
    password: String, // Note: In production, use a more secure way to store passwords

    #[getset(get = "pub", set = "pub")]
    accounts: Arc<Mutex<HashMap<H160, NEP6Account>>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub", set = "pub")]
    extra: Option<serde_json::Value>,

    #[getset(get = "pub", set = "pub")]
    scrypt: ScryptParameters,
}

impl NEP6Wallet {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("Failed to serialize NEP6Wallet")
    }

    pub fn from_json(
        path: &str,
        password: &str,
        settings: Arc<ProtocolSettings>,
        json: &str,
    ) -> Result<Self, serde_json::Error> {
        let mut wallet: NEP6Wallet = serde_json::from_str(json)?;
        wallet.protocol_settings = settings;
        wallet.path = PathBuf::from(path);
        wallet.password = password.to_string();

        if !wallet.verify_password(password) {
            panic!("Wrong password.");
        }

        Ok(wallet)
    }
}


impl NEP6Wallet {
    /// Loads or creates a wallet at the specified path.
    pub fn new(path: &str, password: &str, settings: Arc<ProtocolSettings>, name: Option<&str>) -> Self {
        let path_buf = Path::new(path);
        if path_buf.exists() {
            let wallet_json = fs::read_to_string(path).expect("Unable to read wallet file");
            let wallet: serde_json::Value =
                serde_json::from_str(&wallet_json).expect("Invalid JSON in wallet file");
            Self::from_json(path, password, settings, &wallet)
        } else {
            Self {
                protocol_settings: settings,
                name: name.unwrap_or("").to_string(),
                path: path_buf.to_path_buf(),
                version: Version::try_from("1.0".to_string()).unwrap(),
                password: password.to_string(),
                accounts: Arc::new(Mutex::new(HashMap::new())),
                extra: None,
                scrypt: ScryptParameters::default(),
            }
        }
    }

    fn add_account(&self, account: NEP6Account) {
        let mut accounts = self.accounts.lock().unwrap();
        if let Some(_old_account) = accounts.get(&account.get_script_hash()) {
            // Update existing account properties
            // Note: This is a simplified version. You might need to implement more complex merging logic.
            accounts.insert(account.get_script_hash(), account);
        } else {
            accounts.insert(account.get_script_hash(), account);
        }
    }

    pub fn contains(&self, script_hash: &H160) -> bool {
        let accounts = self.accounts.lock().unwrap();
        accounts.contains_key(script_hash)
    }

    pub fn create_account(&self, private_key: &[u8]) -> Result<NEP6Account, String> {
        let key = KeyPair::from_private_key(private_key)?;
        let contract = Contract::create_signature_contract(&key.public_key());
        let account = NEP6Account::new(
            contract.script_hash(),
            Some(key),
            &self.password,
        );
        self.add_account(account.clone());
        Ok(account)
    }

    pub fn delete_account(&self, script_hash: &H160) -> Result<(), WalletError> {
        let mut accounts = self.accounts.lock().unwrap();
        if accounts.remove(script_hash).is_none() {
            return Err(WalletError::AccountNotFound);
        }
        self.save()?;
        Ok(())
    }

    pub fn get_account(&self, script_hash: &H160) -> Option<NEP6Account> {
        let accounts = self.accounts.lock().unwrap();
        accounts.get(script_hash).cloned()
    }

    pub fn get_accounts(&self) -> Vec<NEP6Account> {
        let accounts = self.accounts.lock().unwrap();
        accounts.values().cloned().collect()
    }

    pub fn import_from_wif(&self, wif: &str) -> Result<NEP6Account, WalletError> {
        let private_key = KeyPair::from_wif(wif)?;
        self.create_account(&private_key.private_key()).map_err(|e| WalletError::Other(e))
    }

    pub fn import_from_nep2(&self, nep2: &str, passphrase: &str) -> Result<NEP6Account, WalletError> {
        let private_key = KeyPair::from_nep2(nep2, passphrase, &self.scrypt)?;
        self.create_account(&private_key.private_key()).map_err(|e| WalletError::Other(e))
    }

    pub fn save(&self) -> Result<(), WalletError> {
        let json = self.to_json().to_string();
        fs::write(&self.path, json).map_err(|_| WalletError::IOError)?;
        Ok(())
    }

    pub fn verify_password(&self, password: &str) -> bool {
        // Use a secure password verification method
        let decrypted_account = self.accounts.lock().unwrap().values().find(|a| a.has_key());
        if let Some(account) = decrypted_account {
            account.get_key().is_some()
        } else {
            false
        }
    }

    pub fn change_password(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), WalletError> {
        if !self.verify_password(old_password) {
            return Err(WalletError::InvalidPassword);
        }

        let mut accounts = self.accounts.lock().unwrap();
        for account in accounts.values_mut() {
            if let Some(key) = account.get_key() {
                let new_nep2key = key.export(
                    new_password,
                    self.protocol_settings.address_version,
                    self.scrypt.n,
                    self.scrypt.r,
                    self.scrypt.p,
                );
                account.set_nep2key(new_nep2key);
            }
        }

        self.password = new_password.to_string();
        self.save()?;
        Ok(())
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_version(&self) -> &Version {
        &self.version
    }

    pub fn create_key(&mut self) -> Result<KeyPair, WalletError> {
        let key_pair = KeyPair::new();
        self.create_account(&key_pair.private_key()).map_err(|e| WalletError::Other(e))?;
        Ok(key_pair)
    }

    pub fn import_key(&mut self, wif: &str) -> Result<KeyPair, WalletError> {
        let key_pair = KeyPair::from_wif(wif)?;
        self.create_account(&key_pair.private_key()).map_err(|e| WalletError::Other(e))?;
        Ok(key_pair)
    }

    pub fn create_contract_account(&mut self, contract: Contract, key: Option<KeyPair>) -> NEP6Account {
        let account = NEP6Account::new(
            contract.script_hash(),
            key,
            &self.password,
        );
        self.add_account(account.clone());
        account
    }

    pub fn create_watch_only_account(&mut self, script_hash: H160) -> NEP6Account {
        let account = NEP6Account::new(
            script_hash,
            None,
            &self.password,
        );
        self.add_account(account.clone());
        account
    }

    pub fn delete(&mut self) {
        // Implementation of delete
        unimplemented!()
    }

    pub fn make_transaction(&self, snapshot: &Snapshot, outputs: &[TransferOutput], from: Option<&H160>, cosigners: Option<&[Signer]>, persisting_block: Option<&Block>) -> Result<Transaction, String> {
        // Implementation of make_transaction
        unimplemented!()
    }

    pub fn make_transaction_with_script(&self, snapshot: &Snapshot, script: &[u8], sender: Option<&H160>, cosigners: Option<&[Signer]>, attributes: Option<&[TransactionAttribute]>, max_gas: i64, persisting_block: Option<&Block>) -> Result<Transaction, String> {
        // Implementation of make_transaction_with_script
        unimplemented!()
    }

    pub fn sign(&self, context: &mut ContractParametersContext) -> bool {
        // Implementation of sign
        unimplemented!()
    }

}



impl Wallet for NEP6Wallet {
    fn change_password(&mut self, old_password: &str, new_password: &str) -> bool {
        self.change_password(old_password, new_password).is_ok()
    }

    fn contains(&self, script_hash: &H160) -> bool {
        self.contains(script_hash)
    }

    fn create_account(&mut self, private_key: &[u8]) -> WalletAccount {
        self.create_account(private_key).unwrap()
    }

    fn create_contract_account(&mut self, contract: Contract, key: Option<KeyPair>) -> WalletAccount {
        self.create_contract_account(contract, key)
    }

    fn create_watch_only_account(&mut self, script_hash: H160) -> WalletAccount {
        self.create_watch_only_account(script_hash)
    }

    fn delete(&mut self) {
        self.delete()
    }

    fn delete_account(&mut self, script_hash: &H160) -> bool {
        self.delete_account(script_hash).is_ok()
    }

    fn get_account(&self, script_hash: &H160) -> Option<WalletAccount> {
        self.get_account(script_hash).map(|a| a.into())
    }

    fn get_accounts(&self) -> Vec<WalletAccount> {
        self.get_accounts().into_iter().map(|a| a.into()).collect()
    }

    fn create_account_random(&mut self) -> WalletAccount {
        self.create_key().unwrap().into()
    }

    fn create_contract_account_with_private_key(&mut self, contract: Contract, private_key: Option<&[u8]>) -> WalletAccount {
        match private_key {
            Some(key) => self.create_contract_account(contract, Some(KeyPair::from_private_key(key).unwrap())),
            None => self.create_contract_account(contract, None),
        }
    }

    fn make_transaction(&self, snapshot: &Snapshot, outputs: &[TransferOutput], from: Option<&H160>, cosigners: Option<&[Signer]>, persisting_block: Option<&Block>) -> Result<Transaction, String> {
        self.make_transaction(snapshot, outputs, from, cosigners, persisting_block)
    }

    fn make_transaction_with_script(&self, snapshot: &Snapshot, script: &[u8], sender: Option<&H160>, cosigners: Option<&[Signer]>, attributes: Option<&[TransactionAttribute]>, max_gas: i64, persisting_block: Option<&Block>) -> Result<Transaction, String> {
        self.make_transaction_with_script(snapshot, script, sender, cosigners, attributes, max_gas, persisting_block)
    }

    fn sign(&self, context: &mut ContractParametersContext) -> bool {
        self.sign(context)
    }

    fn verify_password(&self, password: &str) -> bool {
        self.verify_password(password)
    }

    fn save(&self) {
        self.save().unwrap()
    }
    
    type CreateError;
    
    fn create(name: &str, path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized {
        todo!()
    }
    
    fn open(path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized {
        todo!()
    }
    
    fn migrate(path: &str, old_path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_create_and_save_wallet() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = Arc::new(ProtocolSettings::default());

        let wallet =
            NEP6Wallet::new(path.to_str().unwrap(), "password", settings, Some("TestWallet"));
        wallet.save().unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_create_account() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = Arc::new(ProtocolSettings::default());

        let mut wallet =
            NEP6Wallet::new(path.to_str().unwrap(), "password", settings, Some("TestWallet"));
        let key_pair = wallet.create_key().unwrap();

        assert!(wallet.contains(&key_pair.script_hash()));
    }

    #[test]
    fn test_verify_password() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = Arc::new(ProtocolSettings::default());

        let wallet =
            NEP6Wallet::new(path.to_str().unwrap(), "password", settings, Some("TestWallet"));

        assert!(wallet.verify_password("password"));
        assert!(!wallet.verify_password("wrong_password"));
    }
}
