// Copyright (C) 2015-2025 The Neo Project.
//
// i_wallet_changed_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::wallets::Wallet;

/// Wallet changed handler interface matching C# IWalletChangedHandler exactly
pub trait IWalletChangedHandler {
    /// The handler of WalletChanged event from the IWalletProvider.
    /// Triggered when a new wallet is assigned to the node.
    /// Matches C# IWalletProvider_WalletChanged_Handler method
    fn i_wallet_provider_wallet_changed_handler(&self, sender: &dyn std::any::Any, wallet: &Wallet);
}
