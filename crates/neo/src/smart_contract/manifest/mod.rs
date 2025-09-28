//! Contract manifest module for Neo smart contracts.
//!
//! This module provides types for contract manifests, which describe
//! the features, permissions, and interface of smart contracts.

pub mod contract_abi;
pub mod contract_event_descriptor;
pub mod contract_group;
pub mod contract_manifest;
pub mod contract_method_descriptor;
pub mod contract_parameter_definition;
pub mod contract_permission;
pub mod contract_permission_descriptor;
pub mod wild_card_container;

pub use contract_abi::ContractAbi;
pub use contract_event_descriptor::ContractEventDescriptor;
pub use contract_group::ContractGroup;
pub use contract_manifest::ContractManifest;
pub use contract_method_descriptor::ContractMethodDescriptor;
pub use contract_parameter_definition::ContractParameterDefinition;
pub use contract_permission::ContractPermission;
pub use contract_permission_descriptor::ContractPermissionDescriptor;
pub use wild_card_container::WildCardContainer;
