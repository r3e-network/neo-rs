use crate::application_logs::store::states::{ContractLogState, NotifyLogState};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::UInt160;
use neo_vm::StackItem;

/// Execution notification model mirroring the C# plugin output.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockchainEventModel {
    pub script_hash: UInt160,
    pub event_name: String,
    pub state: Vec<StackItem>,
}

impl BlockchainEventModel {
    pub fn create_from_notification(notification: &NotifyEventArgs) -> Self {
        Self {
            script_hash: notification.script_hash,
            event_name: notification.event_name.clone(),
            state: notification.state.clone(),
        }
    }

    pub fn create_from_notify_state(state: &NotifyLogState, stack: Vec<StackItem>) -> Self {
        Self {
            script_hash: state.script_hash,
            event_name: state.event_name.clone(),
            state: stack,
        }
    }

    pub fn create_from_contract_state(state: &ContractLogState, stack: Vec<StackItem>) -> Self {
        Self {
            script_hash: state.notify.script_hash,
            event_name: state.notify.event_name.clone(),
            state: stack,
        }
    }
}
