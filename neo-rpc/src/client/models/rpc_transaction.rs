use super::vm_state_utils::{vm_state_from_str, vm_state_to_string};
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_payloads::Transaction;
use neo_primitives::UInt256;
use neo_serialization::json::JObject;
use neo_vm_rs::VmState;

/// RPC transaction information matching C# `RpcTransaction`
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
    pub vm_state: Option<VmState>,
}

impl RpcTransaction {
    /// Converts to JSON
    /// Matches C# `ToJson`
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json =
            super::super::utility::transaction_to_json(&self.transaction, protocol_settings);

        if let Some(confirmations) = self.confirmations {
            if let Some(ref block_hash) = self.block_hash {
                json.insert(
                    "blockhash".to_string(),
                    neo_serialization::json::JToken::String(block_hash.to_string()),
                );
            }
            json.insert(
                "confirmations".to_string(),
                neo_serialization::json::JToken::Number(f64::from(confirmations)),
            );

            if let Some(block_time) = self.block_time {
                json.insert(
                    "blocktime".to_string(),
                    neo_serialization::json::JToken::Number(block_time as f64),
                );
            }

            if let Some(ref vm_state) = self.vm_state {
                json.insert(
                    "vmstate".to_string(),
                    neo_serialization::json::JToken::String(vm_state_to_string(*vm_state)),
                );
            }
        }

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let transaction = super::super::utility::transaction_from_json(json, protocol_settings)?;

        let (block_hash, confirmations, block_time, vm_state) =
            if json.get("confirmations").is_some() {
                let block_hash = json
                    .get("blockhash")
                    .and_then(neo_serialization::json::JToken::as_string)
                    .and_then(|s| UInt256::parse(&s).ok());

                let confirmations = json
                    .get("confirmations")
                    .and_then(neo_serialization::json::JToken::as_number)
                    .map(|n| n as u32);

                let block_time = json
                    .get("blocktime")
                    .and_then(neo_serialization::json::JToken::as_number)
                    .map(|n| n as u64);

                let vm_state = json
                    .get("vmstate")
                    .and_then(neo_serialization::json::JToken::as_string)
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
#[path = "../../tests/client/models/rpc_transaction.rs"]
mod tests;
