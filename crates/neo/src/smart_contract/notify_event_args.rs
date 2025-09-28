//! NotifyEventArgs - matches C# Neo.SmartContract.NotifyEventArgs exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::{IVerifiable, UInt160};
use neo_vm::StackItem;
use std::sync::Arc;

/// The EventArgs of ApplicationEngine.Notify (matches C# NotifyEventArgs)
#[derive(Clone, Debug)]
pub struct NotifyEventArgs {
    /// The container that containing the executed script
    pub script_container: Arc<dyn IVerifiable>,

    /// The script hash of the contract that sends the log
    pub script_hash: UInt160,

    /// The name of the event
    pub event_name: String,

    /// The arguments of the event
    pub state: Vec<StackItem>,
}

impl NotifyEventArgs {
    /// Initializes a new instance
    pub fn new(
        container: Arc<dyn IVerifiable>,
        script_hash: UInt160,
        event_name: String,
        state: Vec<StackItem>,
    ) -> Self {
        Self {
            script_container: container,
            script_hash,
            event_name,
            state,
        }
    }
}

impl IInteroperable for NotifyEventArgs {
    fn from_stack_item(&mut self, _stack_item: StackItem) {
        // Not supported in C# implementation
        panic!("NotSupportedException: FromStackItem is not supported for NotifyEventArgs");
    }

    fn to_stack_item(&self) -> StackItem {
        // Returns an array with [ScriptHash, EventName, State]
        StackItem::from_array(vec![
            StackItem::from_byte_string(self.script_hash.to_bytes()),
            StackItem::from_byte_string(self.event_name.clone().into_bytes()),
            StackItem::from_array(self.state.clone()),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
