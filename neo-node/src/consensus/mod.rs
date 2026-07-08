//! # neo-node::consensus
//!
//! Consensus-facing node adapters and startup helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `driver`: validator-node dBFT driver task and event routing.
//! - `hsm`: node-side HSM signer wiring.
//! - `payload`: dBFT extensible-payload codec helpers.
//! - `proposal`: consensus proposal construction helpers.
//! - `setup`: validator-set, signer, and consensus startup setup helpers.
//! - `tests`: Module-local tests and regression coverage.

mod driver;
mod hsm;
mod native_provider;
mod payload;
mod proposal;
mod setup;

pub use driver::consensus_driver_task;
pub use hsm::HsmKeyConfig;
pub use payload::extensible_to_consensus;
pub use setup::build_consensus_setup;

#[cfg(test)]
use driver::ConsensusDriver;
#[cfg(test)]
use payload::{DBFT_CATEGORY, consensus_to_extensible};
#[cfg(test)]
use proposal::{
    cache_available_proposal_transactions, expected_dbft_block_size_without_transactions,
    prepare_request_passes_ledger_guards, proposal_rejection_reason,
    select_primary_proposal_transactions,
};
#[cfg(test)]
pub use setup::build_consensus_validators;

/// C# DBFTPlugin `DbftSettings.MaxBlockSystemFee` default. This value is
/// unchanged in the Neo v3.10.1 reference node.
const DBFT_MAX_BLOCK_SYSTEM_FEE: i64 = 150_000_000_000;

#[cfg(test)]
#[path = "../tests/consensus/mod.rs"]
mod tests;
