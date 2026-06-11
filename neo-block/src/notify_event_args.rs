//! NotifyEventArgs - matches C# Neo.SmartContract.NotifyEventArgs exactly.
//!
//! This type was moved from `neo_execution` so that ledger-level
//! consumers (ApplicationLogs, TokensTracker, OracleService, and the
//! `ApplicationExecuted` payload in this crate) can use the rich
//! notification type without taking a dependency on the full
//! `neo_execution` smart-contract engine crate. The execution crate
//! re-exports this type for back-compat with code that still imports
//! it from there.

use neo_primitives::{UInt160, Verifiable};
use neo_vm::interoperable::Interoperable;
use neo_vm::StackItem;
use std::fmt;
use std::sync::Arc;

/// The EventArgs of ApplicationEngine.Notify (matches C# NotifyEventArgs)
#[derive(Clone)]
pub struct NotifyEventArgs {
    /// The container that containing the executed script.
    /// This can be None when the contract is invoked by system (e.g., OnPersist/PostPersist).
    pub script_container: Option<Arc<dyn Verifiable>>,

    /// The script hash of the contract that sends the log
    pub script_hash: UInt160,

    /// The name of the event
    pub event_name: String,

    /// The arguments of the event
    pub state: Vec<StackItem>,
}

impl NotifyEventArgs {
    /// Initializes a new instance with a container
    pub fn new(
        container: Arc<dyn Verifiable>,
        script_hash: UInt160,
        event_name: String,
        state: Vec<StackItem>,
    ) -> Self {
        Self {
            script_container: Some(container),
            script_hash,
            event_name,
            state,
        }
    }

    /// Initializes a new instance with an optional container (for system invocations)
    pub fn new_with_optional_container(
        container: Option<Arc<dyn Verifiable>>,
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

impl fmt::Debug for NotifyEventArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotifyEventArgs")
            .field("script_hash", &self.script_hash)
            .field("event_name", &self.event_name)
            .field("state_len", &self.state.len())
            .finish()
    }
}

impl Interoperable for NotifyEventArgs {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), neo_vm::VmError> {
        // Not supported in C# implementation (throws NotSupportedException)
        Err(neo_vm::VmError::invalid_operation_msg(
            "FromStackItem is not supported for NotifyEventArgs",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, neo_vm::VmError> {
        // Returns an array with [ScriptHash, EventName, State]
        let state: Vec<StackItem> = self.state.iter().cloned().collect();
        Ok(StackItem::from_array(vec![
            StackItem::from_byte_string(self.script_hash.to_bytes()),
            StackItem::from_byte_string(self.event_name.clone().into_bytes()),
            StackItem::from_array(state),
        ]))
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}
