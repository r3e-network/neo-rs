//! # neo-consensus::context
//!
//! Runtime context records carried through the local workflow.
//!
//! ## Boundary
//!
//! This module belongs to `neo-consensus`. This protocol/service crate owns
//! dBFT state and messages and must not own ledger persistence, RPC transport,
//! or application startup.
//!
//! ## Contents
//!
//! - `construction`: fresh context defaults for a new dBFT round.
//! - `liveness`: validator liveness, failure, and view-change guards.
//! - `model`: dBFT context data model.
//! - `persistence`: Persistence traits, snapshots, transactions, and cache
//!   overlays.
//! - `policy`: dBFT context defaults and bounded-cache limits.
//! - `quorum`: validator counts, speaker role, dBFT thresholds, and quorum
//!   checks.
//! - `replay`: bounded message-hash replay protection.
//! - `round`: view/block lifecycle resets.
//! - `signatures`: prepare, commit, and change-view payload mutation helpers.
//! - `state`: domain state records for the surrounding workflow.
//! - `timer`: consensus timer policy and scheduling helpers.
//! - `transactions`: proposal transaction availability and block-policy math.
//! - `validator_info`: validator metadata records.
//! - `tests`: Module-local tests and regression coverage.

mod construction;
mod liveness;
mod model;
mod persistence;
mod policy;
mod quorum;
mod replay;
mod round;
mod signatures;
mod state;
mod timer;
mod transactions;
mod validator_info;

pub use model::ConsensusContext;
pub use policy::{
    BLOCK_TIME_MS, DEFAULT_BLOCK_TIME_MS, DEFAULT_MAX_BLOCK_SIZE, DEFAULT_MAX_BLOCK_SYSTEM_FEE,
    MAX_MESSAGE_CACHE_SIZE, MAX_VALIDATORS,
};
pub use state::ConsensusState;
pub use transactions::TxMetrics;
pub use validator_info::ValidatorInfo;

#[cfg(test)]
#[path = "../tests/context/mod.rs"]
mod tests;
