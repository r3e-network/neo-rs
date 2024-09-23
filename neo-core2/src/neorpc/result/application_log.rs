use serde::{Deserialize, Serialize};
use serde_json::{self, json};
use std::error::Error;
use std::fmt;

use crate::core::state;
use crate::smartcontract::trigger;
use crate::util;

// ApplicationLog represents the results of the script executions for a block or a transaction.
#[derive(Serialize, Deserialize)]
pub struct ApplicationLog {
    container: util::Uint256,
    is_transaction: bool,
    executions: Vec<state::Execution>,
}

// applicationLogAux is an auxiliary struct for ApplicationLog JSON marshalling.
#[derive(Serialize, Deserialize)]
struct ApplicationLogAux {
    #[serde(rename = "txid", skip_serializing_if = "Option::is_none")]
    tx_hash: Option<util::Uint256>,
    #[serde(rename = "blockhash", skip_serializing_if = "Option::is_none")]
    block_hash: Option<util::Uint256>,
    executions: Vec<serde_json::Value>,
}

// Implement custom serialization for ApplicationLog
impl serde::Serialize for ApplicationLog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut result = ApplicationLogAux {
            tx_hash: None,
            block_hash: None,
            executions: vec![json!(null); self.executions.len()],
        };

        if self.is_transaction {
            result.tx_hash = Some(self.container.clone());
        } else {
            result.block_hash = Some(self.container.clone());
        }

        for (i, execution) in self.executions.iter().enumerate() {
            result.executions[i] = serde_json::to_value(execution).map_err(serde::ser::Error::custom)?;
        }

        result.serialize(serializer)
    }
}

// Implement custom deserialization for ApplicationLog
impl<'de> serde::Deserialize<'de> for ApplicationLog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let aux = ApplicationLogAux::deserialize(deserializer)?;

        let container = if let Some(tx_hash) = aux.tx_hash {
            tx_hash
        } else if let Some(block_hash) = aux.block_hash {
            block_hash
        } else {
            return Err(serde::de::Error::custom("no block or transaction hash"));
        };

        let mut executions = Vec::with_capacity(aux.executions.len());
        for execution in aux.executions {
            executions.push(serde_json::from_value(execution).map_err(serde::de::Error::custom)?);
        }

        Ok(ApplicationLog {
            container,
            is_transaction: aux.tx_hash.is_some(),
            executions,
        })
    }
}

// NewApplicationLog creates an ApplicationLog from a set of several application execution results
// including only the results with the specified trigger.
pub fn new_application_log(hash: util::Uint256, aers: Vec<state::AppExecResult>, trig: trigger::Type) -> ApplicationLog {
    let mut result = ApplicationLog {
        container: hash,
        is_transaction: aers[0].trigger == trigger::Type::Application,
        executions: Vec::new(),
    };

    for aer in aers {
        if aer.trigger & trig != 0 {
            result.executions.push(aer.execution);
        }
    }

    result
}
