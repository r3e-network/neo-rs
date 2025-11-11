use alloc::format;

use serde_json::{Map, Value};

use crate::tx::WitnessScope;

use super::super::Signer;

pub(super) fn build_signer_json(signer: &Signer) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "account".to_string(),
        Value::String(signer.account.to_string()),
    );
    obj.insert(
        "scopes".to_string(),
        Value::String(signer.scopes.to_string()),
    );

    if signer.scopes.has_scope(WitnessScope::CustomContracts) && !signer.allowed_contract.is_empty()
    {
        obj.insert(
            "allowedcontracts".to_string(),
            Value::Array(
                signer
                    .allowed_contract
                    .iter()
                    .map(|hash| Value::String(hash.to_string()))
                    .collect(),
            ),
        );
    }

    if signer.scopes.has_scope(WitnessScope::CustomGroups) && !signer.allowed_groups.is_empty() {
        obj.insert(
            "allowedgroups".to_string(),
            Value::Array(
                signer
                    .allowed_groups
                    .iter()
                    .map(|group| {
                        let encoded = group.to_compressed();
                        Value::String(format!("0x{}", hex::encode(encoded)))
                    })
                    .collect(),
            ),
        );
    }

    if signer.scopes.has_scope(WitnessScope::WitnessRules) && !signer.rules.is_empty() {
        obj.insert(
            "rules".to_string(),
            Value::Array(
                signer
                    .rules
                    .iter()
                    .map(|rule| serde_json::to_value(rule).unwrap_or(Value::Null))
                    .collect(),
            ),
        );
    }

    Value::Object(obj)
}
