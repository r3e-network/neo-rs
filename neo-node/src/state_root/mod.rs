//! # neo-node::state_root
//!
//! Active signed-StateRoot (StateValidators) consensus driver.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics. The deterministic vote/aggregate/verify core lives in
//! `neo-blockchain` (`neo_blockchain::StateRootVoteCollector`,
//! `neo_blockchain::verify_state_root_with_native_provider`); this module is
//! the node-side driver that feeds it network payloads and persists the
//! finalized signed root.
//!
//! ## C# reference
//!
//! Mirrors `Neo.Plugins.StateService`:
//! - `StatePlugin` (extensible category `"StateService"`, block-persist hook),
//! - `VerificationService` (inbound routing, vote/state-root relay, timers),
//! - `VerificationContext` (per-round verifier set, my-index, sender rotation,
//!   vote signing, `M`-of-`N` aggregation).
//!
//! ## Contents
//!
//! - `codec`: StateService extensible `<-> {Vote, StateRoot}` helpers.
//! - `driver`: single-task voting, relay, and signed-root persistence driver.
//! - `setup`: [`StateRootSetup`] and [`build_state_root_setup`] key resolution.
//! - `tests`: module-local codec and driver regression coverage.

mod codec;
mod driver;
mod setup;

pub use driver::state_root_driver_task;
pub use setup::{StateRootSetup, build_state_root_setup};

#[cfg(test)]
use codec::{
    STATE_ROOT_VALID_BLOCK_END_THRESHOLD, VOTE_VALID_BLOCK_END_THRESHOLD, build_extensible,
    decode_message,
};
#[cfg(test)]
use driver::StateRootDriver;

#[cfg(test)]
#[path = "../tests/state_root/mod.rs"]
mod tests;
