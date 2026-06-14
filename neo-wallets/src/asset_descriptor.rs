// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Asset descriptor — mirrors C# `Neo.Wallets.AssetDescriptor`.
//!
//! Reads the human-facing metadata (`name`, `symbol`, `decimals`) of a NEP-17
//! asset by looking up its deployed [`ContractState`] and probing the
//! `decimals` / `symbol` methods through a read-only [`ApplicationEngine`] run.
//! Used by the wallet/RPC layer to render transfer amounts.
//!
//! This is a faithful port of `Neo.Wallets.AssetDescriptor`
//! (`src/Neo/Wallets/AssetDescriptor.cs`):
//!
//! 1. `ContractManagement.GetContract(snapshot, assetId)` — absent ⇒ error.
//! 2. Build a script that emits a dynamic call to `decimals` then `symbol`
//!    (both `CallFlags.ReadOnly`).
//! 3. Run it with a 0.3 GAS budget; a non-`HALT` final state ⇒ error.
//! 4. `Symbol` is the top result (emitted last), `Decimals` is below it;
//!    `AssetName` is the contract manifest name.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_native_contracts::ContractManagement;
use neo_primitives::{CallFlags, TriggerType, UInt160};
use neo_vm::script_builder::ScriptBuilder;
use neo_storage::DataCache;
use neo_vm_rs::VmState;
use num_traits::ToPrimitive;

/// GAS budget for the descriptor probe, matching C# `ApplicationEngine.Run`
/// (`gas: 0_30000000L` — 0.3 GAS).
const DESCRIPTOR_PROBE_GAS: i64 = 30_000_000;

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
    /// Builds the descriptor for `asset_id` against `snapshot`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::invalid_argument`] when no contract is deployed at
    /// `asset_id`, or when the read-only `decimals` / `symbol` probe fails to
    /// `HALT` — matching the two `ArgumentException` cases C# throws. Returns
    /// [`CoreError::invalid_format`] when the reported `symbol`/`decimals` are
    /// not a valid UTF-8 string / in-range byte.
    pub fn new(
        snapshot: Arc<DataCache>,
        settings: ProtocolSettings,
        asset_id: UInt160,
    ) -> CoreResult<Self> {
        let contract = ContractManagement::get_contract_from_snapshot(&snapshot, &asset_id)?
            .ok_or_else(|| {
                CoreError::invalid_argument(format!(
                    "No asset contract found for assetId {asset_id}. Please ensure the assetId \
                     is correct and the asset is deployed on the blockchain."
                ))
            })?;

        let mut builder = ScriptBuilder::new();
        emit_descriptor_call(&mut builder, &asset_id, "decimals")?;
        emit_descriptor_call(&mut builder, &asset_id, "symbol")?;
        let script = builder.to_array();

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            settings,
            DESCRIPTOR_PROBE_GAS,
            None,
        )
        .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, None)
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?;

        let state = engine.execute_allow_fault();
        if state != VmState::HALT {
            return Err(CoreError::invalid_argument(format!(
                "Failed to execute 'decimals' or 'symbol' method for asset {asset_id}. The \
                 contract execution did not complete successfully (VM state: {state:?})."
            )));
        }

        // The script emitted `decimals` first then `symbol`, so the result stack
        // is `[decimals, symbol]` bottom-to-top: `symbol` is on top (index 0),
        // `decimals` below it (index 1) — the same pop order as C#.
        let result_stack = engine.result_stack();

        let symbol_bytes = result_stack
            .peek(0)
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?
            .as_bytes()
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
        let symbol = String::from_utf8(symbol_bytes).map_err(|e| {
            CoreError::invalid_format(format!("asset symbol is not valid UTF-8: {e}"))
        })?;

        let decimals_int = result_stack
            .peek(1)
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?
            .as_int()
            .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
        // C# narrows with `(byte)`; an out-of-range value would overflow there,
        // so reject it here rather than silently wrapping.
        let decimals = decimals_int.to_u8().ok_or_else(|| {
            CoreError::invalid_format(format!(
                "asset {asset_id} reported an out-of-range decimals value: {decimals_int}"
            ))
        })?;

        Ok(Self {
            asset_id,
            asset_name: contract.manifest.name,
            symbol,
            decimals,
        })
    }
}

impl std::fmt::Display for AssetDescriptor {
    /// Matches C# `AssetDescriptor.ToString()`, which returns `AssetName`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.asset_name)
    }
}

/// Emits a no-argument dynamic call to `method` on `asset_id` with
/// `CallFlags.ReadOnly`, mirroring C# `ScriptBuilderExtensions.EmitDynamicCall`:
/// push an empty argument array (`PUSH0; PACK`), then the call flags, method
/// name, and contract hash, followed by the `System.Contract.Call` syscall.
fn emit_descriptor_call(
    builder: &mut ScriptBuilder,
    asset_id: &UInt160,
    method: &str,
) -> CoreResult<()> {
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&asset_id.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|e| CoreError::invalid_operation(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::{NativeContract, build_native_contract_state};
    use neo_native_contracts::{GasToken, NeoToken};
    use neo_storage::{DataCache, StorageItem, StorageKey};

    /// `ContractManagement.PREFIX_CONTRACT` — the per-contract storage prefix
    /// (verified against `neo-native-contracts/src/contract_management.rs`).
    const PREFIX_CONTRACT: u8 = 8;

    /// Inserts a deployed `ContractState` for `state.hash` into `cache` under the
    /// ContractManagement record key (the C# interoperable stack-item record),
    /// mirroring a post-genesis snapshot so `get_contract_from_snapshot` can
    /// resolve it.
    fn deploy_contract_record(cache: &DataCache, state: &ContractState) {
        let record = state
            .serialize_contract_record()
            .expect("serialize contract record");

        let mut key = Vec::with_capacity(1 + 20);
        key.push(PREFIX_CONTRACT);
        key.extend_from_slice(&state.hash.to_bytes());

        cache.add(
            StorageKey::new(ContractManagement::ID, key),
            StorageItem::from_bytes(record),
        );
    }

    #[test]
    fn nonexistent_asset_id_is_rejected() {
        // C# `TestConstructorWithNonexistAssetId`: an undeployed asset id throws
        // ArgumentException; here it maps to `invalid_argument`.
        let snapshot = Arc::new(DataCache::new(false));
        let settings = ProtocolSettings::default();
        let bogus = UInt160::from_bytes(&[0xAB; 20]).unwrap();

        let err = AssetDescriptor::new(snapshot, settings, bogus)
            .expect_err("undeployed asset must be rejected");
        assert!(
            err.to_string().contains("No asset contract found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn descriptor_reads_gas_metadata() {
        // C# `Check_GAS`: against a snapshot where GAS is deployed, the descriptor
        // exposes name=GasToken, symbol=GAS, decimals=8.
        // The in-VM `System.Contract.Call` resolves the target via the global
        // native-contract provider seam, so install the standard provider (the
        // same thing node startup does) before probing.
        neo_native_contracts::install();
        let cache = DataCache::new(false);
        let settings = ProtocolSettings::default();
        let gas = GasToken;
        let gas_state = build_native_contract_state(&gas, &settings, 0);
        deploy_contract_record(&cache, &gas_state);

        let snapshot = Arc::new(cache);
        let gas_hash = NativeContract::hash(&gas);

        let descriptor =
            AssetDescriptor::new(snapshot, settings, gas_hash).expect("GAS descriptor must build");

        assert_eq!(descriptor.asset_id, gas_hash);
        assert_eq!(descriptor.asset_name, "GasToken");
        assert_eq!(descriptor.to_string(), "GasToken");
        assert_eq!(descriptor.symbol, "GAS");
        assert_eq!(descriptor.decimals, 8);
    }

    #[test]
    fn descriptor_reads_neo_metadata() {
        // C# `Check_NEO`: name=NeoToken, symbol=NEO, decimals=0 (exercises the
        // zero-decimals extraction path).
        neo_native_contracts::install();
        let cache = DataCache::new(false);
        let settings = ProtocolSettings::default();
        let neo = NeoToken;
        let neo_state = build_native_contract_state(&neo, &settings, 0);
        deploy_contract_record(&cache, &neo_state);

        let snapshot = Arc::new(cache);
        let neo_hash = NativeContract::hash(&neo);

        let descriptor =
            AssetDescriptor::new(snapshot, settings, neo_hash).expect("NEO descriptor must build");

        assert_eq!(descriptor.asset_id, neo_hash);
        assert_eq!(descriptor.asset_name, "NeoToken");
        assert_eq!(descriptor.to_string(), "NeoToken");
        assert_eq!(descriptor.symbol, "NEO");
        assert_eq!(descriptor.decimals, 0);
    }
}
