// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractPermissionDescriptorJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractPermissionDescriptor;
use serde_json::Value;

pub struct ContractPermissionDescriptorJsonConverter;

impl ContractPermissionDescriptorJsonConverter {
    pub fn to_json(descriptor: &ContractPermissionDescriptor) -> Value {
        RestServerUtility::contract_permission_descriptor_to_j_token(descriptor)
    }
}
