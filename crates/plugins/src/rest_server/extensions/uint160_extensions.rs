// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of the specialised utilities in `Neo.Plugins.RestServer.Extensions.UInt160Extensions`.

use crate::rest_server::helpers::contract_helper::ContractHelper;
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use neo_core::smart_contract::native::contract_management::ContractManagement;
use neo_core::UInt160;

/// Helper functions to inspect SmartContract support for REST handlers.
pub struct UInt160Extensions;

impl UInt160Extensions {
    /// Returns `true` when the supplied hash corresponds to a deployed contract that supports NEP-17.
    pub fn is_valid_nep17(script_hash: &UInt160) -> bool {
        let Some(system) = RestServerGlobals::neo_system() else {
            return false;
        };
        let snapshot = system.store_cache().data_cache().clone();
        if let Ok(Some(contract)) =
            ContractManagement::get_contract_from_snapshot(&snapshot, script_hash)
        {
            ContractHelper::is_nep17_supported_contract(&contract)
        } else {
            false
        }
    }

    /// Returns `true` when the supplied hash resolves to an existing contract.
    pub fn is_valid_contract(script_hash: &UInt160) -> bool {
        let Some(system) = RestServerGlobals::neo_system() else {
            return false;
        };
        let snapshot = system.store_cache().data_cache().clone();
        ContractManagement::get_contract_from_snapshot(&snapshot, script_hash)
            .map(|contract| contract.is_some())
            .unwrap_or(false)
    }
}
