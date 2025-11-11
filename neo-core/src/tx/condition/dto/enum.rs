use alloc::{boxed::Box, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum WitnessConditionDto {
    Boolean {
        expression: bool,
    },
    Not {
        expression: Box<WitnessConditionDto>,
    },
    And {
        expressions: Vec<WitnessConditionDto>,
    },
    Or {
        expressions: Vec<WitnessConditionDto>,
    },
    ScriptHash {
        hash: String,
    },
    Group {
        group: String,
    },
    CalledByEntry {},
    CalledByContract {
        hash: String,
    },
    CalledByGroup {
        group: String,
    },
}
