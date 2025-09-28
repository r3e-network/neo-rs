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
    smart_contract::{application_engine::ApplicationEngine, call_flags::CallFlags},
    uint160::UInt160,
};
use neo_vm::{ScriptBuilder, VMState};

/// Represents the descriptor of an asset.
/// Matches C# AssetDescriptor class exactly
pub struct AssetDescriptor {
    /// The id of the asset.
    /// Matches C# AssetId property
    pub asset_id: UInt160,

    /// The name of the asset.
    /// Matches C# AssetName property
    pub asset_name: String,

    /// The symbol of the asset.
    /// Matches C# Symbol property
    pub symbol: String,

    /// The number of decimal places of the token.
    /// Matches C# Decimals property
    pub decimals: u8,
}

impl AssetDescriptor {
    /// Initializes a new instance of the AssetDescriptor class.
    /// Matches C# constructor exactly
    pub fn new(
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        asset_id: UInt160,
    ) -> Result<Self, String> {
        let contract = NativeContract::ContractManagement::get_contract(snapshot, asset_id)
            .ok_or_else(|| format!("No asset contract found for assetId {}. Please ensure the assetId is correct and the asset is deployed on the blockchain.", asset_id))?;

        let mut script_builder = ScriptBuilder::new();
        script_builder.emit_dynamic_call(asset_id, "decimals", CallFlags::ReadOnly);
        script_builder.emit_dynamic_call(asset_id, "symbol", CallFlags::ReadOnly);
        let script = script_builder.to_array();

        let mut engine =
            ApplicationEngine::run(&script, snapshot, Some(settings), Some(30_000_000))?;

        if engine.state != VMState::Halt {
            return Err(format!("Failed to execute 'decimals' or 'symbol' method for asset {}. The contract execution did not complete successfully (VM state: {:?}).", asset_id, engine.state));
        }

        let symbol = engine.result_stack.pop().unwrap().get_string()?;
        let decimals = engine.result_stack.pop().unwrap().get_integer()? as u8;

        Ok(AssetDescriptor {
            asset_id,
            asset_name: contract.manifest.name.clone(),
            symbol,
            decimals,
        })
    }
}

impl std::fmt::Display for AssetDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.asset_name)
    }
}

/// Native contract management
pub mod NativeContract {
    pub mod ContractManagement {
        use super::super::{DataCache, UInt160};

        pub fn get_contract(snapshot: &DataCache, asset_id: UInt160) -> Option<Contract> {
            // In a real implementation, this would get the contract from the snapshot
            Some(Contract {
                manifest: ContractManifest {
                    name: "Asset".to_string(),
                },
            })
        }
    }
}

/// Contract structure
pub struct Contract {
    pub manifest: ContractManifest,
}

/// Contract manifest structure
pub struct ContractManifest {
    pub name: String,
}
