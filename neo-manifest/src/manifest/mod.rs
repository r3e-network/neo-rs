//! # neo-manifest::manifest
//!
//! Contract manifest, ABI, permission, and NEF-adjacent metadata types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-manifest`. This module belongs to neo-manifest
//! and must respect the workspace layer boundaries.
//!
//! ## Contents
//!
//! - `contract_abi`: contract ABI descriptor records.
//! - `contract_event_descriptor`: contract event descriptor records.
//! - `contract_group`: manifest contract group records.
//! - `contract_manifest`: contract manifest records and validation helpers.
//! - `contract_method_descriptor`: contract method descriptor records.
//! - `contract_parameter_definition`: contract parameter descriptor records.
//! - `contract_permission`: contract permission records.
//! - `contract_permission_descriptor`: contract permission target descriptors.
//! - `stack_value_helpers`: manifest stack-value conversion helpers.
//! - `wild_card_container`: wildcard container used by manifest permissions.

#[path = "abi/contract_abi.rs"]
pub mod contract_abi;
#[path = "abi/contract_event_descriptor.rs"]
pub mod contract_event_descriptor;
#[path = "permissions/contract_group.rs"]
pub mod contract_group;
pub mod contract_manifest;
#[path = "abi/contract_method_descriptor.rs"]
pub mod contract_method_descriptor;
#[path = "abi/contract_parameter_definition.rs"]
pub mod contract_parameter_definition;
#[path = "permissions/contract_permission.rs"]
pub mod contract_permission;
#[path = "permissions/contract_permission_descriptor.rs"]
pub mod contract_permission_descriptor;
#[path = "support/stack_value_helpers.rs"]
pub(crate) mod stack_value_helpers;
#[path = "support/wild_card_container.rs"]
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
