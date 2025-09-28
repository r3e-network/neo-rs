// Copyright (C) 2015-2025 The Neo Project.
//
// witness_rule.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    conditions::witness_condition::WitnessCondition, witness_rule_action::WitnessRuleAction,
};
use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Represents a rule for witness verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WitnessRule {
    /// The action to be taken if the current context satisfies the rule.
    pub action: WitnessRuleAction,

    /// The condition of the rule.
    pub condition: WitnessCondition,
}

impl WitnessRule {
    /// Creates a new witness rule.
    pub fn new(action: WitnessRuleAction, condition: WitnessCondition) -> Self {
        Self { action, condition }
    }

    /// Converts the witness rule to a JSON object.
    /// Matches C# ToJson method.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "action": self.action.to_string(),
            "condition": self.condition.to_json(),
        })
    }

    /// Creates a witness rule from a JSON object.
    /// Matches C# FromJson static method.
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let action = json["action"]
            .as_str()
            .and_then(|s| WitnessRuleAction::from_str(s).ok())
            .ok_or("Invalid action")?;

        let condition = WitnessCondition::from_json(&json["condition"])?;

        Ok(Self { action, condition })
    }
}

impl Serializable for WitnessRule {
    fn size(&self) -> usize {
        1 + self.condition.size()
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&[self.action.to_byte()])?;
        self.condition.serialize(writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let action_byte = reader.read_u8().map_err(|e| e.to_string())?;
        let action = WitnessRuleAction::from_byte(action_byte)
            .ok_or_else(|| format!("Invalid witness rule action: {}", action_byte))?;

        let condition = WitnessCondition::deserialize(reader)?;

        Ok(Self { action, condition })
    }
}
