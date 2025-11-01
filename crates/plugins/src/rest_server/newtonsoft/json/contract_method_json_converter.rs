// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractMethodJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractMethodDescriptor;
use serde_json::Value;

pub struct ContractMethodJsonConverter;

impl ContractMethodJsonConverter {
    pub fn to_json(method: &ContractMethodDescriptor) -> Value {
        RestServerUtility::contract_method_to_j_token(method)
    }
}
