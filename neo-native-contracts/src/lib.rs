//! Native contract definitions, metadata, and helper macros.

/// Deterministic genesis block construction from protocol settings.
pub mod genesis;
pub mod native;

// Re-export the native-contract surface at the crate root so downstream crates
// (neo-ledger, neo-system) can keep using the pre-decomposition
// `neo_native_contracts::X` paths that `neo-core` used to provide.
pub use native::*;

// Genesis lives here (not in neo-core) so the native-contract storage
// initialization and the genesis block that seeds it stay in one crate.
pub use genesis::create_genesis_block;
