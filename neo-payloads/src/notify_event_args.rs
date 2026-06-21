//! NotifyEventArgs - matches C# Neo.SmartContract.NotifyEventArgs exactly.
//!
//! Ledger-level consumers (ApplicationLogs, TokensTracker, OracleService, and
//! the `ApplicationExecuted` payload in this crate) use this rich notification
//! type without taking a dependency on the full `neo-execution`
//! smart-contract engine crate. The execution crate re-exports this type for
//! back-compat with code that still imports it from there.

use neo_primitives::{UInt160, Verifiable};
use neo_vm::{Interoperable, InteroperableError, StackItem, VmError};
use neo_vm_rs::StackValue;
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

    /// Builds the C# `NotifyEventArgs.ToStackItem` layout with a caller-prepared
    /// state array in the lean neo-vm-rs representation.
    ///
    /// The runtime owns hardfork-specific state-copying policy. This helper keeps
    /// the `[ScriptHash, EventName, State]` projection in one place.
    pub fn to_stack_value_with_state_array(&self, state_array: StackValue) -> StackValue {
        StackValue::Array(
            0,
            vec![
                StackValue::ByteString(self.script_hash.to_bytes()),
                StackValue::ByteString(self.event_name.clone().into_bytes()),
                state_array,
            ],
        )
    }

    /// Converts the notification to a neo-vm-rs stack value using its current state array.
    pub fn to_stack_value(&self) -> Result<StackValue, VmError> {
        let state = self
            .state
            .iter()
            .cloned()
            .map(StackValue::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.to_stack_value_with_state_array(StackValue::Array(0, state)))
    }

    /// Builds the C# `NotifyEventArgs.ToStackItem` layout with a caller-prepared
    /// state array, adapting through the canonical [`StackValue`] projection.
    pub fn try_to_stack_item_with_state_array(
        &self,
        state_array: StackItem,
    ) -> Result<StackItem, VmError> {
        let StackValue::Array(0, mut fields) =
            self.to_stack_value_with_state_array(StackValue::Null)
        else {
            unreachable!("notification projection is always an array");
        };
        let script_hash = StackItem::try_from(fields.remove(0)).map_err(|error| {
            VmError::invalid_operation_msg(format!(
                "Failed to convert notification script hash StackValue to StackItem: {error}"
            ))
        })?;
        let event_name = StackItem::try_from(fields.remove(0)).map_err(|error| {
            VmError::invalid_operation_msg(format!(
                "Failed to convert notification event name StackValue to StackItem: {error}"
            ))
        })?;

        Ok(StackItem::from_array(vec![
            script_hash,
            event_name,
            state_array,
        ]))
    }

    /// Builds the C# `NotifyEventArgs.ToStackItem` layout with a caller-prepared
    /// state array.
    pub fn to_stack_item_with_state_array(&self, state_array: StackItem) -> StackItem {
        self.try_to_stack_item_with_state_array(state_array)
            .expect("notification StackValue projection must be StackItem-compatible")
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
    fn from_stack_value(&mut self, _value: StackValue) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "NotifyEventArgs::from_stack_value is not supported".into(),
        ))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        self.to_stack_value()
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
#[path = "tests/notify_event_args.rs"]
mod tests;
