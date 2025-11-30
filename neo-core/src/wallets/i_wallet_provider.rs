// Copyright (C) 2015-2025 The Neo Project.
//
// i_wallet_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::wallets::Wallet;

use std::any::Any;
use std::sync::{mpsc, Arc};

/// A provider for obtaining wallet instance.
/// Matches C# IWalletProvider exactly
pub trait IWalletProvider: Send + Sync + Any {
    /// Returns a type-erased view of the provider for event dispatch.
    fn as_any(&self) -> &dyn Any;

    /// Triggered when a wallet is opened or closed.
    /// Matches C# WalletChanged event
    fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<dyn Wallet>>>;

    /// Get the currently opened Wallet instance.
    /// Matches C# GetWallet method
    fn get_wallet(&self) -> Option<Arc<dyn Wallet>>;
}
