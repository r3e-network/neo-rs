use crate::application_logs::store::states::EngineLogState;
use neo_core::UInt160;

/// Lightweight representation of an engine log entry captured during execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationEngineLogModel {
    pub script_hash: UInt160,
    pub message: String,
}

impl ApplicationEngineLogModel {
    pub fn create(state: &EngineLogState) -> Self {
        Self {
            script_hash: state.script_hash,
            message: state.message.clone(),
        }
    }
}
