//! # neo-manifest
//!
//! Contract manifest, NEF, ABI, permission, and method-token domain types.
//!
//! ## Boundary
//!
//! This module belongs to neo-manifest and must respect the workspace layer
//! boundaries.
//!
//! ## Contents
//!
//! - `manifest`: Contract manifest, ABI, permission, and NEF-adjacent metadata
//!   types.
//! - `method_token`: NEF method-token records.
//! - `nef_file`: NEF file records and checksum logic.

#![doc(html_root_url = "https://docs.rs/neo-manifest/0.10.0")]

pub mod manifest;
#[path = "nef/method_token.rs"]
pub mod method_token;
#[path = "nef/nef_file.rs"]
pub mod nef_file;

pub use manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, ManifestExtra, ManifestFeatures, WildCardContainer,
};
pub use method_token::MethodToken;
pub use nef_file::NefFile;
