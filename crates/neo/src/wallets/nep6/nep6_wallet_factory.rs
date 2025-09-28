// Copyright (C) 2015-2025 The Neo Project.
//
// nep6_wallet_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{protocol_settings::ProtocolSettings, wallets::Wallet};

use super::super::{IWalletFactory, WalletError};
use super::NEP6Wallet;
use std::path::Path;
use std::sync::Arc;

/// NEP6 wallet factory implementation.
/// Matches C# NEP6WalletFactory class exactly
pub struct NEP6WalletFactory;

impl NEP6WalletFactory {
    /// The singleton instance of the NEP6WalletFactory.
    /// Matches C# Instance property
    pub fn instance() -> &'static Self {
        static INSTANCE: NEP6WalletFactory = NEP6WalletFactory;
        &INSTANCE
    }
}

impl IWalletFactory for NEP6WalletFactory {
    /// Determines whether the factory can handle the specified path.
    /// Matches C# Handle method
    fn handle(&self, path: &str) -> bool {
        if let Some(extension) = Path::new(path).extension() {
            extension.to_string_lossy().to_lowercase() == "json"
        } else {
            false
        }
    }
    
    /// Creates a new wallet.
    /// Matches C# CreateWallet method
    fn create_wallet(&self, name: &str, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, String> {
        if Path::new(path).exists() {
            return Err("The wallet file already exists.".to_string());
        }
        
        let wallet = NEP6Wallet::new(path, password, settings, Some(name.to_string()))?;
        wallet.save()?;
        Ok(Box::new(wallet))
    }
    
    /// Opens a wallet.
    /// Matches C# OpenWallet method
    fn open_wallet(&self, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, String> {
        let wallet = NEP6Wallet::new(path, password, settings, None)?;
        Ok(Box::new(wallet))
    }
}

impl Default for NEP6WalletFactory {
    fn default() -> Self {
        Self
    }
}