// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Models.ExecutionEngineModel.

use crate::rest_server::models::error::error_model::ErrorModel;
use neo_core::UInt160;
use neo_vm::vm_state::VMState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEngineModel {
    pub gas_consumed: i64,
    #[serde(with = "vm_state_serde")]
    pub state: VMState,
    #[serde(default)]
    pub notifications: Vec<BlockchainEventModel>,
    #[serde(default)]
    pub result_stack: Vec<Value>,
    pub fault_exception: Option<ErrorModel>,
}

impl ExecutionEngineModel {
    pub fn new(
        gas_consumed: i64,
        state: VMState,
        notifications: Vec<BlockchainEventModel>,
        result_stack: Vec<Value>,
        fault_exception: Option<ErrorModel>,
    ) -> Self {
        Self {
            gas_consumed,
            state,
            notifications,
            result_stack,
            fault_exception,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainEventModel {
    pub script_hash: UInt160,
    pub event_name: String,
    #[serde(default)]
    pub state: Vec<Value>,
}

impl BlockchainEventModel {
    pub fn new(script_hash: UInt160, event_name: impl Into<String>, state: Vec<Value>) -> Self {
        Self {
            script_hash,
            event_name: event_name.into(),
            state,
        }
    }
}

mod vm_state_serde {
    use neo_vm::vm_state::VMState;
    use serde::Deserialize;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(state: &VMState, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match state {
            VMState::NONE => "NONE",
            VMState::HALT => "HALT",
            VMState::FAULT => "FAULT",
            VMState::BREAK => "BREAK",
        })
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VMState, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.to_ascii_uppercase().as_str() {
            "NONE" => Ok(VMState::NONE),
            "HALT" => Ok(VMState::HALT),
            "FAULT" => Ok(VMState::FAULT),
            "BREAK" => Ok(VMState::BREAK),
            other => Err(serde::de::Error::custom(format!(
                "Unknown VMState value: {}",
                other
            ))),
        }
    }
}
