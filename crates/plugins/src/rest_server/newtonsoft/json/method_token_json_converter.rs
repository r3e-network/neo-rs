// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.MethodTokenJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::contract_state::MethodToken;
use serde_json::Value;

pub struct MethodTokenJsonConverter;

impl MethodTokenJsonConverter {
    pub fn to_json(token: &MethodToken) -> Value {
        RestServerUtility::method_token_to_j_token(token)
    }
}
