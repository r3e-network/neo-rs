// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.ContractManifestJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::manifest::ContractManifest;
use serde_json::Value;

pub struct ContractManifestJsonConverter;

impl ContractManifestJsonConverter {
    pub fn to_json(manifest: &ContractManifest) -> Value {
        RestServerUtility::contract_manifest_to_j_token(manifest)
    }
}
