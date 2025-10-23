// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.TransactionJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::transaction::Transaction;
use serde_json::Value;

pub struct TransactionJsonConverter;

impl TransactionJsonConverter {
    pub fn to_json(tx: &Transaction) -> Value {
        RestServerUtility::transaction_to_j_token(tx)
    }
}
