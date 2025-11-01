// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::contract_state::ContractState;
use serde_json::Value;

pub struct ContractJsonConverter;

impl ContractJsonConverter {
    pub fn to_json(contract: &ContractState) -> Value {
        RestServerUtility::contract_state_to_j_token(contract)
    }
}
