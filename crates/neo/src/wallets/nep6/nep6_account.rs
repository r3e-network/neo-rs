// Copyright (C) 2015-2025 The Neo Project.
//
// nep6_account.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    protocol_settings::ProtocolSettings,
    smart_contract::contract::Contract,
    uint160::UInt160,
    wallets::{helper::Helper, key_pair::KeyPair},
};

use super::super::{WalletAccount, BaseWalletAccount};
use super::{NEP6Wallet, NEP6Contract};
use std::sync::Arc;

/// NEP6 account implementation.
/// Matches C# NEP6Account class exactly
pub struct NEP6Account {
    /// Base wallet account functionality
    base: BaseWalletAccount,
    
    /// Reference to the NEP6 wallet
    wallet: Arc<NEP6Wallet>,
    
    /// NEP-2 encrypted private key
    nep2_key: Option<String>,
    
    /// New NEP-2 key during password change
    nep2_key_new: Option<String>,
    
    /// Decrypted private key
    key: Option<KeyPair>,
    
    /// Extra data
    extra: Option<serde_json::Value>,
}

impl NEP6Account {
    /// Creates a new NEP6Account instance.
    /// Matches C# constructor with nep2key parameter
    pub fn new(wallet: Arc<NEP6Wallet>, script_hash: UInt160, nep2_key: Option<String>) -> Self {
        let base = BaseWalletAccount::new(script_hash, wallet.protocol_settings().clone());
        
        Self {
            base,
            wallet,
            nep2_key,
            nep2_key_new: None,
            key: None,
            extra: None,
        }
    }
    
    /// Creates a new NEP6Account instance with key and password.
    /// Matches C# constructor with key and password parameters
    pub fn new_with_key(wallet: Arc<NEP6Wallet>, script_hash: UInt160, key: KeyPair, password: &str) -> Result<Self, String> {
        let nep2_key = key.export_with_passphrase(
            password,
            wallet.protocol_settings().address_version,
            wallet.scrypt().n,
            wallet.scrypt().r,
            wallet.scrypt().p,
        )?;
        
        Ok(Self {
            base: BaseWalletAccount::new(script_hash, wallet.protocol_settings().clone()),
            wallet,
            nep2_key: Some(nep2_key),
            nep2_key_new: None,
            key: Some(key),
            extra: None,
        })
    }
    
    /// Creates an NEP6Account from JSON.
    /// Matches C# FromJson method
    pub fn from_json(json: &serde_json::Value, wallet: Arc<NEP6Wallet>) -> Result<Self, String> {
        let address = json["address"].as_str().ok_or("Missing address field")?;
        let script_hash = Helper::to_script_hash(address, wallet.protocol_settings().address_version)?;
        let nep2_key = json["key"].as_str().map(|s| s.to_string());
        
        let mut account = Self::new(wallet, script_hash, nep2_key);
        
        if let Some(label) = json["label"].as_str() {
            account.base.set_label(label.to_string());
        }
        
        if let Some(is_default) = json["isDefault"].as_bool() {
            account.base.set_is_default(is_default);
        }
        
        if let Some(lock) = json["lock"].as_bool() {
            account.base.set_lock(lock);
        }
        
        if let Some(contract_json) = json["contract"].as_object() {
            account.base.set_contract(Some(NEP6Contract::from_json(contract_json)?));
        }
        
        account.extra = json["extra"].clone();
        
        Ok(account)
    }
    
    /// Gets the decrypted private key.
    /// Matches C# GetKey method
    pub fn get_key(&mut self) -> Option<&KeyPair> {
        if self.nep2_key.is_none() {
            return None;
        }
        
        if self.key.is_none() {
            if let Some(nep2_key) = &self.nep2_key {
                self.key = self.wallet.decrypt_key(nep2_key).ok();
            }
        }
        
        self.key.as_ref()
    }
    
    /// Gets the decrypted private key with password.
    /// Matches C# GetKey(string password) method
    pub fn get_key_with_password(&mut self, password: &str) -> Option<&KeyPair> {
        if self.nep2_key.is_none() {
            return None;
        }
        
        if self.key.is_none() {
            if let Some(nep2_key) = &self.nep2_key {
                self.key = self.wallet.get_private_key_from_nep2(
                    nep2_key,
                    password,
                    self.wallet.protocol_settings().address_version,
                    self.wallet.scrypt().n,
                    self.wallet.scrypt().r,
                    self.wallet.scrypt().p,
                ).ok().map(KeyPair::new).transpose().ok().flatten();
            }
        }
        
        self.key.as_ref()
    }
    
    /// Converts the account to JSON.
    /// Matches C# ToJson method
    pub fn to_json(&self) -> serde_json::Value {
        let mut account = serde_json::Map::new();
        
        account.insert("address".to_string(), serde_json::Value::String(
            Helper::to_address(&self.base.script_hash(), self.wallet.protocol_settings().address_version)
        ));
        
        account.insert("label".to_string(), serde_json::Value::String(self.base.label().to_string()));
        account.insert("isDefault".to_string(), serde_json::Value::Bool(self.base.is_default()));
        account.insert("lock".to_string(), serde_json::Value::Bool(self.base.lock()));
        
        if let Some(nep2_key) = &self.nep2_key {
            account.insert("key".to_string(), serde_json::Value::String(nep2_key.clone()));
        }
        
        if let Some(contract) = self.base.contract() {
            if let Some(nep6_contract) = contract.as_any().downcast_ref::<NEP6Contract>() {
                account.insert("contract".to_string(), nep6_contract.to_json());
            }
        }
        
        if let Some(extra) = &self.extra {
            account.insert("extra".to_string(), extra.clone());
        }
        
        serde_json::Value::Object(account)
    }
    
    /// Verifies the password.
    /// Matches C# VerifyPassword method
    pub fn verify_password(&self, password: &str) -> bool {
        if let Some(nep2_key) = &self.nep2_key {
            self.wallet.get_private_key_from_nep2(
                nep2_key,
                password,
                self.wallet.protocol_settings().address_version,
                self.wallet.scrypt().n,
                self.wallet.scrypt().r,
                self.wallet.scrypt().p,
            ).is_ok()
        } else {
            false
        }
    }
    
    /// Prepares for password change.
    /// Matches C# ChangePasswordPrepare method
    pub fn change_password_prepare(&mut self, password_old: &str, password_new: &str) -> bool {
        if self.base.watch_only() {
            return true;
        }
        
        let key_template = if self.nep2_key.is_none() {
            if self.key.is_none() {
                return true;
            }
            self.key.clone()
        } else {
            if let Some(nep2_key) = &self.nep2_key {
                self.wallet.get_private_key_from_nep2(
                    nep2_key,
                    password_old,
                    self.wallet.protocol_settings().address_version,
                    self.wallet.scrypt().n,
                    self.wallet.scrypt().r,
                    self.wallet.scrypt().p,
                ).ok().map(KeyPair::new).transpose().ok().flatten()
            } else {
                None
            }
        };
        
        if let Some(key_template) = key_template {
            self.nep2_key_new = key_template.export_with_passphrase(
                password_new,
                self.wallet.protocol_settings().address_version,
                self.wallet.scrypt().n,
                self.wallet.scrypt().r,
                self.wallet.scrypt().p,
            ).ok();
            true
        } else {
            false
        }
    }
    
    /// Commits the password change.
    /// Matches C# ChangePasswordCommit method
    pub fn change_password_commit(&mut self) {
        if let Some(nep2_key_new) = self.nep2_key_new.take() {
            self.nep2_key = Some(nep2_key_new);
        }
    }
    
    /// Rolls back the password change.
    /// Matches C# ChangePasswordRollback method
    pub fn change_password_rollback(&mut self) {
        self.nep2_key_new = None;
    }
    
    /// Gets the extra data.
    pub fn extra(&self) -> Option<&serde_json::Value> {
        self.extra.as_ref()
    }
    
    /// Sets the extra data.
    pub fn set_extra(&mut self, extra: Option<serde_json::Value>) {
        self.extra = extra;
    }
}

impl WalletAccount for NEP6Account {
    fn script_hash(&self) -> UInt160 {
        self.base.script_hash()
    }
    
    fn label(&self) -> &str {
        self.base.label()
    }
    
    fn set_label(&mut self, label: String) {
        self.base.set_label(label);
    }
    
    fn is_default(&self) -> bool {
        self.base.is_default()
    }
    
    fn set_is_default(&mut self, is_default: bool) {
        self.base.set_is_default(is_default);
    }
    
    fn lock(&self) -> bool {
        self.base.lock()
    }
    
    fn set_lock(&mut self, lock: bool) {
        self.base.set_lock(lock);
    }
    
    fn contract(&self) -> Option<&Contract> {
        self.base.contract()
    }
    
    fn set_contract(&mut self, contract: Option<Contract>) {
        self.base.set_contract(contract);
    }
    
    fn has_key(&self) -> bool {
        self.nep2_key.is_some()
    }
    
    fn get_key(&self) -> Option<KeyPair> {
        // This method should not be used directly, use get_key() instead
        None
    }
    
    fn protocol_settings(&self) -> &ProtocolSettings {
        self.base.protocol_settings()
    }
}

/// Extension trait for Contract to support downcasting
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl AsAny for Contract {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}