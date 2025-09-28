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

use super::witness_rule::WitnessRule;
use crate::neo_io::{MemoryReader, Serializable};
use crate::{
    neo_cryptography::ECPoint,
    UInt160,
    WitnessCondition,
    WitnessRuleAction,
    WitnessScope,
};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};

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

    /// Converts all rules contained in the Signer object to WitnessRule.
    /// Matches C# GetAllRules() exactly.
    pub fn get_all_rules(&self) -> Vec<WitnessRule> {
        if self.scopes == WitnessScope::GLOBAL {
            return vec![WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::Boolean { value: true },
            )];
        }

        let mut result = Vec::new();

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
        let mut json = serde_json::json!({
            "account": format!("0x{}", hex::encode(self.account.as_bytes())),
            "scopes": self.scopes.to_string(),
        });

        if let Some(obj) = json.as_object_mut() {
            if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
                obj.insert(
                    "allowedcontracts".to_string(),
                    serde_json::json!(self
                        .allowed_contracts
                        .iter()
                        .map(|c| format!("0x{}", hex::encode(c.as_bytes())))
                        .collect::<Vec<_>>()),
                );
            }

            if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
                obj.insert(
                    "allowedgroups".to_string(),
                    serde_json::json!(self
                        .allowed_groups
                        .iter()
                        .map(|g| format!("0x{}", hex::encode(g.to_vec())))
                        .collect::<Vec<_>>()),
                );
            }

            if self.scopes.contains(WitnessScope::WITNESS_RULES) {
                // TODO: Add rules to JSON when to_json is implemented for WitnessRule
                // obj.insert(
                //     "rules".to_string(),
                //     serde_json::json!(self.rules.iter()
                //         .map(|r| r.to_json())
                //         .collect::<Vec<_>>())
                // );
            }
        }

        json
    }

    /// Creates a signer from a JSON object.
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        // TODO: Implement from_json when all required parsing methods are available
        Err("from_json not yet implemented".to_string())
    }
}

impl Serializable for Signer {
    fn size(&self) -> usize {
        20 + // Account (UInt160)
        1 + // Scopes
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            1 + self.allowed_contracts.len() * 20
        } else { 0 } +
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            1 + self.allowed_groups.len() * 33
        } else { 0 } +
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            1 + self.rules.iter().map(|r| r.size()).sum::<usize>()
        } else { 0 }
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.account.serialize(writer)?;
        writer.write_all(&[self.scopes.bits()])?;

        // Write allowed contracts if flag is set
        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            if self.allowed_contracts.len() > MAX_SUBITEMS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too many allowed contracts",
                ));
            }
            writer.write_all(&[self.allowed_contracts.len() as u8])?;
            for contract in &self.allowed_contracts {
                contract.serialize(writer)?;
            }
        }

        // Write allowed groups if flag is set
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            if self.allowed_groups.len() > MAX_SUBITEMS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too many allowed groups",
                ));
            }
            writer.write_all(&[self.allowed_groups.len() as u8])?;
            for group in &self.allowed_groups {
                group.serialize(writer)?;
            }
        }

        // Write rules if flag is set
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            if self.rules.len() > MAX_SUBITEMS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too many rules",
                ));
            }
            writer.write_all(&[self.rules.len() as u8])?;
            for rule in &self.rules {
                rule.serialize(writer)?;
            }
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let account = UInt160::deserialize(reader)?;
        let scopes_byte = reader.read_u8().map_err(|e| e.to_string())?;
        let scopes = WitnessScope::from_bits(scopes_byte)
            .ok_or_else(|| "Invalid witness scope".to_string())?;

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
            return Err("Invalid witness scope flags".to_string());
        }
        if scopes.contains(WitnessScope::GLOBAL) && scopes != WitnessScope::GLOBAL {
            return Err("Global scope cannot be combined with other scopes".to_string());
        }

        // Read allowed contracts if flag is set
        if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            let count = reader
                .read_var_int(MAX_SUBITEMS as u64)
                .map_err(|e| e.to_string())?;
            if count == 0 || count > MAX_SUBITEMS as u64 {
                return Err("Invalid allowed contracts count".to_string());
            }
            for _ in 0..count {
                allowed_contracts.push(UInt160::deserialize(reader)?);
            }

            // Check for duplicates
            let mut seen = std::collections::HashSet::new();
            for contract in &allowed_contracts {
                if !seen.insert(contract) {
                    return Err("Duplicate allowed contract".to_string());
                }
            }
        }

        // Read allowed groups if flag is set
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            let count = reader
                .read_var_int(MAX_SUBITEMS as u64)
                .map_err(|e| e.to_string())?;
            if count == 0 || count > MAX_SUBITEMS as u64 {
                return Err("Invalid allowed groups count".to_string());
            }
            for _ in 0..count {
                allowed_groups.push(ECPoint::deserialize(reader)?);
            }

            // Check for duplicates
            let mut seen = std::collections::HashSet::new();
            for group in &allowed_groups {
                if !seen.insert(group.clone()) {
                    return Err("Duplicate allowed group".to_string());
                }
            }
        }

        // Read rules if flag is set
        if scopes.contains(WitnessScope::WITNESS_RULES) {
            let count = reader
                .read_var_int(MAX_SUBITEMS as u64)
                .map_err(|e| e.to_string())?;
            if count == 0 || count > MAX_SUBITEMS as u64 {
                return Err("Invalid rules count".to_string());
            }
            for _ in 0..count {
                rules.push(WitnessRule::deserialize(reader)?);
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

impl Hash for Signer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.account.hash(state);
        self.scopes.hash(state);
    }
}
