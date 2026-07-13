use crate::witness_rule::WitnessCondition;
use crate::witness_rule::{WitnessRule, WitnessRuleAction};
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_io::macros::{OptionExt, ValidateLength};
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::hex_util;
use neo_primitives::{UINT160_SIZE, UInt160, WitnessScope};
use neo_vm::StackValue;
use neo_vm::{Interoperable, InteroperableError};
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
                .map(|g| serde_json::Value::String(hex_util::encode_hex(g.as_bytes())))
                .collect();
            json.insert(
                "allowedgroups".to_string(),
                serde_json::Value::Array(groups),
            );
        }

        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            let rules: Vec<_> = self.rules.iter().map(|r| r.to_json()).collect();
            json.insert("rules".to_string(), serde_json::Value::Array(rules));
        }

        serde_json::Value::Object(json)
    }

    /// Creates a signer from a JSON object.
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Signer JSON must be an object"))?;

        let account_str = obj
            .get("account")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Signer.account must be a string"))?;
        let account = UInt160::from_str(account_str)
            .map_err(|e| CoreError::other(format!("Invalid signer account: {e}")))?;

        let scopes_str = obj
            .get("scopes")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Signer.scopes must be a string"))?;
        let scopes = scopes_str
            .parse::<WitnessScope>()
            .map_err(|e| CoreError::other(format!("Invalid witness scope: {e}")))?;

        let mut signer = Self::new(account, scopes);

        if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            let contracts_value = obj.get("allowedcontracts").ok_or_else(|| {
                CoreError::other(
                    "allowedcontracts must be provided when CustomContracts scope is set",
                )
            })?;
            let contracts_array = contracts_value
                .as_array()
                .ok_or_else(|| CoreError::other("allowedcontracts must be an array"))?;
            if contracts_array.len() > MAX_SUBITEMS {
                return Err(CoreError::other("Too many allowed contracts"));
            }

            signer.allowed_contracts = contracts_array
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| CoreError::other("allowedcontracts items must be strings"))
                        .and_then(|s| {
                            UInt160::from_str(s).map_err(|e| {
                                CoreError::other(format!("Invalid allowed contract hash: {e}"))
                            })
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            let groups_value = obj.get("allowedgroups").ok_or_else(|| {
                CoreError::other("allowedgroups must be provided when CustomGroups scope is set")
            })?;
            let groups_array = groups_value
                .as_array()
                .ok_or_else(|| CoreError::other("allowedgroups must be an array"))?;
            if groups_array.len() > MAX_SUBITEMS {
                return Err(CoreError::other("Too many allowed groups"));
            }

            signer.allowed_groups = groups_array
                .iter()
                .map(|value| {
                    let text = value
                        .as_str()
                        .ok_or_else(|| CoreError::other("allowedgroups items must be strings"))?;
                    let bytes = hex_util::decode_hex(text)
                        .map_err(|e| CoreError::other(format!("Invalid ECPoint hex: {e}")))?;
                    ECPoint::from_bytes(&bytes)
                        .map_err(|e| CoreError::other(format!("Invalid ECPoint: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        if scopes.contains(WitnessScope::WITNESS_RULES) {
            let rules_value = obj.get("rules").ok_or_else(|| {
                CoreError::other("rules must be provided when WitnessRules scope is set")
            })?;
            let rules_array = rules_value
                .as_array()
                .ok_or_else(|| CoreError::other("rules must be an array"))?;
            if rules_array.len() > MAX_SUBITEMS {
                return Err(CoreError::other("Too many witness rules"));
            }

            signer.rules = rules_array
                .iter()
                .map(|value| {
                    WitnessRule::from_json(value)
                        .map_err(|e| CoreError::other(format!("Invalid witness rule: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        Ok(signer)
    }

    /// Converts the signer to a neo-vm stack value (matches C# `Signer.ToStackItem` layout).
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

        StackValue::Array(
            neo_vm::next_stack_item_id(),
            vec![
                StackValue::ByteString(self.account.to_bytes()),
                StackValue::Integer(i64::from(self.scopes.bits())),
                StackValue::Array(neo_vm::next_stack_item_id(), allowed_contracts),
                StackValue::Array(neo_vm::next_stack_item_id(), allowed_groups),
                StackValue::Array(neo_vm::next_stack_item_id(), rules),
            ],
        )
    }
}

impl Serializable for Signer {
    fn size(&self) -> usize {
        let mut size = UINT160_SIZE + 1;

        if self.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
            size += SerializeHelper::get_var_size_serializable_slice(&self.allowed_contracts);
        }

        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            size += SerializeHelper::get_var_size_for_slice(&self.allowed_groups, |group| {
                group.as_bytes().len()
            });
        }

        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            size += SerializeHelper::get_var_size_serializable_slice(&self.rules);
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
            SerializeHelper::serialize_array(&self.allowed_contracts, writer)?;
        }

        // Write allowed groups if flag is set
        if self.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            self.allowed_groups
                .validate_max_length(MAX_SUBITEMS, "Allowed groups")?;
            SerializeHelper::serialize_array_with(
                &self.allowed_groups,
                writer,
                |group, writer| {
                    let encoded = group
                        .encode_point(true)
                        .map_err(|e| IoError::invalid_data(e.to_string()))?;
                    if encoded.len() != 33 {
                        return Err(IoError::invalid_data("Group must be compressed"));
                    }
                    writer.write_bytes(&encoded)
                },
            )?;
        }

        // Write rules if flag is set
        if self.scopes.contains(WitnessScope::WITNESS_RULES) {
            self.rules.validate_max_length(MAX_SUBITEMS, "Rules")?;
            SerializeHelper::serialize_array(&self.rules, writer)?;
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
            allowed_contracts = SerializeHelper::deserialize_array(reader, MAX_SUBITEMS)?;
        }

        // Read allowed groups if flag is set
        if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
            allowed_groups =
                SerializeHelper::deserialize_array_with(reader, MAX_SUBITEMS, |reader| {
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
            rules = SerializeHelper::deserialize_array(reader, MAX_SUBITEMS)?;
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
    fn from_stack_value(&mut self, _value: StackValue) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "Signer::from_stack_value is not supported".into(),
        ))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }
}

// Use macro to reduce boilerplate for Hash implementation
neo_io::impl_hash_for_fields!(Signer, account, scopes);

#[cfg(test)]
#[path = "../tests/signing/signer.rs"]
mod tests;
