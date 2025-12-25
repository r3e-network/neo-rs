//! ExecutionContextState - matches C# Neo.SmartContract.ExecutionContextState exactly

use crate::persistence::data_cache::DataCache;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::smart_contract::contract_state::ContractState;
use crate::UInt160;
use neo_vm::ExecutionContext;
use std::sync::Arc;

/// State associated with an execution context (matches C# ExecutionContextState)
#[derive(Clone)]
pub struct ExecutionContextState {
    /// The script hash being executed
    pub script_hash: Option<UInt160>,

    /// The calling script hash (matches C# CallingContext hash resolution)
    pub calling_script_hash: Option<UInt160>,

    /// The calling execution context (matches C# CallingContext property)
    pub calling_context: Option<ExecutionContext>,

    /// The native calling script hash (set by native contracts)
    pub native_calling_script_hash: Option<UInt160>,

    /// The contract being executed
    pub contract: Option<ContractState>,

    /// The call flags for this context
    pub call_flags: CallFlags,

    /// Cloned snapshot cache for the context
    pub snapshot_cache: Option<Arc<DataCache>>,

    /// Notification count emitted by this context
    pub notification_count: usize,

    /// Indicates whether this context was produced by a dynamic call
    pub is_dynamic_call: bool,

    /// Name of the method currently executing
    pub method_name: Option<String>,

    /// Number of arguments supplied to the method
    pub argument_count: usize,

    /// Return type of the executing method
    pub return_type: Option<ContractParameterType>,

    /// Parameter types for the executing method
    pub parameter_types: Vec<ContractParameterType>,
}

impl ExecutionContextState {
    /// Creates a new execution context state
    pub fn new() -> Self {
        Self {
            script_hash: None,
            calling_script_hash: None,
            calling_context: None,
            native_calling_script_hash: None,
            contract: None,
            call_flags: CallFlags::ALL,
            snapshot_cache: None,
            notification_count: 0,
            is_dynamic_call: false,
            method_name: None,
            argument_count: 0,
            return_type: None,
            parameter_types: Vec::new(),
        }
    }
}

impl Default for ExecutionContextState {
    fn default() -> Self {
        Self::new()
    }
}
