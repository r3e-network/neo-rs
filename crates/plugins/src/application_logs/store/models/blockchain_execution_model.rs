use super::{ApplicationEngineLogModel, BlockchainEventModel};
use crate::application_logs::store::states::ExecutionLogState;
use neo_core::smart_contract::TriggerType;
use neo_vm::{StackItem, VMState};

#[derive(Clone, Debug, PartialEq)]
pub struct BlockchainExecutionModel {
    pub trigger: TriggerType,
    pub vm_state: VMState,
    pub exception: String,
    pub gas_consumed: i64,
    pub stack: Vec<StackItem>,
    pub notifications: Vec<BlockchainEventModel>,
    pub logs: Vec<ApplicationEngineLogModel>,
}

impl BlockchainExecutionModel {
    pub fn create(trigger: TriggerType, state: &ExecutionLogState, stack: Vec<StackItem>) -> Self {
        Self {
            trigger,
            vm_state: state.vm_state,
            exception: state.exception.clone(),
            gas_consumed: state.gas_consumed,
            stack,
            notifications: Vec::new(),
            logs: Vec::new(),
        }
    }

    pub fn with_notifications(mut self, notifications: Vec<BlockchainEventModel>) -> Self {
        self.notifications = notifications;
        self
    }

    pub fn with_logs(mut self, logs: Vec<ApplicationEngineLogModel>) -> Self {
        self.logs = logs;
        self
    }
}
