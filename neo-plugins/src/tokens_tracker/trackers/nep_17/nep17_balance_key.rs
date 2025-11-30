// Copyright (C) 2015-2025 The Neo Project.
//
// nep17_balance_key.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{
    io::{BinaryWriter, ISerializable, MemoryReader},
    UInt160,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// NEP-17 balance key implementation.
/// Matches C# Nep17BalanceKey class exactly
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nep17BalanceKey {
    /// User script hash
    /// Matches C# UserScriptHash field
    pub user_script_hash: UInt160,

    /// Asset script hash
    /// Matches C# AssetScriptHash field
    pub asset_script_hash: UInt160,
}

impl Nep17BalanceKey {
    /// Creates a new Nep17BalanceKey instance.
    /// Matches C# constructor
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
        }
    }

    /// Gets the size of the serialized data.
    /// Matches C# Size property
    pub fn size(&self) -> usize {
        20 + 20 // UInt160 + UInt160
    }
}

impl Default for Nep17BalanceKey {
    fn default() -> Self {
        Self {
            user_script_hash: UInt160::zero(),
            asset_script_hash: UInt160::zero(),
        }
    }
}

// Use macro to reduce boilerplate for ordering implementation
neo_core::impl_ord_by_fields!(Nep17BalanceKey, user_script_hash, asset_script_hash);

impl ISerializable for Nep17BalanceKey {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write user script hash
        writer.write_all(&self.user_script_hash.to_bytes())?;

        // Write asset script hash
        writer.write_all(&self.asset_script_hash.to_bytes())?;

        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read user script hash
        let mut user_hash_bytes = [0u8; 20];
        reader.read_exact(&mut user_hash_bytes)?;
        self.user_script_hash = UInt160::from_bytes(&user_hash_bytes);

        // Read asset script hash
        let mut asset_hash_bytes = [0u8; 20];
        reader.read_exact(&mut asset_hash_bytes)?;
        self.asset_script_hash = UInt160::from_bytes(&asset_hash_bytes);

        Ok(())
    }
}
