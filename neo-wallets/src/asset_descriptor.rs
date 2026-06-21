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
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
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
#[path = "tests/asset_descriptor.rs"]
mod tests;
