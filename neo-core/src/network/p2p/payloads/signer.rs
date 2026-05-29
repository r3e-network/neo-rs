// Copyright (C) 2015-2025 The Neo Project.
//
// signer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::macros::{OptionExt, ValidateLength};
use crate::neo_io::serializable::helper::{
    deserialize_array, deserialize_array_with, get_var_size_for_slice,
    get_var_size_serializable_slice, serialize_array, serialize_array_with,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::Interoperable;
use crate::neo_vm::StackItem;
use crate::witness_rule::{WitnessRule, WitnessRuleAction};
use crate::{WitnessCondition, WitnessScope};
use neo_crypto::{ECCurve, ECPoint};
use hex::{decode as hex_decode, encode as hex_encode};
use neo_primitives::{UInt160, UINT160_SIZE};
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
// Hash and Hasher now provided by impl_hash_for_fields macro
use std::str::FromStr;

// This limits maximum number of AllowedContracts or AllowedGroups
const MAX_SUBITEMS: usize = 16;

/// Represents a signer of a Transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signer {
    /// The account of the signer.
    pub account: UInt160,

    /// The scopes of the witness.
    pub scopes: WitnessScope,

    /// The contracts that allowed by the witness.
    /// Only available when the CustomContracts flag is set.
    pub allowed_contracts: Vec<UInt160>,

    /// The groups that allowed by the witness.
    /// Only available when the CustomGroups flag is set.
    pub allowed_groups: Vec<ECPoint>,

    /// The rules that the witness must meet.
    /// Only available when the WitnessRules flag is set.
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

    /// Returns the witness scope (C# compatibility helper).
    pub fn scopes(&self) -> WitnessScope {
        self.scopes
    }

    /// Converts all rules contained in the Signer object to WitnessRule.
    /// Matches C# GetAllRules() exactly.
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

        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS)
            && !self.allowed_contracts.is_empty()
        {
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

        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) && !self.allowed_groups.is_empty() {
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

        if self.scopes.contains(WitnessScope::WITNESS_RULES) && !self.rules.is_empty() {
            let rules: Vec<_> = self.rules.iter().map(|r| r.to_json()).collect();
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
            let contracts_value = obj.get("allowedcontracts").ok_or_else(|| {
                "allowedcontracts must be provided when CustomContracts scope is set".to_string()
            })?;
            let contracts_array = contracts_value
                .as_array()
                .ok_or_else(|| "allowedcontracts must be an array".to_string())?;
            if contracts_array.len() > MAX_SUBITEMS {
                return Err("Too many allowed contracts".to_string());
            }

            signer.allowed_contracts = contracts_array
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
            let groups_value = obj.get("allowedgroups").ok_or_else(|| {
                "allowedgroups must be provided when CustomGroups scope is set".to_string()
            })?;
            let groups_array = groups_value
                .as_array()
                .ok_or_else(|| "allowedgroups must be an array".to_string())?;
            if groups_array.len() > MAX_SUBITEMS {
                return Err("Too many allowed groups".to_string());
            }

            signer.allowed_groups = groups_array
                .iter()
                .map(|value| {
                    let text = value
                        .as_str()
                        .ok_or_else(|| "allowedgroups items must be strings".to_string())?;
                    let trimmed = text.trim_start_matches("0x");
                    let bytes =
                        hex_decode(trimmed).map_err(|e| format!("Invalid ECPoint hex: {e}"))?;
                    ECPoint::from_bytes(&bytes).map_err(|e| format!("Invalid ECPoint: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        if scopes.contains(WitnessScope::WITNESS_RULES) {
            let rules_value = obj.get("rules").ok_or_else(|| {
                "rules must be provided when WitnessRules scope is set".to_string()
            })?;
            let rules_array = rules_value
                .as_array()
                .ok_or_else(|| "rules must be an array".to_string())?;
            if rules_array.len() > MAX_SUBITEMS {
                return Err("Too many witness rules".to_string());
            }

            signer.rules = rules_array
                .iter()
                .map(|value| {
                    WitnessRule::from_json(value).map_err(|e| format!("Invalid witness rule: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        Ok(signer)
    }

    /// Converts the signer to a neo-vm-rs stack value (matches C# `Signer.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        let allowed_contracts = if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            self.allowed_contracts
                .iter()
                .copied()
                .map(|hash| StackValue::ByteString(hash.to_bytes()))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let allowed_groups = if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            self.allowed_groups
                .iter()
                .map(|group| StackValue::ByteString(group.to_bytes()))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let rules = if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            self.rules
                .iter()
                .map(WitnessRule::to_stack_value)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        StackValue::Array(vec![
            StackValue::ByteString(self.account.to_bytes()),
            StackValue::Integer(i64::from(self.scopes.bits())),
            StackValue::Array(allowed_contracts),
            StackValue::Array(allowed_groups),
            StackValue::Array(rules),
        ])
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

        // Write allowed contracts if flag is set (use ValidateLength trait)
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            self.allowed_contracts
                .validate_max_length(MAX_SUBITEMS, "Allowed contracts")?;
            serialize_array(&self.allowed_contracts, writer)?;
        }

        // Write allowed groups if flag is set
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            self.allowed_groups
                .validate_max_length(MAX_SUBITEMS, "Allowed groups")?;
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

        // Write rules if flag is set
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            self.rules.validate_max_length(MAX_SUBITEMS, "Rules")?;
            serialize_array(&self.rules, writer)?;
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let account = <UInt160 as Serializable>::deserialize(reader)?;
        let scopes_byte = reader.read_u8()?;
        // Use OptionExt trait to reduce boilerplate
        let scopes =
            WitnessScope::from_byte(scopes_byte).ok_or_invalid_data("Invalid witness scope")?;

        let mut allowed_contracts = Vec::new();
        let mut allowed_groups = Vec::new();
        let mut rules = Vec::new();

        // Validate scopes
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

        // Read allowed contracts if flag is set
        if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            allowed_contracts = deserialize_array(reader, MAX_SUBITEMS)?;
        }

        // Read allowed groups if flag is set
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            allowed_groups = deserialize_array_with(reader, MAX_SUBITEMS, |reader| {
                // C# Signer.Deserialize reads each AllowedGroups entry via ECPoint
                // (DeserializeFrom -> DecodePoint), which accepts 33-byte compressed
                // AND 65-byte uncompressed encodings. Reading a fixed 33 bytes here
                // would misalign the stream on an uncompressed point and reject a
                // transaction C# accepts (Signer is part of the tx hash preimage).
                let encoded = crate::witness_rule::helpers::read_group_bytes(reader)?;
                ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &encoded)
                    .map_err(|e| IoError::invalid_data(e.to_string()))
            })?;
        }

        // Read rules if flag is set
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

impl Interoperable for Signer {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), crate::neo_vm::VmError> {
        // This operation is not supported for Signer.
        // The C# implementation throws NotSupportedException.
        Err(crate::neo_vm::VmError::invalid_operation_msg(
            "Signer::from_stack_item is not supported",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, crate::neo_vm::VmError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert signer StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

// Use macro to reduce boilerplate for Hash implementation
crate::impl_hash_for_fields!(Signer, account, scopes);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_contract::Interoperable;
    use neo_vm_rs::StackValue;

    #[test]
    fn signer_projects_to_neo_vm_rs_stack_value() {
        let account = UInt160::from_bytes(&[0x11; UINT160_SIZE]).unwrap();
        let allowed_contract = UInt160::from_bytes(&[0x22; UINT160_SIZE]).unwrap();
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::Boolean { value: true },
        );
        let scopes = WitnessScope::CUSTOM_CONTRACTS | WitnessScope::WITNESS_RULES;
        let mut signer = Signer::new(account, scopes);
        signer.allowed_contracts.push(allowed_contract);
        signer.rules.push(rule.clone());

        assert_eq!(
            signer.to_stack_value(),
            StackValue::Array(vec![
                StackValue::ByteString(account.to_bytes()),
                StackValue::Integer(i64::from(scopes.bits())),
                StackValue::Array(vec![StackValue::ByteString(allowed_contract.to_bytes())]),
                StackValue::Array(Vec::new()),
                StackValue::Array(vec![rule.to_stack_value()]),
            ])
        );
    }

    #[test]
    fn signer_stack_item_projection_matches_stack_value_projection() {
        let account = UInt160::from_bytes(&[0x33; UINT160_SIZE]).unwrap();
        let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);

        let expected = StackItem::try_from(signer.to_stack_value()).unwrap();
        assert_eq!(signer.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn signer_deserialize_rejects_global_combined_scope() {
        let mut data = vec![0u8; UINT160_SIZE];
        data.push((WitnessScope::GLOBAL | WitnessScope::CALLED_BY_ENTRY).bits());
        let mut reader = MemoryReader::new(&data);

        assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
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
            let contract = UInt160::from_bytes(&[value as u8; UINT160_SIZE]).unwrap();
            writer.write_serializable(&contract).unwrap();
        }
        let data = writer.into_bytes();
        let mut reader = MemoryReader::new(&data);

        assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
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
        let data = writer.into_bytes();
        let mut reader = MemoryReader::new(&data);

        assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
    }
}
