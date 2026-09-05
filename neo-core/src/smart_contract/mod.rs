//! Smart Contract module for Neo blockchain
//!
//! This module provides smart contract functionality matching the C# Neo.SmartContract namespace.

// Main modules (matching C# files)
pub mod application_engine;
// Compatibility aliases for the historical flat module paths. Implementations
// live under the application_engine directory.
pub use application_engine::{
    contract as application_engine_contract, crypto as application_engine_crypto,
    helper as application_engine_helper, iterator as application_engine_iterator,
    op_code_prices as application_engine_op_code_prices, runtime as application_engine_runtime,
    storage as application_engine_storage,
};
pub mod binary_serializer;
pub mod call_flags;
pub mod contract;
pub mod contract_parameter;
pub mod contract_parameter_type;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod deployed_contract;
pub mod diagnostic;
pub(crate) mod env_flags;
pub mod execution_context_state;
pub mod find_options;
pub mod helper;
pub mod interop_descriptor;
pub mod interop_parameter_descriptor;
pub mod interoperable;
pub mod iterators;
pub mod key_builder;

/// Compatibility module for the historical flat contract helper paths.
pub mod contract_basic_method {
    pub use neo_primitives::ContractBasicMethod;
}
/// Re-export of the log event payload raised by `System.Runtime.Log`.
pub mod log_event_args {
    pub use neo_primitives::LogEventArgs;
}
/// Re-export of the notification payload raised by `System.Runtime.Notify`.
pub mod notify_event_args {
    pub use neo_vm::NotifyEventArgs;
}

pub mod manifest;
pub mod max_length_attribute;
pub mod native;
pub mod storage_context;
pub mod storage_item;
pub mod storage_key;
pub mod trigger_type;
pub mod validator_attribute;

// Re-export commonly used types
pub use application_engine::ApplicationEngine;
pub use contract::Contract;
pub use contract_parameter::ContractParameter;
pub use contract_parameters_context::ContractParametersContext;
pub use contract_state::{ContractState, NefFile};
pub use deployed_contract::DeployedContract;
pub use diagnostic::Diagnostic;
pub use execution_context_state::ExecutionContextState;
pub use helper::Helper;
pub use interop_descriptor::InteropDescriptor;
pub use interop_parameter_descriptor::InteropParameterDescriptor;
pub use manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, WildCardContainer,
};
pub use max_length_attribute::MaxLengthAttribute;
pub use validator_attribute::ValidatorAttribute;

// Re-exports from foundation crates (no separate files needed)
pub use crate::persistence::{StorageItem, StorageItemExt, StorageKey};
pub use neo_io_crate::MethodToken;
pub use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, ContractTask, FindOptions, LogEventArgs,
    TriggerType,
};
pub use neo_vm::{
    BinarySerializer, Interoperable, JsonSerializer, NotifyEventArgs, StorageContext,
};
