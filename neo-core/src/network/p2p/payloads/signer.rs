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

use crate::error::CoreError;
use crate::macros::{OptionExt, ValidateLength};
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::IInteroperable;
use crate::witness_rule::{WitnessRule, WitnessRuleAction};
use crate::{
    WitnessCondition, WitnessScope,
    cryptography::{ECCurve, ECPoint},
};
use hex::{decode as hex_decode, encode as hex_encode};
use neo_primitives::{UINT160_SIZE, UInt160};
use neo_vm::StackItem;
use serde::{Deserialize, Serialize};
// Hash and Hasher now provided by impl_hash_for_fields macro
use std::str::FromStr;

// This limits maximum number of AllowedContracts or AllowedGroups
const MAX_SUBITEMS: usize = 16;
const ECPOINT_COMPRESSED_SIZE: usize = 33;

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
}

impl Serializable for Signer {
    fn size(&self) -> usize {
        let mut size = UINT160_SIZE + 1;

        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            size += get_var_size(self.allowed_contracts.len() as u64)
                + self.allowed_contracts.len() * UINT160_SIZE;
        }

        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            size += get_var_size(self.allowed_groups.len() as u64)
                + self
                    .allowed_groups
                    .iter()
                    .map(|g| g.as_bytes().len())
                    .sum::<usize>();
        }

        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            size += get_var_size(self.rules.len() as u64)
                + self.rules.iter().map(|r| r.size()).sum::<usize>();
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
            writer.write_var_uint(self.allowed_contracts.len() as u64)?;
            for contract in &self.allowed_contracts {
                writer.write_serializable(contract)?;
            }
        }

        // Write allowed groups if flag is set
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            self.allowed_groups
                .validate_max_length(MAX_SUBITEMS, "Allowed groups")?;
            writer.write_var_uint(self.allowed_groups.len() as u64)?;
            for group in &self.allowed_groups {
                let encoded = group
                    .encode_point(true)
                    .map_err(|e| IoError::invalid_data(e.to_string()))?;
                if encoded.len() != 33 {
                    return Err(IoError::invalid_data("Group must be compressed"));
                }
                writer.write_bytes(&encoded)?;
            }
        }

        // Write rules if flag is set
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            self.rules.validate_max_length(MAX_SUBITEMS, "Rules")?;
            writer.write_var_uint(self.rules.len() as u64)?;
            for rule in &self.rules {
                writer.write_serializable(rule)?;
            }
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let account = <UInt160 as Serializable>::deserialize(reader)?;
        let scopes_byte = reader.read_u8()?;
        // Use OptionExt trait to reduce boilerplate
        let scopes =
            WitnessScope::from_bits(scopes_byte).ok_or_invalid_data("Invalid witness scope")?;

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
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            allowed_contracts.reserve(count);
            for _ in 0..count {
                allowed_contracts.push(<UInt160 as Serializable>::deserialize(reader)?);
            }
        }

        // Read allowed groups if flag is set
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            allowed_groups.reserve(count);
            for _ in 0..count {
                let encoded = reader.read_bytes(ECPOINT_COMPRESSED_SIZE)?;
                let point = ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &encoded)
                    .map_err(|e| IoError::invalid_data(e.to_string()))?;
                allowed_groups.push(point);
            }
        }

        // Read rules if flag is set
        if scopes.contains(WitnessScope::WITNESS_RULES) {
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            rules.reserve(count);
            for _ in 0..count {
                rules.push(<WitnessRule as Serializable>::deserialize(reader)?);
            }
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

impl IInteroperable for Signer {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), CoreError> {
        // This operation is not supported for Signer.
        // The C# implementation throws NotSupportedException.
        Err(CoreError::InvalidOperation {
            message: "Signer::from_stack_item is not supported".into(),
        })
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        let allowed_contracts = if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            StackItem::from_array(
                self.allowed_contracts
                    .iter()
                    .copied()
                    .map(|hash| StackItem::from_byte_string(hash.to_bytes()))
                    .collect::<Vec<_>>(),
            )
        } else {
            StackItem::from_array(Vec::new())
        };

        let allowed_groups = if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            StackItem::from_array(
                self.allowed_groups
                    .iter()
                    .map(|group| StackItem::from_byte_string(group.to_bytes()))
                    .collect::<Vec<_>>(),
            )
        } else {
            StackItem::from_array(Vec::new())
        };

        let rules = if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            StackItem::from_array(
                self.rules
                    .iter()
                    .map(WitnessRule::to_stack_item)
                    .collect::<Vec<_>>(),
            )
        } else {
            StackItem::from_array(Vec::new())
        };

        Ok(StackItem::from_array(vec![
            StackItem::from_byte_string(self.account.to_bytes()),
            StackItem::from_int(i64::from(self.scopes.bits())),
            allowed_contracts,
            allowed_groups,
            rules,
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

// Use macro to reduce boilerplate for Hash implementation
crate::impl_hash_for_fields!(Signer, account, scopes);
