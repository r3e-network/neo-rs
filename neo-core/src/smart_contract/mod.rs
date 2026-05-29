//! Smart Contract module for Neo blockchain
//!
//! This module provides smart contract functionality matching the C# Neo.SmartContract namespace.

// Main modules (matching C# files)
pub mod application_engine;
pub mod application_engine_contract;
pub mod application_engine_crypto;
pub mod application_engine_helper;
pub mod application_engine_iterator;
pub mod application_engine_op_code_prices;
pub mod application_engine_runtime;
pub mod application_engine_storage;
pub mod contract;
pub mod contract_parameter;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod deployed_contract;
pub mod diagnostic;
pub(crate) mod env_flags;
pub mod execution_context_state;
pub mod helper;
pub mod interop_descriptor;
pub mod interop_parameter_descriptor;
pub mod interoperable;
pub mod iterators;
pub mod key_builder;
pub mod manifest;
pub mod max_length_attribute;
pub mod native;
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
pub use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, ContractTask, FindOptions, LogEventArgs,
    TriggerType,
};
pub use neo_io_crate::MethodToken;
pub use neo_vm::{BinarySerializer, Interoperable, JsonSerializer, NotifyEventArgs, StorageContext};
pub use crate::persistence::{StorageItem, StorageItemExt, StorageKey};

// Module-path aliases for relocated modules (callers use `module::Type` paths).
pub use neo_primitives::{call_flags, contract_parameter_type, find_options, trigger_type};
pub use neo_primitives::{contract_basic_method, log_event_args};
pub use neo_vm::{binary_serializer, storage_context};
pub use neo_vm::notify_event_args;
pub use crate::persistence::{storage_item, storage_key};
