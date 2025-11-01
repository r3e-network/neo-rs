// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractGroupJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractGroup;
use serde_json::Value;

pub struct ContractGroupJsonConverter;

impl ContractGroupJsonConverter {
    pub fn to_json(group: &ContractGroup) -> Value {
        RestServerUtility::contract_group_to_j_token(group)
    }
}
