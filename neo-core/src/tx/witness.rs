// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::script::Script;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Witness {
    pub invocation_script: Script,
    pub verification_script: Script,
}

impl Witness {
    pub fn new(invocation: Script, verification: Script) -> Self {
        Self {
            invocation_script: invocation,
            verification_script: verification,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Witnesses(pub(crate) [Witness; 1]);

impl Witnesses {
    pub fn witness(&self) -> &Witness {
        &self.0[0]
    }
}

impl Default for Witnesses {
    fn default() -> Self {
        Self([Witness::new(Script::default(), Script::default())])
    }
}

impl From<Witness> for Witnesses {
    fn from(value: Witness) -> Self {
        Self([value])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum WitnessScope {
    None = 0x00,
    CalledByEntry = 0x01,
    CustomContracts = 0x10,
    CustomGroups = 0x20,
    WitnessRules = 0x40,
    Global = 0x80,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WitnessScopes {
    scopes: u8,
}

impl WitnessScopes {
    pub fn scopes(&self) -> u8 {
        self.scopes
    }

    pub fn add_scope(&mut self, scope: WitnessScope) {
        self.scopes |= scope as u8;
    }

    pub fn has_scope(&self, scope: WitnessScope) -> bool {
        self.scopes & (scope as u8) != 0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WitnessRule {
    pub action: Action,
    pub condition: WitnessCondition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Action {
    Deny = 0x00,
    Allow = 0x01,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessConditionType {
    Boolean = 0x00,
    Not = 0x01,
    And = 0x02,
    Or = 0x03,
    ScriptHash = 0x18,
    Group = 0x19,
    CalledByEntry = 0x20,
    CalledByContract = 0x28,
    CalledByGroup = 0x29,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum WitnessCondition {
    // #[bin(tag = 0x00)]
    Boolean { expression: bool },

    // #[bin(tag = 0x01)]
    Not { hash: String },

    // #[bin(tag = 0x02)]
    And { expressions: Vec<WitnessCondition> },

    // #[bin(tag = 0x03)]
    Or { expressions: Vec<WitnessCondition> },

    // #[bin(tag = 0x18)]
    ScriptHash { hash: String },

    // #[bin(tag = 0x19)]
    Group { group: String },

    // #[bin(tag = 0x20)]
    CalledByEntry {},

    // #[bin(tag = 0x28)]
    CalledByContract { hash: String },

    // #[bin(tag = 0x29)]
    CalledByGroup { group: String },
}
