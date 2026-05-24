use super::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
use std::fmt;

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
