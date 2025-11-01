// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.WitnessJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::witness::Witness;
use serde_json::Value;

pub struct WitnessJsonConverter;

impl WitnessJsonConverter {
    pub fn to_json(witness: &Witness) -> Value {
        RestServerUtility::witness_to_j_token(witness)
    }
}
