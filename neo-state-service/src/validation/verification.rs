//! State-root verification pipeline.
//!
//! Receives a candidate [`StateRoot`] (typically from a peer over
//! the network) and runs the verification step:
//!
//! 1. Recompute the state root from the supplied [`DataCache`]
//!    snapshot.
//! 2. Compare the recomputed root with the candidate's claimed
//!    root hash.
//! 3. Mark the candidate as validated (via
//!    [`StateStore::commit_validated_state_roots`]) or discard it
//!    (via [`StateStore::discard`]).
//!
//! Mirrors the C# `StateService.Verification` actor's
//! `VerifyStateRoot` request.

use crate::state_root::StateRoot;
use crate::state_store::StateStore;
use neo_error::CoreResult;
use neo_storage::DataCache;
use std::sync::Arc;

/// Result of a state-root calculation used by the verification pipeline.
pub trait StateRootCalculator: Send + Sync {
    /// Computes the state root for the block's storage change set.
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> CoreResult<StateRoot>;
}

impl<T> StateRootCalculator for Arc<T>
where
    T: StateRootCalculator + ?Sized,
{
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> CoreResult<StateRoot> {
        self.as_ref().compute(block_index, snapshot)
    }
}

impl<T> StateRootCalculator for Box<T>
where
    T: StateRootCalculator + ?Sized,
{
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> CoreResult<StateRoot> {
        self.as_ref().compute(block_index, snapshot)
    }
}

/// Outcome of a [`Verifier::verify`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// The candidate's claimed root hash matches the recomputed
    /// root. The candidate is now a validated state root.
    Accepted,
    /// The candidate's claimed root hash does NOT match the
    /// recomputed root. The candidate is discarded.
    Rejected,
    /// The underlying calculator returned an error.
    CalculationError,
}

/// State-root verifier.
pub struct Verifier<C = Arc<dyn StateRootCalculator>>
where
    C: StateRootCalculator,
{
    state_store: Arc<StateStore>,
    calculator: C,
}

impl<C> Verifier<C>
where
    C: StateRootCalculator,
{
    /// Constructs a new verifier backed by the supplied state store
    /// and calculator.
    pub fn new(state_store: Arc<StateStore>, calculator: C) -> Self {
        Self {
            state_store,
            calculator,
        }
    }

    /// Verifies the supplied candidate state root against the
    /// supplied snapshot.
    pub fn verify(&self, candidate: StateRoot, snapshot: &DataCache) -> VerifyOutcome {
        let index = candidate.index();
        match self.calculator.compute(index, snapshot) {
            Ok(recomputed) => {
                if recomputed.root_hash() == candidate.root_hash() {
                    self.state_store.commit_validated_state_roots(&[candidate]);
                    VerifyOutcome::Accepted
                } else {
                    self.state_store.discard(candidate.root_hash());
                    VerifyOutcome::Rejected
                }
            }
            Err(_) => {
                self.state_store.discard(candidate.root_hash());
                VerifyOutcome::CalculationError
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/validation/verification.rs"]
mod tests;
