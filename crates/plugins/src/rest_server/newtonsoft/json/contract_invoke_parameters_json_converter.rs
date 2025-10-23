// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractInvokeParametersJsonConverter`.

use crate::rest_server::models::contract::invoke_params::InvokeParams;
use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use serde_json::Value;

pub struct ContractInvokeParametersJsonConverter;

impl ContractInvokeParametersJsonConverter {
    pub fn from_json(token: &Value) -> Result<InvokeParams, RestServerUtilityError> {
        RestServerUtility::contract_invoke_parameters_from_j_token(token)
            .map_err(RestServerUtilityError::StackItem)
    }
}
