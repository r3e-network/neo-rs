// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Asset descriptor — mirrors C# `Neo.Wallets.AssetDescriptor`.
//!
//! Reads the human-facing metadata (`name`, `symbol`, `decimals`) of a NEP-17
//! asset by looking up its deployed `ContractState` and probing the
//! `decimals` / `symbol` methods through a read-only metadata reader.
//! Used by the wallet/RPC layer to render transfer amounts.
//!
//! This is a faithful port of `Neo.Wallets.AssetDescriptor`
//! (`src/Neo/Wallets/AssetDescriptor.cs`):
//!
//! 1. `ContractManagement.GetContract(snapshot, assetId)` — absent ⇒ error.
//! 2. Read `symbol` and `decimals` via [`Nep17MetadataReader`] (which runs
//!    a read-only contract call in the execution layer).
//! 3. `AssetName` is the contract manifest name.
//!
//! The VM execution that was previously inlined here (building a script,
//! running `ApplicationEngine`, checking `VmState::HALT`) has been extracted
//! into the [`Nep17MetadataReader`] trait (defined in `neo-runtime`, implemented
//! in `neo-execution`). This removes the direct `neo-wallets → neo-execution`
//! dependency: the wallet layer now depends on the trait, not the engine.

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::ContractManagement;
use neo_primitives::UInt160;
use neo_runtime::Nep17MetadataReader;
use neo_storage::{CacheRead, DataCache};

/// Describes a NEP-17 asset: its id, human-readable name, ticker symbol, and
/// decimal precision. Mirrors C# `Neo.Wallets.AssetDescriptor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDescriptor {
    /// The script hash of the asset contract.
    pub asset_id: UInt160,
    /// The contract manifest name (e.g. `"GasToken"`).
    pub asset_name: String,
    /// The asset's ticker symbol (e.g. `"GAS"`).
    pub symbol: String,
    /// The number of decimal places used by the asset.
    pub decimals: u8,
}

impl AssetDescriptor {
    /// Builds the descriptor for `asset_id` against `snapshot`, using
    /// `reader` to obtain the `symbol` and `decimals` from the contract.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::invalid_argument`] when no contract is deployed at
    /// `asset_id`, or when the read-only `decimals` / `symbol` probe fails —
    /// matching the two `ArgumentException` cases C# throws. Returns
    /// [`CoreError::invalid_format`] when the reported `symbol`/`decimals` are
    /// not a valid UTF-8 string / in-range byte.
    pub fn new<B, R>(snapshot: Arc<DataCache<B>>, reader: &R, asset_id: UInt160) -> CoreResult<Self>
    where
        B: CacheRead,
        R: Nep17MetadataReader + ?Sized,
    {
        let contract = ContractManagement::get_contract_from_snapshot(&snapshot, &asset_id)?
            .ok_or_else(|| {
                CoreError::invalid_argument(format!(
                    "No asset contract found for assetId {asset_id}. Please ensure the assetId \
                     is correct and the asset is deployed on the blockchain."
                ))
            })?;

        let metadata = reader
            .read_metadata(asset_id)
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?;

        Ok(Self {
            asset_id,
            asset_name: contract.manifest.name,
            symbol: metadata.symbol,
            decimals: metadata.decimals,
        })
    }
}

impl std::fmt::Display for AssetDescriptor {
    /// Matches C# `AssetDescriptor.ToString()`, which returns `AssetName`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.asset_name)
    }
}

#[cfg(test)]
#[path = "../tests/assets/asset_descriptor.rs"]
mod tests;
