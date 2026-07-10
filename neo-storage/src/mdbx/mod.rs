//! # neo-storage::mdbx
//!
//! Production default MDBX provider and store adapter.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `provider`: Provider adapter for the surrounding trait boundary.
//! - `snapshot`: Read snapshot view for the surrounding store backend.
//! - `store`: Store implementation for the surrounding backend or domain.
//! - `tests`: Module-local tests and regression coverage.

mod provider;
mod snapshot;
mod store;

pub use provider::{MdbxStoreProvider, MdbxTuning};
pub use snapshot::MdbxSnapshot;
pub use store::MdbxStore;

#[cfg(test)]
#[path = "../tests/mdbx/mod.rs"]
mod tests;
