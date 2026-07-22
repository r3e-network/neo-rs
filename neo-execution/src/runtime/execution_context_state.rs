//! ExecutionContextState - matches C# Neo.SmartContract.ExecutionContextState exactly

use crate::contract_state::ContractState;
use neo_primitives::CallFlags;
use neo_primitives::ContractParameterType;
use neo_primitives::UInt160;
use neo_storage::{CacheRead, DataCache, EmptyCacheBacking};
use std::sync::Arc;

/// NeoVM context specialized with the application engine's host state.
pub type ApplicationExecutionContext<B = EmptyCacheBacking> =
    neo_vm::ExecutionContext<ExecutionContextState<B>>;

/// NeoVM engine specialized with the application engine's host state.
pub type ApplicationExecutionEngine<B = EmptyCacheBacking> =
    neo_vm::ExecutionEngine<ExecutionContextState<B>>;

/// NeoVM opcode table specialized with the application engine's host state.
pub type ApplicationJumpTable<B = EmptyCacheBacking> = neo_vm::JumpTable<ExecutionContextState<B>>;

/// State associated with an execution context (matches C# ExecutionContextState)
#[derive(Clone)]
pub struct ExecutionContextState<B = EmptyCacheBacking> {
    /// The script hash being executed
    pub script_hash: Option<UInt160>,

    /// The calling script hash (matches C# CallingContext hash resolution)
    pub calling_script_hash: Option<UInt160>,

    /// The calling execution context (matches C# CallingContext property)
    pub calling_context: Option<ApplicationExecutionContext<B>>,

    /// The native calling script hash (set by native contracts)
    pub native_calling_script_hash: Option<UInt160>,

    /// The contract being executed
    pub contract: Option<Arc<ContractState>>,

    /// The call flags for this context
    pub call_flags: CallFlags,

    /// Cloned snapshot cache for the context
    pub snapshot_cache: Option<Arc<DataCache<B>>>,

    /// Notification count emitted by this context
    pub notification_count: usize,

    /// Indicates whether this context was produced by a dynamic call
    pub is_dynamic_call: bool,

    /// Indicates whether this context is whitelisted for fixed fees
    pub whitelisted: bool,

    /// Name of the method currently executing
    pub method_name: Option<String>,

    /// Number of arguments supplied to the method
    pub argument_count: usize,

    /// Return type of the executing method
    pub return_type: Option<ContractParameterType>,

    /// Parameter types for the executing method
    pub parameter_types: Vec<ContractParameterType>,
}

impl<B: CacheRead> ExecutionContextState<B> {
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
            whitelisted: false,
            method_name: None,
            argument_count: 0,
            return_type: None,
            parameter_types: Vec::new(),
        }
    }
}

impl<B: CacheRead> Default for ExecutionContextState<B> {
    fn default() -> Self {
        Self::new()
    }
}
