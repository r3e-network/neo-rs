// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.SignerJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::Signer;
use serde_json::Value;

pub struct SignerJsonConverter;

impl SignerJsonConverter {
    pub fn to_json(signer: &Signer) -> Value {
        RestServerUtility::signer_to_j_token(signer)
    }
}
