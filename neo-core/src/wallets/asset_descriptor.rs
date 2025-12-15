// Copyright (C) 2015-2025 The Neo Project.
//
// asset_descriptor.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    persistence::data_cache::DataCache,
    protocol_settings::ProtocolSettings,
    smart_contract::{
        application_engine::ApplicationEngine, call_flags::CallFlags, native::ContractManagement,
        trigger_type::TriggerType,
    },
};
use neo_primitives::UInt160;
use neo_vm::{op_code::OpCode, vm_state::VMState, ScriptBuilder};
use num_traits::ToPrimitive;
use std::sync::Arc;

/// Represents the descriptor of an asset (matches C# `AssetDescriptor`).
pub struct AssetDescriptor {
    pub asset_id: UInt160,
    pub asset_name: String,
    pub symbol: String,
    pub decimals: u8,
}

impl AssetDescriptor {
    /// GAS budget for querying decimals/symbol (matches C# 0.3 GAS).
    pub const QUERY_GAS: i64 = 30_000_000;

    pub fn new(
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        asset_id: UInt160,
    ) -> Result<Self, String> {
        let contract = ContractManagement::get_contract_from_snapshot(snapshot, &asset_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| {
                format!(
                    "No asset contract found for assetId {}. Please ensure the assetId is correct and deployed.",
                    asset_id
                )
            })?;
        let asset_name = contract.manifest.name.clone();

        let mut builder = ScriptBuilder::new();
        Self::emit_contract_call(&mut builder, &asset_id, b"decimals")?;
        Self::emit_contract_call(&mut builder, &asset_id, b"symbol")?;
        let script = builder.to_array();

        let snapshot_arc = Arc::new(snapshot.clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot_arc),
            None,
            settings.clone(),
            Self::QUERY_GAS,
            None,
        )
        .map_err(|err| err.to_string())?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(asset_id))
            .map_err(|err| err.to_string())?;
        engine.execute().map_err(|err| err.to_string())?;

        if engine.state() != VMState::HALT {
            return Err(format!(
                "Failed to query asset metadata (VM state: {:?})",
                engine.state()
            ));
        }

        let stack = engine.result_stack();
        if stack.len() < 2 {
            return Err("Asset metadata stack underflow".to_string());
        }

        let symbol_item = stack.peek(0).map_err(|err| err.to_string())?;
        let decimals_item = stack.peek(1).map_err(|err| err.to_string())?;

        let symbol_bytes = symbol_item.as_bytes().map_err(|err| err.to_string())?;
        let symbol = String::from_utf8(symbol_bytes)
            .map_err(|err| format!("Invalid UTF-8 symbol: {}", err))?;

        let decimals_value = decimals_item.as_int().map_err(|err| err.to_string())?;
        let decimals = decimals_value
            .to_u8()
            .ok_or_else(|| "Decimals value is out of range for u8".to_string())?;

        Ok(Self {
            asset_id,
            asset_name,
            symbol,
            decimals,
        })
    }

    fn emit_contract_call(
        builder: &mut ScriptBuilder,
        asset_id: &UInt160,
        method: &[u8],
    ) -> Result<(), String> {
        builder.emit_opcode(OpCode::PUSH0);
        builder.emit_opcode(OpCode::NEWARRAY);
        builder.emit_push_int(CallFlags::READ_ONLY.bits() as i64);
        builder.emit_push(method);
        let asset_bytes = asset_id.to_bytes();
        builder.emit_push(&asset_bytes);
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

impl std::fmt::Display for AssetDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.asset_name)
    }
}
