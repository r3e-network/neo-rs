use alloc::boxed::Box;

use neo_base::hash::Hash160;

use crate::tx::condition::{WitnessConditionDto, WitnessConditionError, WitnessConditionType};

use super::super::WitnessCondition;
use super::helpers::{map_conditions, parse_public_key};

impl WitnessCondition {
    pub fn from_dto(dto: WitnessConditionDto) -> Result<Self, WitnessConditionError> {
        match dto {
            WitnessConditionDto::Boolean { expression } => {
                Ok(WitnessCondition::Boolean { expression })
            }
            WitnessConditionDto::Not { expression } => Ok(WitnessCondition::Not {
                expression: Box::new(Self::from_dto(*expression)?),
            }),
            WitnessConditionDto::And { expressions } => Ok(WitnessCondition::And {
                expressions: map_conditions(expressions)?,
            }),
            WitnessConditionDto::Or { expressions } => Ok(WitnessCondition::Or {
                expressions: map_conditions(expressions)?,
            }),
            WitnessConditionDto::ScriptHash { hash } => Ok(WitnessCondition::ScriptHash {
                hash: Hash160::from_hex_str(&hash)
                    .map_err(WitnessConditionError::InvalidScriptHash)?,
            }),
            WitnessConditionDto::Group { group } => Ok(WitnessCondition::Group {
                group: parse_public_key(&group)?,
            }),
            WitnessConditionDto::CalledByEntry {} => Ok(WitnessCondition::CalledByEntry),
            WitnessConditionDto::CalledByContract { hash } => {
                Ok(WitnessCondition::CalledByContract {
                    hash: Hash160::from_hex_str(&hash)
                        .map_err(WitnessConditionError::InvalidScriptHash)?,
                })
            }
            WitnessConditionDto::CalledByGroup { group } => Ok(WitnessCondition::CalledByGroup {
                group: parse_public_key(&group)?,
            }),
        }
    }

    pub fn kind(&self) -> WitnessConditionType {
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
}

impl TryFrom<WitnessConditionDto> for WitnessCondition {
    type Error = WitnessConditionError;

    fn try_from(value: WitnessConditionDto) -> Result<Self, Self::Error> {
        WitnessCondition::from_dto(value)
    }
}
