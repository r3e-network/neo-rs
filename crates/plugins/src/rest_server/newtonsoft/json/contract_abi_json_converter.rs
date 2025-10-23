// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractAbiJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractAbi;
use serde_json::Value;

pub struct ContractAbiJsonConverter;

impl ContractAbiJsonConverter {
    pub fn to_json(abi: &ContractAbi) -> Value {
        RestServerUtility::contract_abi_to_j_token(abi)
    }
}
