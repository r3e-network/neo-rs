// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractParameterJsonConverter`.

use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_core::smart_contract::contract_parameter::ContractParameter;
use serde_json::Value;

pub struct ContractParameterJsonConverter;

impl ContractParameterJsonConverter {
    pub fn from_json(token: &Value) -> Result<ContractParameter, RestServerUtilityError> {
        RestServerUtility::contract_parameter_from_j_token(token)
            .map_err(RestServerUtilityError::StackItem)
    }
}
