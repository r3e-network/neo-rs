use std::sync::atomic::{AtomicPtr, Ordering};
use neo_json::jtoken::JToken;
use crate::wallet::key_pair::KeyPair;
use crate::wallet::nep6::{NEP6Contract, NEP6Wallet};
use crate::wallet::wallet::Wallet;

pub struct NEP6Account {
    wallet: NEP6Wallet,
    nep2key: Option<String>,
    nep2key_new: AtomicPtr<String>,
    key: Option<KeyPair>,
    pub extra: Option<JToken>,
}

impl WalletAccount for NEP6Account {
    fn is_decrypted(&self) -> bool {
        self.nep2key.is_none() || self.key.is_some()
    }

    fn has_key(&self) -> bool {
        self.nep2key.is_some()
    }

    fn get_key(&mut self) -> Option<&KeyPair> {
        if self.nep2key.is_none() {
            return None;
        }
        if self.key.is_none() {
            self.key = Some(self.wallet.decrypt_key(self.nep2key.as_ref().unwrap()));
        }
        self.key.as_ref()
    }
}

impl NEP6Account {
    pub fn new(wallet: NEP6Wallet, script_hash: H160, nep2key: Option<String>) -> Self {
        Self {
            wallet,
            nep2key,
            nep2key_new: AtomicPtr::new(std::ptr::null_mut()),
            key: None,
            extra: None,
        }
    }

    pub fn new_with_key(wallet: NEP6Wallet, script_hash: H160, key: KeyPair, password: &str) -> Self {
        let nep2key = key.export(
            password,
            wallet.protocol_settings().address_version,
            wallet.scrypt().n,
            wallet.scrypt().r,
            wallet.scrypt().p,
        );
        Self {
            wallet,
            nep2key: Some(nep2key),
            nep2key_new: AtomicPtr::new(std::ptr::null_mut()),
            key: Some(key),
            extra: None,
        }
    }


    pub fn get_key_with_password(&mut self, password: &str) -> Result<Option<&KeyPair>, Error> {
        if self.nep2key.is_none() {
            return Ok(None);
        }
        if self.key.is_none() {
            let private_key = Wallet::get_private_key_from_nep2(
                self.nep2key.as_ref().unwrap(),
                password,
                self.wallet.protocol_settings().address_version,
                self.wallet.scrypt().n,
                self.wallet.scrypt().r,
                self.wallet.scrypt().p,
            )?;
            self.key = Some(KeyPair::new(private_key));
        }
        Ok(self.key.as_ref())
    }



    pub fn verify_password(&self, password: &str) -> bool {
        Wallet::get_private_key_from_nep2(
            self.nep2key.as_ref().unwrap(),
            password,
            self.wallet.protocol_settings().address_version,
            self.wallet.scrypt().n,
            self.wallet.scrypt().r,
            self.wallet.scrypt().p,
        ).is_ok()
    }

    pub fn change_password_prepare(&mut self, password_old: &str, password_new: &str) -> bool {
        if self.is_watch_only() {
            return true;
        }
        
        let key_template = if let Some(nep2key) = &self.nep2key {
            match Wallet::get_private_key_from_nep2(
                nep2key,
                password_old,
                self.wallet.protocol_settings().address_version,
                self.wallet.scrypt().n,
                self.wallet.scrypt().r,
                self.wallet.scrypt().p,
            ) {
                Ok(private_key) => Some(KeyPair::new(private_key)),
                Err(_) => return false,
            }
        } else {
            self.key.clone()
        };

        if let Some(key) = key_template {
            let new_nep2key = key.export(
                password_new,
                self.wallet.protocol_settings().address_version,
                self.wallet.scrypt().n,
                self.wallet.scrypt().r,
                self.wallet.scrypt().p,
            );
            let boxed_new_key = Box::new(new_nep2key);
            self.nep2key_new.store(Box::into_raw(boxed_new_key), Ordering::SeqCst);
        }
        
        true
    }

    pub fn change_password_commit(&mut self) {
        let ptr = self.nep2key_new.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !ptr.is_null() {
            let boxed_new_key = unsafe { Box::from_raw(ptr) };
            self.nep2key = Some(*boxed_new_key);
        }
    }

    pub fn change_password_rollback(&mut self) {
        let ptr = self.nep2key_new.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !ptr.is_null() {
            unsafe { Box::from_raw(ptr) };
        }
    }
}


impl JsonConvertibleTrait for NEP6Account {
    fn from_json(json: &JToken, wallet: NEP6Wallet) -> Result<Self, Error> {
        let script_hash = json["address"].as_str()
            .ok_or(Error::InvalidFormat("Missing address"))?
            .to_script_hash(wallet.protocol_settings().address_version)?;
        
        let mut account = Self::new(wallet, script_hash, json["key"].as_str().map(String::from));
        
        account.label = json["label"].as_str().map(String::from);
        account.is_default = json["isDefault"].as_bool().unwrap_or(false);
        account.lock = json["lock"].as_bool().unwrap_or(false);
        account.contract = json["contract"].as_object().map(NEP6Contract::from_json);
        account.extra = json["extra"].clone();
        
        Ok(account)
    }


     fn to_json(&self) -> serde_json::Value {
        let mut account = JToken::new_object();
        account["address"] = JToken::from(self.script_hash.to_address(self.wallet.protocol_settings().address_version));
        account["label"] = JToken::from(self.label.clone());
        account["isDefault"] = JToken::from(self.is_default);
        account["lock"] = JToken::from(self.lock);
        account["key"] = JToken::from(self.nep2key.clone());
        if let Some(contract) = &self.contract {
            account["contract"] = contract.to_json();
        }
        account["extra"] = self.extra.clone().unwrap_or_default();
        account
    }
}