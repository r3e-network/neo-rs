// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.TransactionAttributeJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use serde_json::Value;

pub struct TransactionAttributeJsonConverter;

impl TransactionAttributeJsonConverter {
    pub fn to_json(attribute: &TransactionAttribute) -> Value {
        RestServerUtility::transaction_attribute_to_j_token(attribute)
    }
}
