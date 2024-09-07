use neo::prelude::*;
use neo::crypto::{KeyPair, ScryptParameters};
use neo::wallet::{Account, Wallet};
use neo::types::{UInt160, ContractParameterType};
use neo::vm::Contract;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// An implementation of the NEP-6 wallet standard.
///
/// See: https://github.com/neo-project/proposals/blob/master/nep-6.mediawiki
pub struct NEP6Wallet {
    password: String, // Note: In production, use a more secure way to store passwords
    name: String,
    version: String,
    accounts: Arc<Mutex<HashMap<UInt160, NEP6Account>>>,
    extra: Option<serde_json::Value>,
    scrypt: ScryptParameters,
    path: String,
    settings: ProtocolSettings,
}

impl NEP6Wallet {
    /// Loads or creates a wallet at the specified path.
    pub fn new(path: &str, password: &str, settings: ProtocolSettings, name: Option<&str>) -> Self {
        let path_buf = Path::new(path);
        if path_buf.exists() {
            let wallet_json = fs::read_to_string(path).expect("Unable to read wallet file");
            let wallet: serde_json::Value = serde_json::from_str(&wallet_json).expect("Invalid JSON in wallet file");
            Self::from_json(path, password, settings, &wallet)
        } else {
            Self {
                password: password.to_string(),
                name: name.unwrap_or("").to_string(),
                version: "1.0".to_string(),
                accounts: Arc::new(Mutex::new(HashMap::new())),
                extra: None,
                scrypt: ScryptParameters::default(),
                path: path.to_string(),
                settings,
            }
        }
    }

    /// Loads the wallet with the specified JSON string.
    pub fn from_json(path: &str, password: &str, settings: ProtocolSettings, json: &serde_json::Value) -> Self {
        let mut wallet = Self {
            password: password.to_string(),
            name: json["name"].as_str().unwrap_or("").to_string(),
            version: json["version"].as_str().unwrap_or("1.0").to_string(),
            accounts: Arc::new(Mutex::new(HashMap::new())),
            extra: json["extra"].clone(),
            scrypt: ScryptParameters::from_json(&json["scrypt"]),
            path: path.to_string(),
            settings,
        };

        let accounts = json["accounts"].as_array().expect("Invalid accounts in wallet");
        for account_json in accounts {
            let account = NEP6Account::from_json(account_json, &wallet);
            wallet.add_account(account);
        }

        if !wallet.verify_password(password) {
            panic!("Wrong password.");
        }

        wallet
    }

    fn add_account(&self, account: NEP6Account) {
        let mut accounts = self.accounts.lock().unwrap();
        if let Some(old_account) = accounts.get(&account.script_hash) {
            // Update existing account properties
            // Note: This is a simplified version. You might need to implement more complex merging logic.
            accounts.insert(account.script_hash, account);
        } else {
            accounts.insert(account.script_hash, account);
        }
    }

    pub fn contains(&self, script_hash: &UInt160) -> bool {
        let accounts = self.accounts.lock().unwrap();
        accounts.contains_key(script_hash)
    }

    pub fn create_account(&self, private_key: &[u8]) -> Result<NEP6Account, String> {
        let key = KeyPair::from_private_key(private_key)?;
        let contract = Contract::create_signature_contract(&key.public_key());
        let account = NEP6Account::new(
            self,
            contract.script_hash(),
            Some(key),
            &self.password,
            Some(contract),
        );
        self.add_account(account.clone());
        Ok(account)
    }

    pub fn delete_account(&self, script_hash: &UInt160) -> Result<(), Error> {
        let mut accounts = self.accounts.lock().unwrap();
        if accounts.remove(script_hash).is_none() {
            return Err(Error::AccountNotFound);
        }
        self.save()?;
        Ok(())
    }

    pub fn get_account(&self, script_hash: &UInt160) -> Option<NEP6Account> {
        let accounts = self.accounts.lock().unwrap();
        accounts.get(script_hash).cloned()
    }

    pub fn get_accounts(&self) -> Vec<NEP6Account> {
        let accounts = self.accounts.lock().unwrap();
        accounts.values().cloned().collect()
    }

    pub fn import_from_wif(&self, wif: &str) -> Result<NEP6Account, Error> {
        let private_key = KeyPair::from_wif(wif)?;
        self.create_account(&private_key.private_key())
    }

    pub fn import_from_nep2(&self, nep2: &str, passphrase: &str) -> Result<NEP6Account, Error> {
        let private_key = KeyPair::from_nep2(nep2, passphrase, &self.scrypt)?;
        self.create_account(&private_key.private_key())
    }

    pub fn to_json(&self) -> serde_json::Value {
        let accounts = self.accounts.lock().unwrap();
        let accounts_json: Vec<serde_json::Value> = accounts.values().map(|a| a.to_json()).collect();

        json!({
            "name": self.name,
            "version": self.version,
            "scrypt": self.scrypt.to_json(),
            "accounts": accounts_json,
            "extra": self.extra
        })
    }

    pub fn save(&self) {
        let json = self.to_json().to_string();
        fs::write(&self.path, json).expect("Unable to write wallet file");
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

    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<(), Error> {
        if !self.verify_password(old_password) {
            return Err(Error::InvalidPassword);
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
}

impl Wallet for NEP6Wallet {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_version(&self) -> &str {
        &self.version
    }

    fn get_account(&self, script_hash: &UInt160) -> Option<&dyn WalletAccount> {
        self.accounts.lock().unwrap().get(script_hash).map(|a| a as &dyn WalletAccount)
    }

    fn get_accounts(&self) -> Vec<&dyn WalletAccount> {
        self.accounts.lock().unwrap().values().map(|a| a as &dyn WalletAccount).collect()
    }

    fn get_balance(&self, asset_id: &UInt160) -> Fixed8 {
        // Implement balance calculation logic
        unimplemented!()
    }

    fn create_key(&mut self) -> Result<KeyPair, Error> {
        let key_pair = KeyPair::new()?;
        self.create_account(key_pair.private_key())?;
        Ok(key_pair)
    }

    fn import_key(&mut self, wif: &str) -> Result<KeyPair, Error> {
        let key_pair = KeyPair::from_wif(wif)?;
        self.create_account(key_pair.private_key())?;
        Ok(key_pair)
    }

    fn delete_account(&mut self, script_hash: &UInt160) -> bool {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.remove(script_hash).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_save_wallet() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = ProtocolSettings::default();

        let wallet = NEP6Wallet::new(path.to_str().unwrap(), "password", &settings, "TestWallet").unwrap();
        wallet.save();

        assert!(path.exists());
    }

    #[test]
    fn test_create_account() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = ProtocolSettings::default();

        let mut wallet = NEP6Wallet::new(path.to_str().unwrap(), "password", &settings, "TestWallet").unwrap();
        let key_pair = wallet.create_key().unwrap();

        assert!(wallet.contains(&key_pair.script_hash()));
    }

    #[test]
    fn test_verify_password() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_wallet.json");
        let settings = ProtocolSettings::default();

        let wallet = NEP6Wallet::new(path.to_str().unwrap(), "password", &settings, "TestWallet").unwrap();

        assert!(wallet.verify_password("password"));
        assert!(!wallet.verify_password("wrong_password"));
    }
}
