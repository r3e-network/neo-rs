//! # neo-smart-contract-types
//!
//! Re-export facade for the pure-data smart-contract types
//! (`ContractManifest`, `ContractAbi`, `MethodToken`, `NefFile`,
//! `ContractParameterDefinition`, `ContractMethodDescriptor`,
//! `ContractEventDescriptor`, `ContractGroup`,
//! `ContractPermissionDescriptor`).
//! `ContractParameterType` lives in `neo_primitives`; consumers
//! should import it from there directly.
//!
//! The canonical implementations live in [`neo_manifest`]; this
//! crate is kept as a back-compat re-export facade so that historical
//! `use neo_smart_contract_types::*` paths keep compiling after the
//! `kill-neo-core` refactor extracted these types into their own
//! crate.
//!
//! New code should import the canonical types from [`neo_manifest`]
//! directly; this crate exists for back-compat.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends only on the Layer 0
//! crates `neo_primitives`, `neo_error`, `neo_io`, `neo_crypto`,
//! and the shared VM semantics crate `neo_vm_rs`.
//!
//! Must **not** depend on `neo-core` (deleted), `neo-vm` (Layer 1
//! stateful host), `neo-execution`, `neo-native-contracts`, or any
//! storage crate. This is the same rule `polkadot-sdk` and `reth`
//! apply to their `*-types` crates: keep the wire-protocol surface
//! independent of the runtime that consumes it.

#![doc(html_root_url = "https://docs.rs/neo-smart-contract-types/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub use neo_manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition,
    ContractPermissionDescriptor, MethodToken, NefFile, WildCardContainer,
};
