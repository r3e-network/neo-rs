// Copyright (C) 2015-2025 The Neo Project.
//
// account.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, ProtocolSettings, KeyPair};
use neo_core::wallets::{WalletAccount, BaseWalletAccount};

/// SQLite wallet account implementation.
/// Matches C# SQLiteWalletAccount class exactly
pub struct SQLiteWalletAccount {
    /// Base wallet account functionality
    base: BaseWalletAccount,
    
    /// The private key of the account
    /// Matches C# Key field
    pub key: Option<KeyPair>,
}

impl SQLiteWalletAccount {
    /// Creates a new SQLiteWalletAccount instance.
    /// Matches C# constructor exactly
    pub fn new(script_hash: UInt160, settings: &ProtocolSettings) -> Self {
        Self {
            base: BaseWalletAccount::new(script_hash, settings.clone()),
            key: None,
        }
    }
    
    /// Sets the private key.
    pub fn set_key(&mut self, key: Option<KeyPair>) {
        self.key = key;
    }
}

impl WalletAccount for SQLiteWalletAccount {
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
        self.key.is_some()
    }
    
    fn get_key(&self) -> Option<KeyPair> {
        self.key.clone()
    }
    
    fn protocol_settings(&self) -> &ProtocolSettings {
        self.base.protocol_settings()
    }
}

impl Clone for SQLiteWalletAccount {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            key: self.key.clone(),
        }
    }
}