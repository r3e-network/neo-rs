use hex::encode;
use serde_json::{Map as JsonMap, Value};

use crate::signer::SignerScopes;

use super::super::model::Account;

pub(crate) fn embed_signer_extra(extra: &Option<Value>, account: &Account) -> Option<Value> {
    if account.signer_scopes == SignerScopes::CALLED_BY_ENTRY
        && account.allowed_contracts.is_empty()
        && account.allowed_groups.is_empty()
    {
        return extra.clone();
    }

    let signer_value = serialize_signer(account);

    match extra.clone() {
        Some(Value::Object(mut map)) => {
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("data".into(), other);
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
        None => {
            let mut map = JsonMap::new();
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
    }
}

fn serialize_signer(account: &Account) -> Value {
    let mut signer = JsonMap::new();
    signer.insert(
        "scopes".into(),
        Value::String(account.signer_scopes.to_witness_scope_string()),
    );
    if !account.allowed_contracts.is_empty() {
        let contracts = account
            .allowed_contracts
            .iter()
            .map(|hash| Value::String(format!("{hash}")))
            .collect();
        signer.insert("allowedContracts".into(), Value::Array(contracts));
    }
    if !account.allowed_groups.is_empty() {
        let groups = account
            .allowed_groups
            .iter()
            .map(|group| Value::String(format!("0x{}", encode(group))))
            .collect();
        signer.insert("allowedGroups".into(), Value::Array(groups));
    }
    Value::Object(signer)
}
