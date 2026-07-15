//! NotifyEventArgs - matches C# Neo.SmartContract.NotifyEventArgs exactly.
//!
//! Ledger-level consumers (ApplicationLogs, TokensTracker, OracleService, and
//! the `ApplicationExecuted` payload in this crate) use this rich notification
//! type without taking a dependency on the full `neo-execution`
//! smart-contract engine crate. The execution crate re-exports this type for
//! back-compat with code that still imports it from there.

use neo_primitives::UInt160;
use neo_vm::{Interoperable, InteroperableError, StackItem, VmError};
use std::fmt;
use std::sync::Arc;

use crate::VerifiableContainer;

/// The EventArgs of ApplicationEngine.Notify (matches C# NotifyEventArgs)
#[derive(Clone)]
pub struct NotifyEventArgs {
    /// The container that containing the executed script.
    /// This can be None when the contract is invoked by system (e.g., OnPersist/PostPersist).
    pub script_container: Option<Arc<VerifiableContainer>>,

    /// The script hash of the contract that sends the log
    pub script_hash: UInt160,

    /// The name of the event
    pub event_name: String,

    /// The arguments of the event. This sequence is immutable after emission;
    /// use [`Self::state`] for read-only access.
    state: Vec<StackItem>,

    /// The immutable state array retained for pre-Domovoi reference semantics.
    state_array: StackItem,
}

impl NotifyEventArgs {
    /// Initializes a new instance with a container
    pub fn new(
        container: Arc<VerifiableContainer>,
        script_hash: UInt160,
        event_name: String,
        state: Vec<StackItem>,
    ) -> Self {
        let state_array = readonly_state_array(&state);
        Self {
            script_container: Some(container),
            script_hash,
            event_name,
            state,
            state_array,
        }
    }

    /// Initializes a new instance with an optional container (for system invocations)
    pub fn new_with_optional_container(
        container: Option<Arc<VerifiableContainer>>,
        script_hash: UInt160,
        event_name: String,
        state: Vec<StackItem>,
    ) -> Self {
        let state_array = readonly_state_array(&state);
        Self {
            script_container: container,
            script_hash,
            event_name,
            state,
            state_array,
        }
    }

    /// Returns the retained immutable state array used before the Domovoi hardfork.
    pub fn state_array(&self) -> StackItem {
        self.state_array.clone()
    }

    /// Returns the immutable notification arguments.
    #[must_use]
    pub fn state(&self) -> &[StackItem] {
        &self.state
    }

    /// Builds the C# `NotifyEventArgs.ToStackItem` layout with a caller-prepared
    /// state array.
    ///
    /// The runtime owns hardfork-specific state-copying policy. This helper keeps
    /// the `[ScriptHash, EventName, State]` projection in one place.
    pub fn to_stack_item_with_state_array(&self, state_array: StackItem) -> StackItem {
        StackItem::from_array(vec![
            StackItem::from_byte_string(self.script_hash.to_bytes()),
            StackItem::from_byte_string(self.event_name.clone().into_bytes()),
            state_array,
        ])
    }

    /// Builds the C# `NotifyEventArgs.ToStackItem` layout with a caller-prepared
    /// state array.
    pub fn try_to_stack_item_with_state_array(
        &self,
        state_array: StackItem,
    ) -> Result<StackItem, VmError> {
        Ok(self.to_stack_item_with_state_array(state_array))
    }

    /// Converts the notification using the retained immutable state array.
    pub fn to_stack_item(&self) -> StackItem {
        self.to_stack_item_with_state_array(self.state_array())
    }
}

fn readonly_state_array(state: &[StackItem]) -> StackItem {
    let item = StackItem::from_array(state.to_vec());
    if let StackItem::Array(array) = &item {
        array.set_read_only(true);
    }
    item
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
    fn from_stack_item(&mut self, _value: StackItem) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "NotifyEventArgs::from_stack_item is not supported".into(),
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(NotifyEventArgs::to_stack_item(self))
    }
}

#[cfg(test)]
#[path = "../tests/execution/notify_event_args.rs"]
mod tests;
