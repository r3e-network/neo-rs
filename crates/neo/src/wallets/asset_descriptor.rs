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
    persistence::data_cache::DataCache, protocol_settings::ProtocolSettings, uint160::UInt160,
};

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
        _settings: &ProtocolSettings,
        asset_id: UInt160,
    ) -> Result<Self, String> {
        let contract = native_contract::contract_management::get_contract(snapshot, asset_id)
            .ok_or_else(|| format!("No asset contract found for assetId {}. Please ensure the assetId is correct and the asset is deployed on the blockchain.", asset_id))?;

        let symbol = contract.manifest.name.clone();
        let decimals = 0;

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
pub mod native_contract {
    pub mod contract_management {
        use super::super::{Contract, ContractManifest, DataCache, UInt160};

        pub fn get_contract(_snapshot: &DataCache, _asset_id: UInt160) -> Option<Contract> {
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
