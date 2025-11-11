use alloc::{format, string::String};

use serde_json::Value;

use crate::{h160::H160, tx::WitnessRule};
use neo_crypto::ecc256::PublicKey;

pub(crate) const MAX_SUBITEMS: usize = 16;

pub(crate) fn parse_contract_hash(value: &Value) -> Result<H160, String> {
    let text = value
        .as_str()
        .ok_or_else(|| "allowedcontracts entries must be strings".to_string())?;
    H160::try_from(text).map_err(|_| "Invalid contract hash in allowedcontracts".to_string())
}

pub(crate) fn parse_public_key(value: &Value) -> Result<PublicKey, String> {
    let text = value
        .as_str()
        .ok_or_else(|| "allowedgroups entries must be strings".to_string())?;
    let trimmed = text.trim_start_matches("0x").trim_start_matches("0X");
    let bytes =
        hex::decode(trimmed).map_err(|_| "Invalid ECPoint hex in allowedgroups".to_string())?;
    PublicKey::from_sec1_bytes(&bytes).map_err(|_| "Invalid ECPoint encoding".to_string())
}

pub(crate) fn parse_rules(values: &[Value]) -> Result<Vec<WitnessRule>, String> {
    values
        .iter()
        .map(|value| {
            serde_json::from_value::<WitnessRule>(value.clone())
                .map_err(|err| format!("Invalid witness rule: {err}"))
        })
        .collect()
}
