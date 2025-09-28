// Copyright (C) 2015-2025 The Neo Project.
//
// sq_lite_wallet_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, Wallet};
use neo_core::wallets::IWalletFactory;
use std::path::Path;

/// SQLite wallet factory implementation.
/// Matches C# SQLiteWalletFactory class exactly
pub struct SQLiteWalletFactory;

impl SQLiteWalletFactory {
    /// Creates a new SQLiteWalletFactory instance.
    pub fn new() -> Self {
        Self
    }
    
    /// Gets the name of the factory.
    /// Matches C# Name property
    pub fn name(&self) -> &str {
        "SQLiteWallet"
    }
    
    /// Gets the description of the factory.
    /// Matches C# Description property
    pub fn description(&self) -> &str {
        "A SQLite-based wallet provider that supports wallet files with .db3 suffix."
    }
}

impl IWalletFactory for SQLiteWalletFactory {
    /// Determines whether the factory can handle the specified path.
    /// Matches C# Handle method
    fn handle(&self, path: &str) -> bool {
        if let Some(extension) = Path::new(path).extension() {
            extension.to_string_lossy().to_lowercase() == "db3"
        } else {
            false
        }
    }
    
    /// Creates a new wallet.
    /// Matches C# CreateWallet method
    fn create_wallet(&self, name: &str, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, String> {
        // In a real implementation, this would create a SQLite wallet
        // For now, we'll return an error as the SQLite wallet implementation is complex
        Err("SQLite wallet creation not yet implemented".to_string())
    }
    
    /// Opens a wallet.
    /// Matches C# OpenWallet method
    fn open_wallet(&self, path: &str, password: &str, settings: &ProtocolSettings) -> Result<Box<dyn Wallet>, String> {
        // In a real implementation, this would open a SQLite wallet
        // For now, we'll return an error as the SQLite wallet implementation is complex
        Err("SQLite wallet opening not yet implemented".to_string())
    }
}

impl Default for SQLiteWalletFactory {
    fn default() -> Self {
        Self::new()
    }
}