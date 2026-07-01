//! # neo-execution::native
//!
//! Native contract abstractions and registries used by execution.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `hardfork_activable`: hardfork activation trait helpers.
//! - `native_contract`: native contract trait and base behavior.
//! - `native_contract_cache`: native contract cache records.
//! - `native_contract_provider`: native contract provider trait.
//! - `native_registry`: native contract registry.

pub mod hardfork_activable;
pub mod native_contract;
/// Cached native-contract state used while composing native manifests and storage.
pub mod native_contract_cache;
pub mod native_contract_provider;
pub mod native_registry;
