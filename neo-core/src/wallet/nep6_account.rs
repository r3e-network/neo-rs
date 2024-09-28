use alloc::rc::Rc;
use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};
use neo_json::jtoken::JToken;
use crate::wallet::key_pair::KeyPair;
use crate::wallet::nep6::{NEP6Contract, NEP6Wallet};
use crate::wallet::wallet::Wallet;
use crate::wallet::wallet_account::WalletAccountTrait;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;
use crate::contract::Contract;
use crate::wallet::NEP6Wallet;

pub struct NEP6Account {
    protocol_settings: ProtocolSettings,
    script_hash: H160,
    label: String,
    is_default: bool,
    lock: bool,
    contract: Option<Contract>,
    wallet: Option<Rc<RefCell<NEP6Wallet>>>,
    nep2key: Option<String>,
    nep2key_new: AtomicPtr<String>,
    key: Option<KeyPair>,
    pub extra: Option<JToken>,
}

impl WalletAccountTrait for NEP6Account {
    fn address(&self) -> String {
        self.script_hash.to_address(self.protocol_settings.address_version)
    }

    fn has_key(&self) -> bool {
        self.nep2key.is_some()
    }

    fn watch_only(&self) -> bool {
        self.contract.is_none()
    }

    fn get_key(&mut self) -> Option<KeyPair> {
        if self.nep2key.is_none() {
            return None;
        }
        if self.key.is_none() {
            // This should be implemented properly
            unimplemented!("get_key() must be implemented properly")
        }
        self.key.clone()
    }
    
    fn get_protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }
    
    fn set_protocol_settings(&mut self, settings: ProtocolSettings) {
        self.protocol_settings = settings;
    }
    
    fn get_script_hash(&self) -> &H160 {
        &self.script_hash
    }
    
    fn set_script_hash(&mut self, script_hash: H160) {
        self.script_hash = script_hash;
    }
    
    fn get_label(&self) -> &str {
        &self.label
    }
    
    fn set_label(&mut self, label: String) {
        self.label = label;
    }
    
    fn is_default(&self) -> bool {
        self.is_default
    }
    
    fn set_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }
    
    fn is_locked(&self) -> bool {
        self.lock
    }
    
    fn set_locked(&mut self, locked: bool) {
        self.lock = locked;
    }
    
    fn get_contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }
    
    fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }
}

impl NEP6Account {
    fn is_decrypted(&self) -> bool {
        self.nep2key.is_none() || self.key.is_some()
    }

    pub fn new(wallet: NEP6Wallet, script_hash: H160, nep2key: Option<String>) -> Self {
        Self {
            protocol_settings: wallet.protocol_settings().clone(),
            script_hash,
            label: String::new(),
            is_default: false,
            lock: false,
            contract: None,
            wallet: Some(Rc::new(RefCell::new(wallet))),
            nep2key,
            nep2key_new: AtomicPtr::new(std::ptr::null_mut()),
            key: None,
            extra: None,
        }
    }

    pub fn new_with_key(wallet: NEP6Wallet, script_hash: H160, key: KeyPair, password: &str) -> Self {
        let nep2key = key.export_nep2(
            password,
            wallet.protocol_settings().address_version,
            wallet.scrypt().n,
            wallet.scrypt().r,
            wallet.scrypt().p,
        );
        Self {
            protocol_settings: wallet.protocol_settings().clone(),
            script_hash,
            label: String::new(),
            is_default: false,
            lock: false,
            contract: None,
            wallet: Some(Rc::new(RefCell::new(wallet))),
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
            let wallet = self.wallet.as_ref().unwrap().borrow();
            let private_key = Wallet::get_private_key_from_nep2(
                self.nep2key.as_ref().unwrap(),
                password,
                wallet.protocol_settings().address_version,
                wallet.scrypt().n,
                wallet.scrypt().r,
                wallet.scrypt().p,
            )?;
            self.key = Some(KeyPair::new(private_key));
        }
        Ok(self.key.as_ref())
    }

    pub fn verify_password(&self, password: &str) -> bool {
        let wallet = self.wallet.as_ref().unwrap().borrow();
        Wallet::get_private_key_from_nep2(
            self.nep2key.as_ref().unwrap(),
            password,
            wallet.protocol_settings().address_version,
            wallet.scrypt().n,
            wallet.scrypt().r,
            wallet.scrypt().p,
        ).is_ok()
    }

    pub fn change_password_prepare(&mut self, password_old: &str, password_new: &str) -> bool {
        if self.watch_only() {
            return true;
        }
        
        let key_template = if let Some(nep2key) = &self.nep2key {
            let wallet = self.wallet.as_ref().unwrap().borrow();
            match Wallet::get_private_key_from_nep2(
                nep2key,
                password_old,
                wallet.protocol_settings().address_version,
                wallet.scrypt().n,
                wallet.scrypt().r,
                wallet.scrypt().p,
            ) {
                Ok(private_key) => Some(KeyPair::new(private_key)),
                Err(_) => return false,
            }
        } else {
            self.key.clone()
        };

        if let Some(key) = key_template {
            let wallet = self.wallet.as_ref().unwrap().borrow();
            let new_nep2key = key.export_nep2(
                password_new,
                wallet.protocol_settings().address_version,
                wallet.scrypt().n,
                wallet.scrypt().r,
                wallet.scrypt().p,
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
        
        account.label = json["label"].as_str().map(String::from).unwrap_or_default();
        account.is_default = json["isDefault"].as_bool().unwrap_or(false);
        account.lock = json["lock"].as_bool().unwrap_or(false);
        account.contract = json["contract"].as_object().map(NEP6Contract::from_json);
        account.extra = json["extra"].clone();
        
        Ok(account)
    }

    fn to_json(&self) -> serde_json::Value {
        let mut account = JToken::new_object();
        account["address"] = JToken::from(self.script_hash.to_address(self.protocol_settings.address_version));
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