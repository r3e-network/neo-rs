use super::helpers::{read_group_bytes, ECPOINT_COMPRESSED_SIZE};
use super::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
use crate::neo_io::serializable::helper::serialize_array;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::UInt160;

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
                serialize_array(conditions, writer)?;
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
    /// Helper to deserialize a list of conditions (used by And/Or).
    fn deserialize_condition_list(
        reader: &mut MemoryReader,
        max_depth: usize,
        error_msg: &'static str,
    ) -> IoResult<Vec<WitnessCondition>> {
        let count = reader.read_var_int(Self::MAX_SUBITEMS as u64)? as usize;
        if count == 0 || count > Self::MAX_SUBITEMS {
            return Err(IoError::invalid_data(error_msg));
        }
        let mut conditions = Vec::with_capacity(count);
        for _ in 0..count {
            conditions.push(Self::deserialize_with_depth(reader, max_depth - 1)?);
        }
        Ok(conditions)
    }

    /// Helper to deserialize and validate group bytes.
    fn deserialize_group_bytes(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
        let bytes = read_group_bytes(reader)?;
        if bytes.len() != ECPOINT_COMPRESSED_SIZE {
            return Err(IoError::invalid_data("Invalid ECPoint length"));
        }
        Ok(bytes)
    }

    pub fn deserialize_with_depth(reader: &mut MemoryReader, max_depth: usize) -> IoResult<Self> {
        if max_depth == 0 {
            return Err(IoError::invalid_data("Max nesting depth exceeded"));
        }

        let type_byte = reader.read_u8()?;
        let condition_type = WitnessConditionType::from_byte(type_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid witness condition type"))?;

        match condition_type {
            WitnessConditionType::Boolean => Ok(WitnessCondition::Boolean {
                value: reader.read_bool()?,
            }),
            WitnessConditionType::Not => {
                let inner = Self::deserialize_with_depth(reader, max_depth - 1)?;
                Ok(WitnessCondition::Not {
                    condition: Box::new(inner),
                })
            }
            WitnessConditionType::And => {
                let conditions = Self::deserialize_condition_list(
                    reader,
                    max_depth,
                    "Invalid AND witness condition length",
                )?;
                Ok(WitnessCondition::And { conditions })
            }
            WitnessConditionType::Or => {
                let conditions = Self::deserialize_condition_list(
                    reader,
                    max_depth,
                    "Invalid OR witness condition length",
                )?;
                Ok(WitnessCondition::Or { conditions })
            }
            WitnessConditionType::ScriptHash => Ok(WitnessCondition::ScriptHash {
                hash: <UInt160 as Serializable>::deserialize(reader)?,
            }),
            WitnessConditionType::Group => Ok(WitnessCondition::Group {
                group: Self::deserialize_group_bytes(reader)?,
            }),
            WitnessConditionType::CalledByEntry => Ok(WitnessCondition::CalledByEntry),
            WitnessConditionType::CalledByContract => Ok(WitnessCondition::CalledByContract {
                hash: <UInt160 as Serializable>::deserialize(reader)?,
            }),
            WitnessConditionType::CalledByGroup => Ok(WitnessCondition::CalledByGroup {
                group: Self::deserialize_group_bytes(reader)?,
            }),
        }
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
