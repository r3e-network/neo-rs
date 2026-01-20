// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_transaction.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::vm_state_utils::{vm_state_from_str, vm_state_to_string};
use neo_config::ProtocolSettings;
use neo_core::Transaction;
use neo_json::JObject;
use neo_primitives::UInt256;
use neo_vm::VMState;

/// RPC transaction information matching C# RpcTransaction
#[derive(Debug, Clone)]
pub struct RpcTransaction {
    /// The transaction
    pub transaction: Transaction,

    /// Block hash if confirmed
    pub block_hash: Option<UInt256>,

    /// Number of confirmations
    pub confirmations: Option<u32>,

    /// Block timestamp
    pub block_time: Option<u64>,

    /// VM execution state
    pub vm_state: Option<VMState>,
}

impl RpcTransaction {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json =
            super::super::utility::transaction_to_json(&self.transaction, protocol_settings);

        if let Some(confirmations) = self.confirmations {
            if let Some(ref block_hash) = self.block_hash {
                json.insert(
                    "blockhash".to_string(),
                    neo_json::JToken::String(block_hash.to_string()),
                );
            }
            json.insert(
                "confirmations".to_string(),
                neo_json::JToken::Number(confirmations as f64),
            );

            if let Some(block_time) = self.block_time {
                json.insert(
                    "blocktime".to_string(),
                    neo_json::JToken::Number(block_time as f64),
                );
            }

            if let Some(ref vm_state) = self.vm_state {
                json.insert(
                    "vmstate".to_string(),
                    neo_json::JToken::String(vm_state_to_string(*vm_state)),
                );
            }
        }

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let transaction = super::super::utility::transaction_from_json(json, protocol_settings)?;

        let (block_hash, confirmations, block_time, vm_state) =
            if json.get("confirmations").is_some() {
                let block_hash = json
                    .get("blockhash")
                    .and_then(|v| v.as_string())
                    .and_then(|s| UInt256::parse(&s).ok());

                let confirmations = json
                    .get("confirmations")
                    .and_then(|v| v.as_number())
                    .map(|n| n as u32);

                let block_time = json
                    .get("blocktime")
                    .and_then(|v| v.as_number())
                    .map(|n| n as u64);

                let vm_state = json
                    .get("vmstate")
                    .and_then(|v| v.as_string())
                    .and_then(|s| vm_state_from_str(&s));

                (block_hash, confirmations, block_time, vm_state)
            } else {
                (None, None, None, None)
            };

        Ok(Self {
            transaction,
            block_hash,
            confirmations,
            block_time,
            vm_state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JToken;
    use std::fs;
    use std::path::PathBuf;

    fn load_rpc_case_result(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token
            .as_array()
            .expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return result.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
    }

    #[test]
    fn transaction_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("getrawtransactionasync");
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcTransaction::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
