use alloc::{boxed::Box, format};

use hex::encode;

use crate::tx::condition::WitnessCondition;

use super::WitnessConditionDto;

impl From<&WitnessCondition> for WitnessConditionDto {
    fn from(condition: &WitnessCondition) -> Self {
        match condition {
            WitnessCondition::Boolean { expression } => WitnessConditionDto::Boolean {
                expression: *expression,
            },
            WitnessCondition::Not { expression } => WitnessConditionDto::Not {
                expression: Box::new(Self::from(expression.as_ref())),
            },
            WitnessCondition::And { expressions } => WitnessConditionDto::And {
                expressions: expressions.iter().map(Self::from).collect(),
            },
            WitnessCondition::Or { expressions } => WitnessConditionDto::Or {
                expressions: expressions.iter().map(Self::from).collect(),
            },
            WitnessCondition::ScriptHash { hash } => WitnessConditionDto::ScriptHash {
                hash: hash.to_string(),
            },
            WitnessCondition::Group { group } => WitnessConditionDto::Group {
                group: format!("0x{}", encode(group.to_compressed())),
            },
            WitnessCondition::CalledByEntry => WitnessConditionDto::CalledByEntry {},
            WitnessCondition::CalledByContract { hash } => WitnessConditionDto::CalledByContract {
                hash: hash.to_string(),
            },
            WitnessCondition::CalledByGroup { group } => WitnessConditionDto::CalledByGroup {
                group: format!("0x{}", encode(group.to_compressed())),
            },
        }
    }
}
