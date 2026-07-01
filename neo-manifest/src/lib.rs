//! # neo-manifest
//!
//! Contract manifest, NEF, ABI, permission, and call-flag domain types.
//!
//! ## Boundary
//!
//! This module belongs to neo-manifest and must respect the workspace layer
//! boundaries.
//!
//! ## Contents
//!
//! - `call_flags`: contract call-flag records.
//! - `manifest`: Contract manifest, ABI, permission, and NEF-adjacent metadata
//!   types.
//! - `method_token`: NEF method-token records.
//! - `nef_file`: NEF file records and checksum logic.
//! - `validator_attribute`: validator attribute records.

#![doc(html_root_url = "https://docs.rs/neo-manifest/0.9.0")]

#[path = "protocol/call_flags.rs"]
pub mod call_flags;
pub mod manifest;
#[path = "nef/method_token.rs"]
pub mod method_token;
#[path = "nef/nef_file.rs"]
pub mod nef_file;
#[path = "protocol/validator_attribute.rs"]
pub mod validator_attribute;

pub use call_flags::CallFlags;
pub use manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, WildCardContainer,
};
pub use method_token::MethodToken;
pub use nef_file::NefFile;
pub use validator_attribute::ValidatorAttribute;
