// Copyright (C) 2015-2025 The Neo Project.
//
// i_wallet_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{protocol_settings::ProtocolSettings, wallets::Wallet};

/// Wallet factory interface matching C# IWalletFactory exactly
pub trait IWalletFactory {
    /// Determines whether the factory can handle the specified path.
    /// Matches C# Handle method
    fn handle(&self, path: &str) -> bool;

    /// Creates a new wallet.
    /// Matches C# CreateWallet method
    fn create_wallet(
        &self,
        name: &str,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String>;

    /// Opens a wallet.
    /// Matches C# OpenWallet method
    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String>;
}
