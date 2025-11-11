use alloc::string::String;

use serde_json::Value;

use crate::tx::{Signer, WitnessScope};

use super::super::parse::{parse_contract_hash, parse_public_key, parse_rules};

pub(super) fn populate_scoped_fields(
    value: &Value,
    signer: &mut Signer,
    max_items: usize,
) -> Result<(), String> {
    if signer.scopes.has_scope(WitnessScope::CustomContracts) {
        let contracts = value
            .get("allowedcontracts")
            .ok_or_else(|| "allowedcontracts required for CustomContracts scope".to_string())?;
        let array = contracts
            .as_array()
            .ok_or_else(|| "allowedcontracts must be an array".to_string())?;
        if array.len() > max_items {
            return Err("Too many allowed contracts".to_string());
        }
        signer.allowed_contract = array
            .iter()
            .map(parse_contract_hash)
            .collect::<Result<_, _>>()?;
    }

    if signer.scopes.has_scope(WitnessScope::CustomGroups) {
        let groups = value
            .get("allowedgroups")
            .ok_or_else(|| "allowedgroups required for CustomGroups scope".to_string())?;
        let array = groups
            .as_array()
            .ok_or_else(|| "allowedgroups must be an array".to_string())?;
        if array.len() > max_items {
            return Err("Too many allowed groups".to_string());
        }
        signer.allowed_groups = array
            .iter()
            .map(parse_public_key)
            .collect::<Result<_, _>>()?;
    }

    if signer.scopes.has_scope(WitnessScope::WitnessRules) {
        let rules = value
            .get("rules")
            .ok_or_else(|| "rules required when WitnessRules scope is set".to_string())?;
        let array = rules
            .as_array()
            .ok_or_else(|| "rules must be an array".to_string())?;
        if array.len() > max_items {
            return Err("Too many witness rules".to_string());
        }
        signer.rules = parse_rules(array)?;
    }

    Ok(())
}
