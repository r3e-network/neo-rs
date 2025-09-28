// Copyright (C) 2015-2025 The Neo Project.
//
// wallet_account.rs file belongs to the neo project and is free
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

/// Represents an account in a wallet.
/// Matches C# WalletAccount abstract class exactly
pub trait WalletAccount {
    /// The hash of the account.
    /// Matches C# ScriptHash field
    fn script_hash(&self) -> UInt160;

    /// The label of the account.
    /// Matches C# Label property
    fn label(&self) -> &str;

    /// Sets the label of the account.
    /// Matches C# Label property setter
    fn set_label(&mut self, label: String);

    /// Indicates whether the account is the default account in the wallet.
    /// Matches C# IsDefault property
    fn is_default(&self) -> bool;

    /// Sets whether the account is the default account in the wallet.
    /// Matches C# IsDefault property setter
    fn set_is_default(&mut self, is_default: bool);

    /// Indicates whether the account is locked.
    /// Matches C# Lock property
    fn lock(&self) -> bool;

    /// Sets whether the account is locked.
    /// Matches C# Lock property setter
    fn set_lock(&mut self, lock: bool);

    /// The contract of the account.
    /// Matches C# Contract property
    fn contract(&self) -> Option<&Contract>;

    /// Sets the contract of the account.
    /// Matches C# Contract property setter
    fn set_contract(&mut self, contract: Option<Contract>);

    /// The address of the account.
    /// Matches C# Address property
    fn address(&self) -> String {
        Helper::to_address(
            &self.script_hash(),
            self.protocol_settings().address_version,
        )
    }

    /// Indicates whether the account contains a private key.
    /// Matches C# HasKey property
    fn has_key(&self) -> bool;

    /// Indicates whether the account is a watch-only account.
    /// Matches C# WatchOnly property
    fn watch_only(&self) -> bool {
        self.contract().is_none()
    }

    /// Gets the private key of the account.
    /// Matches C# GetKey method
    fn get_key(&self) -> Option<KeyPair>;

    /// Gets the protocol settings.
    /// Matches C# ProtocolSettings field
    fn protocol_settings(&self) -> &ProtocolSettings;
}

/// Base implementation for WalletAccount
pub struct BaseWalletAccount {
    /// The hash of the account.
    /// Matches C# ScriptHash field
    pub script_hash: UInt160,

    /// The label of the account.
    /// Matches C# Label field
    pub label: String,

    /// Indicates whether the account is the default account in the wallet.
    /// Matches C# IsDefault field
    pub is_default: bool,

    /// Indicates whether the account is locked.
    /// Matches C# Lock field
    pub lock: bool,

    /// The contract of the account.
    /// Matches C# Contract field
    pub contract: Option<Contract>,

    /// The protocol settings.
    /// Matches C# ProtocolSettings field
    pub protocol_settings: ProtocolSettings,
}

impl BaseWalletAccount {
    /// Initializes a new instance of the BaseWalletAccount class.
    /// Matches C# constructor exactly
    pub fn new(script_hash: UInt160, settings: ProtocolSettings) -> Self {
        BaseWalletAccount {
            script_hash,
            label: String::new(),
            is_default: false,
            lock: false,
            contract: None,
            protocol_settings: settings,
        }
    }
}

impl WalletAccount for BaseWalletAccount {
    fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn set_label(&mut self, label: String) {
        self.label = label;
    }

    fn is_default(&self) -> bool {
        self.is_default
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }

    fn lock(&self) -> bool {
        self.lock
    }

    fn set_lock(&mut self, lock: bool) {
        self.lock = lock;
    }

    fn contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }

    fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }

    fn has_key(&self) -> bool {
        // In a real implementation, this would check if the account has a private key
        false
    }

    fn get_key(&self) -> Option<KeyPair> {
        // In a real implementation, this would return the private key
        None
    }

    fn protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }
}
