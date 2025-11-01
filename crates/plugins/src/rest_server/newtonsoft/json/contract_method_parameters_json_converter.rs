// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractMethodParametersJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractParameterDefinition;
use serde_json::Value;

pub struct ContractMethodParametersJsonConverter;

impl ContractMethodParametersJsonConverter {
    pub fn to_json(parameter: &ContractParameterDefinition) -> Value {
        RestServerUtility::contract_method_parameter_to_j_token(parameter)
    }
}
