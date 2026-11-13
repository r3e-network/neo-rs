// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.

use super::validate_length;
use hex::{decode as hex_decode, encode as hex_encode};
use neo_crypto::{ECCurve, ECPoint};
use neo_io::serializable::helper::{
    deserialize_array, deserialize_array_with, get_var_size_for_slice,
    get_var_size_serializable_slice, serialize_array, serialize_array_with,
};
use neo_io::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UINT160_SIZE, UInt160, WitnessScope};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

const MAX_SUBITEMS: usize = 16;
/// Maximum number of allowed contracts, groups, or witness rules on a signer.
pub const MAX_SIGNER_SUBITEMS: usize = MAX_SUBITEMS;
const ECPOINT_COMPRESSED_SIZE: usize = 33;

/// Represents a signer of a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signer {
    /// The account of the signer.
    pub account: UInt160,
    /// The scopes of the witness.
    pub scopes: WitnessScope,
    /// Contracts allowed when CustomContracts is set.
    pub allowed_contracts: Vec<UInt160>,
    /// Groups allowed when CustomGroups is set.
    pub allowed_groups: Vec<ECPoint>,
    /// Rules applied when WitnessRules is set.
    pub rules: Vec<WitnessRule>,
}

impl Signer {
    /// Creates a new signer.
    pub fn new(account: UInt160, scopes: WitnessScope) -> Self {
        Self {
            account,
            scopes,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }
    /// Returns the witness scope.
    pub fn scopes(&self) -> WitnessScope {
        self.scopes
    }
    /// Expands scopes in the original GetAllRules order.
    pub fn get_all_rules(&self) -> Vec<WitnessRule> {
        if self.scopes == WitnessScope::GLOBAL {
            return vec![WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::Boolean { value: true },
            )];
        }
        let mut result = Vec::with_capacity(4);
        if self.scopes.contains(WitnessScope::CALLED_BY_ENTRY) {
            result.push(WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::CalledByEntry,
            ));
        }
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            for hash in &self.allowed_contracts {
                result.push(WitnessRule::new(
                    WitnessRuleAction::Allow,
                    WitnessCondition::ScriptHash { hash: *hash },
                ));
            }
        }
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            for group in &self.allowed_groups {
                result.push(WitnessRule::new(
                    WitnessRuleAction::Allow,
                    WitnessCondition::Group {
                        group: group.to_bytes(),
                    },
                ));
            }
        }
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            result.extend(self.rules.clone());
        }
        result
    }
    /// Converts the signer to a JSON object.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::Map::new();
        json.insert(
            "account".to_string(),
            serde_json::json!(self.account.to_string()),
        );
        json.insert(
            "scopes".to_string(),
            serde_json::json!(self.scopes.to_string()),
        );
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            let contracts: Vec<_> = self
                .allowed_contracts
                .iter()
                .map(|c| serde_json::Value::String(c.to_string()))
                .collect();
            json.insert(
                "allowedcontracts".to_string(),
                serde_json::Value::Array(contracts),
            );
        }
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            let groups: Vec<_> = self
                .allowed_groups
                .iter()
                .map(|g| serde_json::Value::String(hex_encode(g.as_bytes())))
                .collect();
            json.insert(
                "allowedgroups".to_string(),
                serde_json::Value::Array(groups),
            );
        }
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            let rules: Vec<_> = self
                .rules
                .iter()
                .map(neo_io::WitnessRule::to_json)
                .collect();
            json.insert("rules".to_string(), serde_json::Value::Array(rules));
        }
        serde_json::Value::Object(json)
    }
    /// Creates a signer from a JSON object.
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json
            .as_object()
            .ok_or_else(|| "Signer JSON must be an object".to_string())?;
        let account_str = obj
            .get("account")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Signer.account must be a string".to_string())?;
        let account =
            UInt160::from_str(account_str).map_err(|e| format!("Invalid signer account: {e}"))?;
        let scopes_str = obj
            .get("scopes")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Signer.scopes must be a string".to_string())?;
        let scopes = scopes_str
            .parse::<WitnessScope>()
            .map_err(|e| format!("Invalid witness scope: {e}"))?;
        let mut signer = Self::new(account, scopes);
        if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            let value = obj.get("allowedcontracts").ok_or_else(|| {
                "allowedcontracts must be provided when CustomContracts scope is set".to_string()
            })?;
            let array = value
                .as_array()
                .ok_or_else(|| "allowedcontracts must be an array".to_string())?;
            if array.len() > MAX_SUBITEMS {
                return Err("Too many allowed contracts".to_string());
            }
            signer.allowed_contracts = array
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| "allowedcontracts items must be strings".to_string())
                        .and_then(|s| {
                            UInt160::from_str(s)
                                .map_err(|e| format!("Invalid allowed contract hash: {e}"))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            let value = obj.get("allowedgroups").ok_or_else(|| {
                "allowedgroups must be provided when CustomGroups scope is set".to_string()
            })?;
            let array = value
                .as_array()
                .ok_or_else(|| "allowedgroups must be an array".to_string())?;
            if array.len() > MAX_SUBITEMS {
                return Err("Too many allowed groups".to_string());
            }
            signer.allowed_groups = array
                .iter()
                .map(|value| {
                    let text = value
                        .as_str()
                        .ok_or_else(|| "allowedgroups items must be strings".to_string())?;
                    let bytes = hex_decode(text.trim_start_matches("0x"))
                        .map_err(|e| format!("Invalid ECPoint hex: {e}"))?;
                    ECPoint::from_bytes(&bytes).map_err(|e| format!("Invalid ECPoint: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        if scopes.contains(WitnessScope::WITNESS_RULES) {
            let value = obj.get("rules").ok_or_else(|| {
                "rules must be provided when WitnessRules scope is set".to_string()
            })?;
            let array = value
                .as_array()
                .ok_or_else(|| "rules must be an array".to_string())?;
            if array.len() > MAX_SUBITEMS {
                return Err("Too many witness rules".to_string());
            }
            signer.rules = array
                .iter()
                .map(|value| {
                    WitnessRule::from_json(value).map_err(|e| format!("Invalid witness rule: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(signer)
    }
}

impl Serializable for Signer {
    fn size(&self) -> usize {
        let mut size = UINT160_SIZE + 1;
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            size += get_var_size_serializable_slice(&self.allowed_contracts);
        }
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            size += get_var_size_for_slice(&self.allowed_groups, |group| group.as_bytes().len());
        }
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            size += get_var_size_serializable_slice(&self.rules);
        }
        size
    }
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_serializable(&self.account)?;
        writer.write_u8(self.scopes.bits())?;
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            validate_length(
                self.allowed_contracts.len(),
                MAX_SUBITEMS,
                "Allowed contracts",
            )?;
            serialize_array(&self.allowed_contracts, writer)?;
        }
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            validate_length(self.allowed_groups.len(), MAX_SUBITEMS, "Allowed groups")?;
            serialize_array_with(&self.allowed_groups, writer, |group, writer| {
                let encoded = group
                    .encode_point(true)
                    .map_err(|e| IoError::invalid_data(e.to_string()))?;
                if encoded.len() != 33 {
                    return Err(IoError::invalid_data("Group must be compressed"));
                }
                writer.write_bytes(&encoded)
            })?;
        }
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            validate_length(self.rules.len(), MAX_SUBITEMS, "Rules")?;
            serialize_array(&self.rules, writer)?;
        }
        Ok(())
    }
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let account = <UInt160 as Serializable>::deserialize(reader)?;
        let scopes_byte = reader.read_u8()?;
        let scopes = WitnessScope::from_byte(scopes_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid witness scope"))?;
        let mut allowed_contracts = Vec::new();
        let mut allowed_groups = Vec::new();
        let mut rules = Vec::new();
        let invalid_flags = !(WitnessScope::CALLED_BY_ENTRY
            | WitnessScope::CUSTOM_CONTRACTS
            | WitnessScope::CUSTOM_GROUPS
            | WitnessScope::WITNESS_RULES
            | WitnessScope::GLOBAL);
        if scopes.intersects(invalid_flags) {
            return Err(IoError::invalid_data("Invalid witness scope flags"));
        }
        if scopes.contains(WitnessScope::GLOBAL) && scopes != WitnessScope::GLOBAL {
            return Err(IoError::invalid_data(
                "Global scope cannot be combined with other scopes",
            ));
        }
        if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            allowed_contracts = deserialize_array(reader, MAX_SUBITEMS)?;
        }
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            allowed_groups = deserialize_array_with(reader, MAX_SUBITEMS, |reader| {
                let encoded = reader.read_bytes(ECPOINT_COMPRESSED_SIZE)?;
                ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &encoded)
                    .map_err(|e| IoError::invalid_data(e.to_string()))
            })?;
        }
        if scopes.contains(WitnessScope::WITNESS_RULES) {
            rules = deserialize_array(reader, MAX_SUBITEMS)?;
        }
        Ok(Self {
            account,
            scopes,
            allowed_contracts,
            allowed_groups,
            rules,
        })
    }
}

impl std::hash::Hash for Signer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.account, state);
        std::hash::Hash::hash(&self.scopes, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signer_deserialize_rejects_global_combined_scope() {
        let mut data = vec![0u8; UINT160_SIZE];
        data.push((WitnessScope::GLOBAL | WitnessScope::CALLED_BY_ENTRY).bits());
        assert!(<Signer as Serializable>::deserialize(&mut MemoryReader::new(&data)).is_err());
    }
    #[test]
    fn signer_deserialize_rejects_too_many_allowed_contracts() {
        let mut writer = BinaryWriter::new();
        writer.write_serializable(&UInt160::zero()).unwrap();
        writer
            .write_u8(WitnessScope::CUSTOM_CONTRACTS.bits())
            .unwrap();
        writer.write_var_uint((MAX_SUBITEMS + 1) as u64).unwrap();
        for value in 0..=MAX_SUBITEMS {
            writer
                .write_serializable(&UInt160::from_bytes(&[value as u8; UINT160_SIZE]).unwrap())
                .unwrap();
        }
        assert!(
            <Signer as Serializable>::deserialize(&mut MemoryReader::new(writer.as_bytes()))
                .is_err()
        );
    }
    #[test]
    fn signer_deserialize_rejects_too_many_rules() {
        let mut writer = BinaryWriter::new();
        writer.write_serializable(&UInt160::zero()).unwrap();
        writer.write_u8(WitnessScope::WITNESS_RULES.bits()).unwrap();
        writer.write_var_uint((MAX_SUBITEMS + 1) as u64).unwrap();
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::Boolean { value: true },
        );
        for _ in 0..=MAX_SUBITEMS {
            writer.write_serializable(&rule).unwrap();
        }
        assert!(
            <Signer as Serializable>::deserialize(&mut MemoryReader::new(writer.as_bytes()))
                .is_err()
        );
    }
}
