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

use crate::sqlite_wallet::SQLiteWallet;
use neo_core::wallets::{IWalletFactory, Wallet};
use neo_core::ProtocolSettings;
use std::path::Path;

/// SQLite wallet factory implementation (stubbed).
pub struct SQLiteWalletFactory;

impl SQLiteWalletFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SQLiteWalletFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl IWalletFactory for SQLiteWalletFactory {
    fn handle(&self, path: &str) -> bool {
        Path::new(path)
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("db3"))
            .unwrap_or(false)
    }

    fn create_wallet(
        &self,
        _name: &str,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String> {
        SQLiteWallet::create(path, password, settings)
            .map(|wallet| Box::new(wallet) as Box<dyn Wallet>)
    }

    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String> {
        SQLiteWallet::open(path, password, settings)
            .map(|wallet| Box::new(wallet) as Box<dyn Wallet>)
    }
}
