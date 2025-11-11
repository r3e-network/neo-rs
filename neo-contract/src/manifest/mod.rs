//! Contract manifest structures that mirror the Neo C# implementation.
//!
//! The module is intentionally split into smaller files to keep each concept
//! (ABI, groups, permissions, etc.) focused.  `mod.rs` simply re-exports the
//! public types so callers can continue to use `crate::manifest::*` without
//! churn.

mod abi;
mod features;
mod group;
mod manifest;
mod permission;
mod util;

pub use abi::{ContractAbi, ContractEvent, ContractMethod, ContractParameter, ParameterKind};
pub use features::ContractFeatures;
pub use group::ContractGroup;
pub use manifest::{ContractManifest, MAX_MANIFEST_LENGTH};
pub use permission::{ContractPermission, ContractPermissionDescriptor, WildcardContainer};
