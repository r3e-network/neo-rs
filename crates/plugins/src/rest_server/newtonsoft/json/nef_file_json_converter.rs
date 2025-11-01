// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.NefFileJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::smart_contract::nef_file::NefFile;
use serde_json::Value;

pub struct NefFileJsonConverter;

impl NefFileJsonConverter {
    pub fn to_json(nef: &NefFile) -> Value {
        RestServerUtility::contract_nef_file_to_j_token(nef)
    }
}
