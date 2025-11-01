// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractPermissionJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractPermission;
use serde_json::Value;

pub struct ContractPermissionJsonConverter;

impl ContractPermissionJsonConverter {
    pub fn to_json(permission: &ContractPermission) -> Value {
        RestServerUtility::contract_permission_to_j_token(permission)
    }
}
