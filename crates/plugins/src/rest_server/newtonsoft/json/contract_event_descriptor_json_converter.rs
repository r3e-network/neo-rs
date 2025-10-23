// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractEventDescriptorJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractEventDescriptor;
use serde_json::Value;

pub struct ContractEventDescriptorJsonConverter;

impl ContractEventDescriptorJsonConverter {
    pub fn to_json(event: &ContractEventDescriptor) -> Value {
        RestServerUtility::contract_event_to_j_token(event)
    }
}
