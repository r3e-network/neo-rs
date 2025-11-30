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
pub mod binary_serializer;
pub mod call_flags;
pub mod contract;
pub mod contract_basic_method;
pub mod contract_parameter;
pub mod contract_parameter_type;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod contract_task;
pub mod contract_task_awaiter;
pub mod contract_task_method_builder;
pub mod deployed_contract;
pub mod execution_context_state;
pub mod find_options;
pub mod helper;
pub mod i_diagnostic;
pub mod i_interoperable;
pub mod i_interoperable_verifiable;
pub mod interop_descriptor;
pub mod interop_parameter_descriptor;
pub mod iterators;
pub mod json_serializer;
pub mod key_builder;
pub mod log_event_args;
pub mod manifest;
pub mod max_length_attribute;
pub mod method_token;
pub mod native;
pub mod nef_file;
pub mod notify_event_args;
pub mod storage_context;
pub mod storage_item;
pub mod storage_key;
pub mod trigger_type;
pub mod validator_attribute;

// Re-export commonly used types
pub use application_engine::ApplicationEngine;
pub use binary_serializer::BinarySerializer;
pub use call_flags::CallFlags;
pub use contract::Contract;
pub use contract_basic_method::ContractBasicMethod;
pub use contract_parameter::ContractParameter;
pub use contract_parameter_type::ContractParameterType;
pub use contract_parameters_context::ContractParametersContext;
pub use contract_state::ContractState;
pub use contract_task::ContractTask;
pub use contract_task_awaiter::ContractTaskAwaiter;
pub use contract_task_method_builder::ContractTaskMethodBuilder;
pub use deployed_contract::DeployedContract;
pub use execution_context_state::ExecutionContextState;
pub use find_options::FindOptions;
pub use helper::Helper;
pub use i_diagnostic::IDiagnostic;
pub use i_interoperable::IInteroperable;
pub use i_interoperable_verifiable::IInteroperableVerifiable;
pub use interop_descriptor::InteropDescriptor;
pub use interop_parameter_descriptor::InteropParameterDescriptor;
pub use json_serializer::JsonSerializer;
pub use key_builder::KeyBuilder;
pub use log_event_args::LogEventArgs;
pub use manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, WildCardContainer,
};
pub use max_length_attribute::MaxLengthAttribute;
pub use method_token::MethodToken;
pub use nef_file::NefFile;
pub use notify_event_args::NotifyEventArgs;
pub use storage_context::StorageContext;
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
pub use trigger_type::TriggerType;
pub use validator_attribute::ValidatorAttribute;
