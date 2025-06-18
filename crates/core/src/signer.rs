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

//! Implementation of Signer for Neo blockchain.

use std::fmt;
use serde::{Deserialize, Serialize};
use crate::{UInt160, WitnessScope, WitnessRule};
use neo_io::{Serializable, BinaryWriter, MemoryReader};
use crate::witness_rule::{WitnessRuleAction, WitnessCondition, WitnessConditionType};

/// Maximum number of allowed contracts or groups
const MAX_SUBITEMS: usize = 16;

/// Represents a signer of a transaction.
///
/// A signer defines who is signing the transaction and what scope
/// their signature covers (which contracts they authorize).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signer {
    /// The account of the signer.
    pub account: UInt160,

    /// The scopes of the witness.
    pub scopes: WitnessScope,

    /// The contracts that are allowed by the witness.
    /// Only available when the CustomContracts flag is set.
    pub allowed_contracts: Vec<UInt160>,

    /// The groups that are allowed by the witness (matches C# AllowedGroups exactly).
    /// Only available when the CustomGroups flag is set.
    /// Each Vec<u8> represents an ECPoint serialized as bytes.
    pub allowed_groups: Vec<Vec<u8>>, // ECPoint[] in C# serialized as bytes

    /// The rules that the witness must meet (matches C# Rules exactly).
    /// Only available when the WitnessRules flag is set.
    pub rules: Vec<WitnessRule>,
}

impl Signer {

    /// Creates a new Signer instance with the specified scope.
    ///
    /// # Arguments
    ///
    /// * `account` - The account of the signer
    /// * `scopes` - The witness scopes
    ///
    /// # Returns
    ///
    /// A new Signer instance
    pub fn with_scope(account: UInt160, scopes: WitnessScope) -> Self {
        Self {
            account,
            scopes,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Creates a new Signer with the specified scope.
    ///
    /// # Arguments
    ///
    /// * `account` - The account of the signer
    /// * `scopes` - The witness scopes
    ///
    /// # Returns
    ///
    /// A new Signer instance
    pub fn new_with_scope(account: UInt160, scopes: WitnessScope) -> Self {
        Self {
            account,
            scopes,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Gets the size of the signer in bytes after serialization.
    ///
    /// # Returns
    ///
    /// The size in bytes
    pub fn get_size(&self) -> usize {
        let mut size = 20 + 1; // UInt160 (20 bytes) + WitnessScope (1 byte)

        if self.scopes.has_flag(WitnessScope::CustomContracts) {
            size += self.get_var_size_for_array(&self.allowed_contracts);
        }

        if self.scopes.has_flag(WitnessScope::CustomGroups) {
            size += self.get_var_size_for_groups(&self.allowed_groups);
        }

        if self.scopes.has_flag(WitnessScope::WitnessRules) {
            size += self.get_var_size_for_rules(&self.rules);
        }

        size
    }

    /// Helper function to calculate variable size for UInt160 array
    fn get_var_size_for_array(&self, array: &[UInt160]) -> usize {
        let var_int_size = if array.len() < 0xFD {
            1
        } else if array.len() <= 0xFFFF {
            3
        } else {
            5
        };
        var_int_size + (array.len() * 20) // Each UInt160 is 20 bytes
    }

    /// Helper function to calculate variable size for groups array
    fn get_var_size_for_groups(&self, groups: &[Vec<u8>]) -> usize {
        let var_int_size = if groups.len() < 0xFD {
            1
        } else if groups.len() <= 0xFFFF {
            3
        } else {
            5
        };
        let data_size: usize = groups.iter().map(|g| g.len()).sum();
        var_int_size + data_size
    }

    /// Helper function to calculate variable size for rules array
    fn get_var_size_for_rules(&self, rules: &[WitnessRule]) -> usize {
        let var_int_size = if rules.len() < 0xFD {
            1
        } else if rules.len() <= 0xFFFF {
            3
        } else {
            5
        };
        let data_size: usize = rules.iter().map(|r| 1 + r.condition.len()).sum(); // action (1 byte) + condition
        var_int_size + data_size
    }

    /// Adds an allowed contract to the signer.
    ///
    /// # Arguments
    ///
    /// * `contract` - The contract hash to allow
    pub fn add_allowed_contract(&mut self, contract: UInt160) {
        if !self.allowed_contracts.contains(&contract) {
            self.allowed_contracts.push(contract);
        }
        // Ensure CustomContracts scope is set
        self.scopes = self.scopes.combine(WitnessScope::CustomContracts);
    }

    /// Adds an allowed group to the signer.
    ///
    /// # Arguments
    ///
    /// * `group` - The group public key to allow
    pub fn add_allowed_group(&mut self, group: Vec<u8>) {
        if !self.allowed_groups.contains(&group) {
            self.allowed_groups.push(group);
        }
        // Ensure CustomGroups scope is set
        self.scopes = self.scopes.combine(WitnessScope::CustomGroups);
    }

    /// Adds a witness rule to the signer.
    ///
    /// # Arguments
    ///
    /// * `rule` - The witness rule to add
    pub fn add_rule(&mut self, rule: WitnessRule) {
        self.rules.push(rule);
        // Ensure WitnessRules scope is set
        self.scopes = self.scopes.combine(WitnessScope::WitnessRules);
    }

    /// Gets all witness rules for this signer.
    ///
    /// # Returns
    ///
    /// A slice of all witness rules
    pub fn get_all_rules(&self) -> &[WitnessRule] {
        &self.rules
    }
}

impl Default for Signer {
    fn default() -> Self {
        Self::new_default(UInt160::new())
    }
}

// Additional constructor for compatibility with tests
impl Signer {
    /// Creates a new Signer instance with account and scope.
    /// This is for compatibility with existing test code.
    ///
    /// # Arguments
    ///
    /// * `account` - The account of the signer
    /// * `scopes` - The witness scopes
    ///
    /// # Returns
    ///
    /// A new Signer instance
    pub fn new(account: UInt160, scopes: WitnessScope) -> Self {
        Self {
            account,
            scopes,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Creates a new Signer instance with just the account (default scope).
    ///
    /// # Arguments
    ///
    /// * `account` - The account of the signer
    ///
    /// # Returns
    ///
    /// A new Signer instance
    pub fn new_default(account: UInt160) -> Self {
        Self {
            account,
            scopes: WitnessScope::None,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }
}

impl Serializable for Signer {
    fn size(&self) -> usize {
        let mut size = 20 + 1; // UInt160 (20 bytes) + WitnessScope (1 byte)

        if self.scopes.has_flag(WitnessScope::CustomContracts) {
            size += self.get_var_size_for_array(&self.allowed_contracts);
        }

        if self.scopes.has_flag(WitnessScope::CustomGroups) {
            size += self.get_var_size_for_groups(&self.allowed_groups);
        }

        if self.scopes.has_flag(WitnessScope::WitnessRules) {
            size += self.get_var_size_for_rules(&self.rules);
        }

        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), neo_io::Error> {
        // Write account (UInt160)
        <UInt160 as neo_io::Serializable>::serialize(&self.account, writer)?;

        // Write scopes
        writer.write_bytes(&[self.scopes.to_byte()])?;

        // Write allowed contracts if CustomContracts scope is set
        if self.scopes.has_flag(WitnessScope::CustomContracts) {
            writer.write_var_int(self.allowed_contracts.len() as u64)?;
            for contract in &self.allowed_contracts {
                <UInt160 as neo_io::Serializable>::serialize(contract, writer)?;
            }
        }

        // Write allowed groups if CustomGroups scope is set
        if self.scopes.has_flag(WitnessScope::CustomGroups) {
            writer.write_var_int(self.allowed_groups.len() as u64)?;
            for group in &self.allowed_groups {
                writer.write_bytes(group)?;
            }
        }

        // Write rules if WitnessRules scope is set
        if self.scopes.has_flag(WitnessScope::WitnessRules) {
            writer.write_var_int(self.rules.len() as u64)?;
            for rule in &self.rules {
                writer.write_bytes(&[rule.action as u8])?;
                self.serialize_condition(writer, &rule.condition)?;
            }
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, neo_io::Error> {
        // Read account (UInt160)
        let account = <UInt160 as neo_io::Serializable>::deserialize(reader)?;

        // Read scopes
        let scope_bytes = reader.read_bytes(1)?;
        let scope_byte = scope_bytes[0];
        let scopes = WitnessScope::from_byte(scope_byte).unwrap_or(WitnessScope::None);

        // Validate scopes
        if !scopes.is_valid() {
            return Err(neo_io::Error::InvalidData(format!("Invalid witness scope: {}", scope_byte)));
        }

        let mut allowed_contracts = Vec::new();
        let mut allowed_groups = Vec::new();
        let mut rules = Vec::new();

        // Read allowed contracts if CustomContracts scope is set
        if scopes.has_flag(WitnessScope::CustomContracts) {
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            if count > MAX_SUBITEMS {
                return Err(neo_io::Error::InvalidData(format!("Too many allowed contracts: {}", count)));
            }
            allowed_contracts = Vec::with_capacity(count);
            for _ in 0..count {
                let contract = <UInt160 as neo_io::Serializable>::deserialize(reader)?;
                allowed_contracts.push(contract);
            }
        }

        // Read allowed groups if CustomGroups scope is set
        if scopes.has_flag(WitnessScope::CustomGroups) {
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            if count > MAX_SUBITEMS {
                return Err(neo_io::Error::InvalidData(format!("Too many allowed groups: {}", count)));
            }
            allowed_groups = Vec::with_capacity(count);
            for _ in 0..count {
                // Production-ready group deserialization (matches C# ECPoint.DecodePoint exactly)
                // This implements the C# logic: reading variable-length ECPoint data
                
                // 1. Read the first byte to determine the ECPoint format (production format detection)
                let format_byte = reader.read_bytes(1)?[0];
                
                // 2. Deserialize based on ECPoint format (matches C# ECPoint format exactly)
                let group = match format_byte {
                    0x02 | 0x03 => {
                        // Compressed public key format (33 bytes total: 1 format + 32 data)
                        let mut key_data = vec![format_byte];
                        key_data.extend_from_slice(&reader.read_bytes(32)?);
                        key_data
                    }
                    0x04 => {
                        // Uncompressed public key format (65 bytes total: 1 format + 64 data)  
                        let mut key_data = vec![format_byte];
                        key_data.extend_from_slice(&reader.read_bytes(64)?);
                        key_data
                    }
                    _ => {
                        // Invalid ECPoint format (production validation)
                        return Err(neo_io::Error::InvalidData(format!("Invalid ECPoint format: 0x{:02x}", format_byte)));
                    }
                };
                
                // 3. Validate ECPoint format (production validation)
                if group.len() != 33 && group.len() != 65 {
                    return Err(neo_io::Error::InvalidData(format!("Invalid ECPoint length: {}", group.len())));
                }
                
                allowed_groups.push(group);
            }
        }

        // Read rules if WitnessRules scope is set
        if scopes.has_flag(WitnessScope::WitnessRules) {
            let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
            if count > MAX_SUBITEMS {
                return Err(neo_io::Error::InvalidData(format!("Too many rules: {}", count)));
            }
            rules = Vec::with_capacity(count);
            for _ in 0..count {
                let action_bytes = reader.read_bytes(1)?;
                let action_byte = action_bytes[0];
                let action = match action_byte {
                    0x00 => WitnessRuleAction::Deny,
                    0x01 => WitnessRuleAction::Allow,
                    _ => return Err(neo_io::Error::InvalidData(format!("Invalid witness rule action: {}", action_byte))),
                };
                
                // Production-ready witness condition deserialization (matches C# WitnessCondition.Deserialize exactly)
                let condition = Self::deserialize_condition(reader)?;
                rules.push(WitnessRule { action, condition });
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

impl Signer {

    /// Helper method to serialize witness conditions (matches C# WitnessCondition.Serialize exactly)
    fn serialize_condition(&self, writer: &mut BinaryWriter, condition: &WitnessCondition) -> Result<(), neo_io::Error> {
        match condition {
            WitnessCondition::Boolean { value } => {
                writer.write_bytes(&[WitnessConditionType::Boolean as u8])?;
                writer.write_bytes(&[if *value { 1 } else { 0 }])?;
            }
            WitnessCondition::Not { condition } => {
                writer.write_bytes(&[WitnessConditionType::Not as u8])?;
                self.serialize_condition(writer, condition)?;
            }
            WitnessCondition::And { conditions } => {
                writer.write_bytes(&[WitnessConditionType::And as u8])?;
                writer.write_var_int(conditions.len() as u64)?;
                for expr in conditions {
                    self.serialize_condition(writer, expr)?;
                }
            }
            WitnessCondition::Or { conditions } => {
                writer.write_bytes(&[WitnessConditionType::Or as u8])?;
                writer.write_var_int(conditions.len() as u64)?;
                for expr in conditions {
                    self.serialize_condition(writer, expr)?;
                }
            }
            WitnessCondition::ScriptHash { hash } => {
                writer.write_bytes(&[WitnessConditionType::ScriptHash as u8])?;
                writer.write_bytes(hash.as_bytes())?;
            }
            WitnessCondition::Group { group } => {
                writer.write_bytes(&[WitnessConditionType::Group as u8])?;
                writer.write_var_bytes(group)?;
            }
            WitnessCondition::CalledByEntry => {
                writer.write_bytes(&[WitnessConditionType::CalledByEntry as u8])?;
            }
            WitnessCondition::CalledByContract { hash } => {
                writer.write_bytes(&[WitnessConditionType::CalledByContract as u8])?;
                writer.write_bytes(hash.as_bytes())?;
            }
            WitnessCondition::CalledByGroup { group } => {
                writer.write_bytes(&[WitnessConditionType::CalledByGroup as u8])?;
                writer.write_var_bytes(group)?;
            }
        }
        Ok(())
    }

    /// Helper method to deserialize witness conditions (matches C# WitnessCondition.Deserialize exactly)
    fn deserialize_condition(reader: &mut MemoryReader) -> Result<WitnessCondition, neo_io::Error> {
        // Production-ready witness condition deserialization (matches C# WitnessCondition.Deserialize exactly)
        // This implements the C# logic: WitnessCondition.Deserialize with full type support
        
        // 1. Read condition type byte (production type detection)
        let condition_type_byte = reader.read_bytes(1)?[0];
        
        // 2. Parse condition based on type (matches C# WitnessConditionType exactly)
        match condition_type_byte {
            0x00 => {
                // Boolean condition (matches C# WitnessConditionType.Boolean)
                let value_byte = reader.read_bytes(1)?[0];
                let value = value_byte != 0;
                Ok(WitnessCondition::Boolean { value })
            }
            0x01 => {
                // Not condition (matches C# WitnessConditionType.Not)
                let inner_condition = Box::new(Self::deserialize_condition(reader)?);
                Ok(WitnessCondition::Not { condition: inner_condition })
            }
            0x02 => {
                // And condition (matches C# WitnessConditionType.And)
                let count = reader.read_var_int(16)? as usize; // Max 16 conditions
                let mut conditions = Vec::with_capacity(count);
                for _ in 0..count {
                    conditions.push(Self::deserialize_condition(reader)?);
                }
                Ok(WitnessCondition::And { conditions })
            }
            0x03 => {
                // Or condition (matches C# WitnessConditionType.Or)
                let count = reader.read_var_int(16)? as usize; // Max 16 conditions
                let mut conditions = Vec::with_capacity(count);
                for _ in 0..count {
                    conditions.push(Self::deserialize_condition(reader)?);
                }
                Ok(WitnessCondition::Or { conditions })
            }
            0x18 => {
                // ScriptHash condition (matches C# WitnessConditionType.ScriptHash)
                let hash_bytes = reader.read_bytes(20)?;
                let hash = UInt160::from_bytes(&hash_bytes)
                    .map_err(|_| neo_io::Error::InvalidData("Invalid script hash in witness condition".to_string()))?;
                Ok(WitnessCondition::ScriptHash { hash })
            }
            0x19 => {
                // Group condition (matches C# WitnessConditionType.Group)
                let group_data = reader.read_var_bytes(33)?; // Max 33 bytes for compressed ECPoint
                Ok(WitnessCondition::Group { group: group_data })
            }
            0x20 => {
                // CalledByEntry condition (matches C# WitnessConditionType.CalledByEntry)
                Ok(WitnessCondition::CalledByEntry)
            }
            0x28 => {
                // CalledByContract condition (matches C# WitnessConditionType.CalledByContract)
                let hash_bytes = reader.read_bytes(20)?;
                let hash = UInt160::from_bytes(&hash_bytes)
                    .map_err(|_| neo_io::Error::InvalidData("Invalid contract hash in witness condition".to_string()))?;
                Ok(WitnessCondition::CalledByContract { hash })
            }
            0x29 => {
                // CalledByGroup condition (matches C# WitnessConditionType.CalledByGroup)
                let group_data = reader.read_var_bytes(33)?; // Max 33 bytes for compressed ECPoint
                Ok(WitnessCondition::CalledByGroup { group: group_data })
            }
            _ => {
                // Invalid condition type (production error handling)
                Err(neo_io::Error::InvalidData(format!("Invalid witness condition type: 0x{:02x}", condition_type_byte)))
            }
        }
    }
}

impl fmt::Display for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Signer {{ account: {}, scopes: {} }}",
            self.account,
            self.scopes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signer_new() {
        let account = UInt160::new();
        let signer = Signer::new(account.clone(), WitnessScope::None);

        assert_eq!(signer.account, account);
        assert_eq!(signer.scopes, WitnessScope::None);
        assert!(signer.allowed_contracts.is_empty());
        assert!(signer.allowed_groups.is_empty());
        assert!(signer.rules.is_empty());
    }

    #[test]
    fn test_signer_new_with_scope() {
        let account = UInt160::new();
        let scope = WitnessScope::CalledByEntry;
        let signer = Signer::new_with_scope(account.clone(), scope);

        assert_eq!(signer.account, account);
        assert_eq!(signer.scopes, scope);
    }

    #[test]
    fn test_signer_add_allowed_contract() {
        let mut signer = Signer::new(UInt160::new(), WitnessScope::None);
        let contract = UInt160::new();

        signer.add_allowed_contract(contract.clone());

        assert_eq!(signer.allowed_contracts.len(), 1);
        assert_eq!(signer.allowed_contracts[0], contract);
        assert!(signer.scopes.has_flag(WitnessScope::CustomContracts));
    }

    #[test]
    fn test_signer_add_allowed_group() {
        let mut signer = Signer::new(UInt160::new(), WitnessScope::None);
        let group = vec![1, 2, 3];

        signer.add_allowed_group(group.clone());

        assert_eq!(signer.allowed_groups.len(), 1);
        assert_eq!(signer.allowed_groups[0], group);
        assert!(signer.scopes.has_flag(WitnessScope::CustomGroups));
    }

    #[test]
    fn test_signer_add_rule() {
        let mut signer = Signer::new(UInt160::new(), WitnessScope::None);
        let rule = WitnessRule {
            action: WitnessRuleAction::Allow,
            condition: WitnessCondition::Boolean { value: true },
        };

        signer.add_rule(rule.clone());

        assert_eq!(signer.rules.len(), 1);
        assert_eq!(signer.rules[0], rule);
        assert!(signer.scopes.has_flag(WitnessScope::WitnessRules));
    }

    #[test]
    fn test_signer_size() {
        let signer = Signer::new(UInt160::new(), WitnessScope::None);
        let size = signer.get_size();

        // UInt160 (20 bytes) + WitnessScope (1 byte) = 21 bytes minimum
        assert_eq!(size, 21);
    }

    #[test]
    fn test_signer_serialization() {
        let mut signer = Signer::new(UInt160::new(), WitnessScope::None);
        signer.scopes = WitnessScope::CalledByEntry;

        let mut writer = BinaryWriter::new();
        <Signer as Serializable>::serialize(&signer, &mut writer).unwrap();

        let mut reader = MemoryReader::new(&writer.to_bytes());
        let deserialized = <Signer as Serializable>::deserialize(&mut reader).unwrap();

        assert_eq!(signer.account, deserialized.account);
        assert_eq!(signer.scopes, deserialized.scopes);
    }
}
