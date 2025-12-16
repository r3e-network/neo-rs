// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! Implementation of WitnessRule (matches C# WitnessRule exactly).

use crate::neo_config::ADDRESS_SIZE;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::{ECCurve, ECPoint};
use crate::UInt160;
use hex::{decode as hex_decode, encode as hex_encode};
use neo_vm::StackItem;
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use std::fmt;
use std::str::FromStr;

const ECPOINT_COMPRESSED_SIZE: usize = 33;
const ECPOINT_UNCOMPRESSED_SIZE: usize = 65;

fn strip_0x(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

fn encode_hex(bytes: &[u8]) -> String {
    hex_encode(bytes)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    hex_decode(strip_0x(value)).map_err(|e| format!("Invalid hex string: {e}"))
}

fn parse_group_bytes(value: &str) -> Result<Vec<u8>, String> {
    let bytes = decode_hex(value)?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1())
        .map_err(|e| format!("Invalid ECPoint: {e}"))?;
    point
        .encode_point(true)
        .map_err(|e| format!("Failed to encode ECPoint: {e}"))
}

fn read_group_bytes(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
    let prefix = reader.peek()?;
    let encoded_len = match prefix {
        0x02 | 0x03 => ECPOINT_COMPRESSED_SIZE,
        0x04 => ECPOINT_UNCOMPRESSED_SIZE,
        _ => {
            return Err(IoError::invalid_data(
                "Invalid ECPoint encoding prefix for witness group",
            ));
        }
    };
    let bytes = reader.read_bytes(encoded_len)?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1()).map_err(IoError::invalid_data)?;
    point.encode_point(true).map_err(IoError::invalid_data)
}

/// The action to be taken if the current context meets with the rule (matches C# WitnessRuleAction exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessRuleAction {
    /// Deny the witness if the condition is met.
    Deny = 0,
    /// Allow the witness if the condition is met.
    Allow = 1,
}

impl WitnessRuleAction {
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Deny),
            1 => Some(Self::Allow),
            _ => None,
        }
    }
}

impl Serialize for WitnessRuleAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessRuleAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessRuleAction::from_byte(byte)
            .ok_or_else(|| D::Error::custom(format!("Invalid witness rule action byte: {byte}")))
    }
}

impl FromStr for WitnessRuleAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Deny" | "deny" => Ok(Self::Deny),
            "Allow" | "allow" => Ok(Self::Allow),
            other => Err(format!("Invalid witness rule action: {other}")),
        }
    }
}

/// The type of witness condition (matches C# WitnessConditionType exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessConditionType {
    /// Boolean condition.
    Boolean = 0x00,
    /// Not condition (logical NOT).
    Not = 0x01,
    /// And condition (logical AND).
    And = 0x02,
    /// Or condition (logical OR).
    Or = 0x03,
    /// Script hash condition.
    ScriptHash = 0x18,
    /// Group condition.
    Group = 0x19,
    /// Called by entry condition.
    CalledByEntry = 0x20,
    /// Called by contract condition.
    CalledByContract = 0x28,
    /// Called by group condition.
    CalledByGroup = 0x29,
}

impl WitnessConditionType {
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Boolean),
            0x01 => Some(Self::Not),
            0x02 => Some(Self::And),
            0x03 => Some(Self::Or),
            0x18 => Some(Self::ScriptHash),
            0x19 => Some(Self::Group),
            0x20 => Some(Self::CalledByEntry),
            0x28 => Some(Self::CalledByContract),
            0x29 => Some(Self::CalledByGroup),
            _ => None,
        }
    }
}

impl Serialize for WitnessConditionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessConditionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessConditionType::from_byte(byte)
            .ok_or_else(|| D::Error::custom(format!("Invalid witness condition type byte: {byte}")))
    }
}

/// Represents a witness condition (matches C# WitnessCondition exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessCondition {
    /// Boolean condition with a fixed value.
    Boolean { value: bool },
    /// Not condition that negates another condition.
    Not { condition: Box<WitnessCondition> },
    /// And condition that requires all sub-conditions to be true.
    And { conditions: Vec<WitnessCondition> },
    /// Or condition that requires at least one sub-condition to be true.
    Or { conditions: Vec<WitnessCondition> },
    /// Script hash condition that checks if the current script hash matches.
    ScriptHash { hash: crate::UInt160 },
    /// Group condition that checks if the current group matches.
    Group { group: Vec<u8> }, // ECPoint serialized as bytes (matches C# ECPoint exactly)
    /// Called by entry condition.
    CalledByEntry,
    /// Called by contract condition that checks if called by a specific contract.
    CalledByContract { hash: crate::UInt160 },
    /// Called by group condition that checks if called by a specific group.
    CalledByGroup { group: Vec<u8> }, // ECPoint serialized as bytes (matches C# ECPoint exactly)
}

/// The rule used to describe the scope of the witness (matches C# WitnessRule exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessRule {
    /// Indicates the action to be taken if the current context meets with the rule.
    pub action: WitnessRuleAction,
    /// The condition of the rule.
    pub condition: WitnessCondition,
}

impl WitnessCondition {
    /// Maximum number of sub-items allowed (matches C# MaxSubitems exactly).
    pub const MAX_SUBITEMS: usize = 16;
    /// Maximum nesting depth allowed (matches C# MaxNestingDepth exactly).
    pub const MAX_NESTING_DEPTH: usize = 3;

    /// Gets the type of the condition (matches C# Type property exactly).
    pub fn condition_type(&self) -> WitnessConditionType {
        match self {
            WitnessCondition::Boolean { .. } => WitnessConditionType::Boolean,
            WitnessCondition::Not { .. } => WitnessConditionType::Not,
            WitnessCondition::And { .. } => WitnessConditionType::And,
            WitnessCondition::Or { .. } => WitnessConditionType::Or,
            WitnessCondition::ScriptHash { .. } => WitnessConditionType::ScriptHash,
            WitnessCondition::Group { .. } => WitnessConditionType::Group,
            WitnessCondition::CalledByEntry => WitnessConditionType::CalledByEntry,
            WitnessCondition::CalledByContract { .. } => WitnessConditionType::CalledByContract,
            WitnessCondition::CalledByGroup { .. } => WitnessConditionType::CalledByGroup,
        }
    }

    /// Validates the condition structure (matches C# validation exactly).
    pub fn is_valid(&self, max_depth: usize) -> bool {
        if max_depth == 0 {
            return false;
        }

        match self {
            WitnessCondition::Boolean { .. } => true,
            WitnessCondition::CalledByEntry => true,
            WitnessCondition::ScriptHash { .. } => true,
            WitnessCondition::Group { .. } => true,
            WitnessCondition::CalledByContract { .. } => true,
            WitnessCondition::CalledByGroup { .. } => true,
            WitnessCondition::Not { condition } => condition.is_valid(max_depth - 1),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                if conditions.is_empty() || conditions.len() > Self::MAX_SUBITEMS {
                    return false;
                }
                conditions.iter().all(|c| c.is_valid(max_depth - 1))
            }
        }
    }

    /// Calculates the size of the condition when serialized (matches C# Size property exactly).
    pub fn size(&self) -> usize {
        let payload = match self {
            WitnessCondition::Boolean { .. } => 1, // bool
            WitnessCondition::Not { condition } => condition.size(),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                get_var_size(conditions.len() as u64)
                    + conditions.iter().map(|c| c.size()).sum::<usize>()
            }
            WitnessCondition::ScriptHash { .. } => ADDRESS_SIZE,
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => group.len(),
            WitnessCondition::CalledByEntry => 0,
            WitnessCondition::CalledByContract { .. } => ADDRESS_SIZE,
        };
        1 + payload
    }

    /// Calculates the size of the condition when serialized (matches earlier `len` helper`).
    pub fn len(&self) -> usize {
        self.size()
    }

    /// Returns true if the condition has zero size when serialized
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    pub fn to_json(&self) -> Value {
        match self {
            WitnessCondition::Boolean { value } => json!({
                "type": "Boolean",
                "expression": value,
            }),
            WitnessCondition::Not { condition } => json!({
                "type": "Not",
                "expression": condition.to_json(),
            }),
            WitnessCondition::And { conditions } => json!({
                "type": "And",
                "expressions": conditions.iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            }),
            WitnessCondition::Or { conditions } => json!({
                "type": "Or",
                "expressions": conditions.iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            }),
            WitnessCondition::ScriptHash { hash } => json!({
                "type": "ScriptHash",
                "hash": hash.to_string(),
            }),
            WitnessCondition::Group { group } => json!({
                "type": "Group",
                "group": encode_hex(group),
            }),
            WitnessCondition::CalledByEntry => json!({
                "type": "CalledByEntry",
            }),
            WitnessCondition::CalledByContract { hash } => json!({
                "type": "CalledByContract",
                "hash": hash.to_string(),
            }),
            WitnessCondition::CalledByGroup { group } => json!({
                "type": "CalledByGroup",
                "group": encode_hex(group),
            }),
        }
    }

    pub fn from_json(json: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(json, Self::MAX_NESTING_DEPTH)
    }

    pub fn from_json_with_depth(json: &Value, max_depth: usize) -> Result<Self, String> {
        if max_depth == 0 {
            return Err("Max nesting depth exceeded".to_string());
        }

        let condition_type = json
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "Condition type missing".to_string())?;
        match condition_type {
            "Boolean" => {
                let value = json
                    .get("expression")
                    .and_then(Value::as_bool)
                    .or_else(|| json.get("value").and_then(Value::as_bool))
                    .ok_or_else(|| "Boolean condition missing expression".to_string())?;
                Ok(WitnessCondition::Boolean { value })
            }
            "Not" => {
                let expression = json
                    .get("expression")
                    .ok_or_else(|| "Not condition missing expression".to_string())?;
                let inner = WitnessCondition::from_json_with_depth(expression, max_depth - 1)?;
                Ok(WitnessCondition::Not {
                    condition: Box::new(inner),
                })
            }
            "And" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "And condition missing expressions".to_string())?;
                if expressions.is_empty() {
                    return Err(
                        "Composite witness condition requires at least one expression".to_string(),
                    );
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err("Composite witness condition exceeds max subitems".to_string());
                }
                let mut conditions = Vec::with_capacity(expressions.len());
                for expr in expressions {
                    conditions.push(WitnessCondition::from_json_with_depth(expr, max_depth - 1)?);
                }
                Ok(WitnessCondition::And { conditions })
            }
            "Or" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "Or condition missing expressions".to_string())?;
                if expressions.is_empty() {
                    return Err(
                        "Composite witness condition requires at least one expression".to_string(),
                    );
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err("Composite witness condition exceeds max subitems".to_string());
                }
                let mut conditions = Vec::with_capacity(expressions.len());
                for expr in expressions {
                    conditions.push(WitnessCondition::from_json_with_depth(expr, max_depth - 1)?);
                }
                Ok(WitnessCondition::Or { conditions })
            }
            "ScriptHash" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "ScriptHash condition missing hash".to_string())?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| format!("Invalid script hash: {e}"))?;
                Ok(WitnessCondition::ScriptHash { hash })
            }
            "Group" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Group condition missing group".to_string())?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::Group { group })
            }
            "CalledByEntry" => Ok(WitnessCondition::CalledByEntry),
            "CalledByContract" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CalledByContract missing hash".to_string())?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| format!("Invalid script hash: {e}"))?;
                Ok(WitnessCondition::CalledByContract { hash })
            }
            "CalledByGroup" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CalledByGroup missing group".to_string())?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::CalledByGroup { group })
            }
            other => Err(format!("Unsupported witness condition type: {other}")),
        }
    }

    /// Converts the witness condition to a VM stack item (matches C# `WitnessCondition.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        let mut items = vec![StackItem::from_int(i64::from(
            self.condition_type().to_byte(),
        ))];

        match self {
            WitnessCondition::Boolean { value } => {
                items.push(StackItem::from_bool(*value));
            }
            WitnessCondition::Not { condition } => {
                items.push(condition.to_stack_item());
            }
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                let expressions = conditions
                    .iter()
                    .map(WitnessCondition::to_stack_item)
                    .collect::<Vec<_>>();
                items.push(StackItem::from_array(expressions));
            }
            WitnessCondition::ScriptHash { hash } | WitnessCondition::CalledByContract { hash } => {
                items.push(StackItem::from_byte_string(hash.to_bytes()));
            }
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                items.push(StackItem::from_byte_string(group.clone()));
            }
            WitnessCondition::CalledByEntry => {}
        }

        StackItem::from_array(items)
    }
}

impl WitnessRule {
    /// Creates a new WitnessRule (matches C# constructor exactly).
    pub fn new(action: WitnessRuleAction, condition: WitnessCondition) -> Self {
        Self { action, condition }
    }

    /// Validates the rule (matches C# validation exactly).
    pub fn is_valid(&self) -> bool {
        self.condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH)
    }

    pub fn size(&self) -> usize {
        1 + self.condition.size()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "action": self.action.to_string(),
            "condition": self.condition.to_json(),
        })
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(value, WitnessCondition::MAX_NESTING_DEPTH)
    }

    pub fn from_json_with_depth(value: &Value, max_depth: usize) -> Result<Self, String> {
        let action_str = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "WitnessRule missing action".to_string())?;
        let action: WitnessRuleAction = action_str.parse()?;
        let condition_value = value
            .get("condition")
            .ok_or_else(|| "WitnessRule missing condition".to_string())?;
        let condition = WitnessCondition::from_json_with_depth(condition_value, max_depth)?;
        Ok(Self { action, condition })
    }

    /// Converts the witness rule to a VM stack item (matches C# `WitnessRule.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_array(vec![
            StackItem::from_int(i64::from(self.action.to_byte())),
            self.condition.to_stack_item(),
        ])
    }
}

impl fmt::Display for WitnessRuleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WitnessRuleAction::Deny => write!(f, "Deny"),
            WitnessRuleAction::Allow => write!(f, "Allow"),
        }
    }
}

impl fmt::Display for WitnessConditionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WitnessConditionType::Boolean => write!(f, "Boolean"),
            WitnessConditionType::Not => write!(f, "Not"),
            WitnessConditionType::And => write!(f, "And"),
            WitnessConditionType::Or => write!(f, "Or"),
            WitnessConditionType::ScriptHash => write!(f, "ScriptHash"),
            WitnessConditionType::Group => write!(f, "Group"),
            WitnessConditionType::CalledByEntry => write!(f, "CalledByEntry"),
            WitnessConditionType::CalledByContract => write!(f, "CalledByContract"),
            WitnessConditionType::CalledByGroup => write!(f, "CalledByGroup"),
        }
    }
}

impl fmt::Display for WitnessCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WitnessCondition::Boolean { value } => write!(f, "Boolean({value})"),
            WitnessCondition::Not { condition } => write!(f, "Not({condition})"),
            WitnessCondition::And { conditions } => {
                write!(
                    f,
                    "And([{}])",
                    conditions
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            WitnessCondition::Or { conditions } => {
                write!(
                    f,
                    "Or([{}])",
                    conditions
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            WitnessCondition::ScriptHash { hash } => write!(f, "ScriptHash({hash})"),
            WitnessCondition::Group { group } => write!(f, "Group({group:?})"),
            WitnessCondition::CalledByEntry => write!(f, "CalledByEntry"),
            WitnessCondition::CalledByContract { hash } => write!(f, "CalledByContract({hash})"),
            WitnessCondition::CalledByGroup { group } => write!(f, "CalledByGroup({group:?})"),
        }
    }
}

impl fmt::Display for WitnessRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WitnessRule {{ action: {}, condition: {} }}",
            self.action, self.condition
        )
    }
}

impl Serializable for WitnessCondition {
    fn size(&self) -> usize {
        WitnessCondition::size(self)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.condition_type().to_byte())?;
        match self {
            WitnessCondition::Boolean { value } => writer.write_bool(*value)?,
            WitnessCondition::Not { condition } => {
                <WitnessCondition as Serializable>::serialize(condition, writer)?;
            }
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                if conditions.is_empty() {
                    return Err(IoError::invalid_data(
                        "Composite witness condition requires at least one entry",
                    ));
                }
                if conditions.len() > WitnessCondition::MAX_SUBITEMS {
                    return Err(IoError::invalid_data(
                        "Composite witness condition exceeds max subitems",
                    ));
                }
                writer.write_var_int(conditions.len() as u64)?;
                for condition in conditions {
                    <WitnessCondition as Serializable>::serialize(condition, writer)?;
                }
            }
            WitnessCondition::ScriptHash { hash } => Serializable::serialize(hash, writer)?,
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                if group.len() != ECPOINT_COMPRESSED_SIZE {
                    return Err(IoError::invalid_data(
                        "Group condition requires a 33-byte compressed ECPoint",
                    ));
                }
                writer.write_bytes(group)?;
            }
            WitnessCondition::CalledByEntry => {}
            WitnessCondition::CalledByContract { hash } => {
                Serializable::serialize(hash, writer)?;
            }
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        WitnessCondition::deserialize_with_depth(reader, WitnessCondition::MAX_NESTING_DEPTH)
    }
}

impl WitnessCondition {
    pub fn deserialize_with_depth(reader: &mut MemoryReader, max_depth: usize) -> IoResult<Self> {
        if max_depth == 0 {
            return Err(IoError::invalid_data("Max nesting depth exceeded"));
        }

        let type_byte = reader.read_u8()?;
        let condition_type = WitnessConditionType::from_byte(type_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid witness condition type"))?;

        match condition_type {
            WitnessConditionType::Boolean => {
                let value = reader.read_bool()?;
                Ok(WitnessCondition::Boolean { value })
            }
            WitnessConditionType::Not => {
                let inner = WitnessCondition::deserialize_with_depth(reader, max_depth - 1)?;
                Ok(WitnessCondition::Not {
                    condition: Box::new(inner),
                })
            }
            WitnessConditionType::And => {
                let count = reader.read_var_int(WitnessCondition::MAX_SUBITEMS as u64)? as usize;
                if count == 0 || count > WitnessCondition::MAX_SUBITEMS {
                    return Err(IoError::invalid_data(
                        "Invalid AND witness condition length",
                    ));
                }
                let mut conditions = Vec::with_capacity(count);
                for _ in 0..count {
                    conditions.push(WitnessCondition::deserialize_with_depth(
                        reader,
                        max_depth - 1,
                    )?);
                }
                Ok(WitnessCondition::And { conditions })
            }
            WitnessConditionType::Or => {
                let count = reader.read_var_int(WitnessCondition::MAX_SUBITEMS as u64)? as usize;
                if count == 0 || count > WitnessCondition::MAX_SUBITEMS {
                    return Err(IoError::invalid_data("Invalid OR witness condition length"));
                }
                let mut conditions = Vec::with_capacity(count);
                for _ in 0..count {
                    conditions.push(WitnessCondition::deserialize_with_depth(
                        reader,
                        max_depth - 1,
                    )?);
                }
                Ok(WitnessCondition::Or { conditions })
            }
            WitnessConditionType::ScriptHash => {
                let hash = <UInt160 as Serializable>::deserialize(reader)?;
                Ok(WitnessCondition::ScriptHash { hash })
            }
            WitnessConditionType::Group => {
                let bytes = read_group_bytes(reader)?;
                if bytes.len() != ECPOINT_COMPRESSED_SIZE {
                    return Err(IoError::invalid_data("Invalid ECPoint length"));
                }
                Ok(WitnessCondition::Group { group: bytes })
            }
            WitnessConditionType::CalledByEntry => Ok(WitnessCondition::CalledByEntry),
            WitnessConditionType::CalledByContract => {
                let hash = <UInt160 as Serializable>::deserialize(reader)?;
                Ok(WitnessCondition::CalledByContract { hash })
            }
            WitnessConditionType::CalledByGroup => {
                let bytes = read_group_bytes(reader)?;
                if bytes.len() != ECPOINT_COMPRESSED_SIZE {
                    return Err(IoError::invalid_data("Invalid ECPoint length"));
                }
                Ok(WitnessCondition::CalledByGroup { group: bytes })
            }
        }
    }
}

impl Serialize for WitnessCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WitnessCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WitnessCondition::from_json(&value).map_err(SerdeDeError::custom)
    }
}

impl Serializable for WitnessRule {
    fn size(&self) -> usize {
        WitnessRule::size(self)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.action.to_byte())?;
        <WitnessCondition as Serializable>::serialize(&self.condition, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let action = WitnessRuleAction::from_byte(reader.read_u8()?)
            .ok_or_else(|| IoError::invalid_data("Invalid witness rule action"))?;
        let condition = <WitnessCondition as Serializable>::deserialize(reader)?;
        Ok(WitnessRule { action, condition })
    }
}

impl Serialize for WitnessRule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WitnessRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WitnessRule::from_json(&value).map_err(SerdeDeError::custom)
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_rule_action_values() {
        assert_eq!(WitnessRuleAction::Deny as u8, 0);
        assert_eq!(WitnessRuleAction::Allow as u8, 1);
    }
    #[test]
    fn test_witness_condition_type_values() {
        assert_eq!(WitnessConditionType::Boolean as u8, 0x00);
        assert_eq!(WitnessConditionType::Not as u8, 0x01);
        assert_eq!(WitnessConditionType::And as u8, 0x02);
        assert_eq!(WitnessConditionType::Or as u8, 0x03);
        assert_eq!(WitnessConditionType::ScriptHash as u8, 0x18);
        assert_eq!(WitnessConditionType::Group as u8, 0x19);
        assert_eq!(WitnessConditionType::CalledByEntry as u8, 0x20);
        assert_eq!(WitnessConditionType::CalledByContract as u8, 0x28);
        assert_eq!(WitnessConditionType::CalledByGroup as u8, 0x29);
    }
    #[test]
    fn test_witness_condition_validation() {
        // Boolean condition should be valid
        let boolean_condition = WitnessCondition::Boolean { value: true };
        assert!(boolean_condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
        // CalledByEntry condition should be valid
        let called_by_entry = WitnessCondition::CalledByEntry;
        assert!(called_by_entry.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
        // Empty And condition should be invalid
        let empty_and = WitnessCondition::And { conditions: vec![] };
        assert!(!empty_and.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
        // Valid And condition
        let valid_and = WitnessCondition::And {
            conditions: vec![
                WitnessCondition::Boolean { value: true },
                WitnessCondition::CalledByEntry,
            ],
        };
        assert!(valid_and.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
    }
    #[test]
    fn test_witness_rule_creation() {
        let condition = WitnessCondition::Boolean { value: true };
        let rule = WitnessRule::new(WitnessRuleAction::Allow, condition);
        assert_eq!(rule.action, WitnessRuleAction::Allow);
        assert!(rule.is_valid());
    }

    #[test]
    fn boolean_condition_json_matches_csharp_structure() {
        let condition = WitnessCondition::Boolean { value: true };
        let json = condition.to_json();
        assert_eq!(json["type"], "Boolean");
        assert_eq!(json["expression"], true);
        assert_eq!(
            WitnessCondition::from_json(&json).unwrap(),
            WitnessCondition::Boolean { value: true }
        );
    }

    #[test]
    fn group_condition_json_roundtrip_without_prefix() {
        let bytes = parse_group_bytes(
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        )
        .unwrap();
        let condition = WitnessCondition::Group {
            group: bytes.clone(),
        };
        let json = condition.to_json();
        assert_eq!(json["type"], "Group");
        assert_eq!(json["group"], hex_encode(&bytes));
        let decoded = WitnessCondition::from_json(&json).unwrap();
        assert_eq!(decoded, condition);
    }
}
