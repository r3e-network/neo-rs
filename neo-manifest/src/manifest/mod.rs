//! Contract manifest module for Neo smart contracts.
//!
//! This module provides types for contract manifests, which describe
//! the features, permissions, and interface of smart contracts.
//!
//! ## Layering
//!
//! The `neo-smart-contract-types` crate was the planned extraction
//! home for the pure data types. After the `kill-neo-core` refactor
//! those types now live in the `neo-manifest` crate (or, for the
//! ones that still depend on `Interoperable` / `StackItem` /
//! `ApplicationEngine`, here). The `neo_core::smart_contract::manifest`
//! path remains as the historical user-facing entry point and
//! continues to be the canonical home for the stateful, fully
//! assembled manifest types (`ContractManifest`, `ContractAbi`,
//! `ContractPermission`).

pub mod contract_abi;
pub mod contract_event_descriptor;
pub mod contract_group;
pub mod contract_manifest;
pub mod contract_method_descriptor;
pub mod contract_parameter_definition;
pub mod contract_permission;
pub mod contract_permission_descriptor;
pub(crate) mod stack_value_helpers;
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
