//! # neo-manifest
//!
//! Canonical home for the smart-contract manifest / ABI / NEF / CallFlags /
//! MethodToken / ValidatorAttribute data types.
//!
//! Mirrors `Neo.SmartContract.Manifest` for the contract descriptor types
//! and `Neo.SmartContract.NefFile` for the on-wire script container. The
//! stateful execution engine, native contracts, and VM interop (StackValue
//! / StackItem round-trip) live elsewhere (`neo-execution`,
//! `neo-native-contracts`, and an extension trait in `neo-core`).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends on:
//! - `neo-primitives` (Layer 0) — for `UInt160`, `ECPoint`,
//!   `ContractParameterType`.
//! - `neo-error` (Layer 0) — for `CoreError` / `CoreResult`.
//! - `neo-io` (Layer 0) — for `Serializable` + `impl_serializable!`.
//! - `neo-crypto` (Layer 0) — for `ECPoint` validation.
//! - `neo-vm-rs` (Layer 0) — for opcode metadata only.
//!
//! Must **not** depend on `neo-core` (Layer 2 runtime), `neo-vm` (Layer 1
//! stateful host), `neo-execution`, `neo-native-contracts`, or any storage
//! crate. This is the same rule polkadot-sdk and reth apply to their
//! `*-types` crates: keep the wire-protocol surface independent of the
//! runtime that consumes it.
//!
//! ## Status
//!
//! The fully-self-contained `MethodToken` (NEP static-call descriptor)
//! and `NefFile` (NEF3 wire container) data types, the `CallFlags`
//! re-export from `neo-primitives`, and the `ValidatorAttribute` trait
//! have been moved here. The full `manifest::*` types remain in
//! `neo_core::smart_contract::manifest` because they depend on the
//! smart-contract `Interoperable` trait and the stateful VM host, both
//! of which still live in `neo-core`. A follow-up refactor will move
//! the `Interoperable` trait to a foundation-layer crate and then
//! extract the manifest types from `neo-core` into this crate.

#![doc(html_root_url = "https://docs.rs/neo-manifest/0.8.0")]

pub mod call_flags;
pub mod manifest;
pub mod method_token;
pub mod nef_file;
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
