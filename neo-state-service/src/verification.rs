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

use crate::commit_handlers::StateRootCalculator;
use crate::state_root::StateRoot;
use crate::state_store::StateStore;
use neo_storage::DataCache;
use std::sync::Arc;

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
pub struct Verifier {
    state_store: Arc<StateStore>,
    calculator: Arc<dyn StateRootCalculator>,
}

impl Verifier {
    /// Constructs a new verifier backed by the supplied state store
    /// and calculator.
    pub fn new(state_store: Arc<StateStore>, calculator: Arc<dyn StateRootCalculator>) -> Self {
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
mod tests {
    use super::*;
    use crate::commit_handlers::SyntheticStateRootCalculator;
    use neo_primitives::UInt256;
    use neo_storage::persistence::DataCache;

    #[test]
    fn accepted_when_claimed_matches_recomputed() {
        let store = Arc::new(StateStore::new());
        let calc = Arc::new(SyntheticStateRootCalculator);
        let verifier = Verifier::new(
            Arc::clone(&store),
            Arc::clone(&calc) as Arc<dyn StateRootCalculator>,
        );
        let snapshot = DataCache::new(false);
        let claimed = calc.compute(1, &snapshot).expect("calc");
        let outcome = verifier.verify(claimed, &snapshot);
        assert_eq!(outcome, VerifyOutcome::Accepted);
    }

    #[test]
    fn rejected_when_claimed_does_not_match() {
        let store = Arc::new(StateStore::new());
        let calc = Arc::new(SyntheticStateRootCalculator);
        let verifier = Verifier::new(Arc::clone(&store), calc);
        let snapshot = DataCache::new(false);
        let bogus = StateRoot::new_current(1, UInt256::from([0x99u8; 32]));
        let outcome = verifier.verify(bogus, &snapshot);
        assert_eq!(outcome, VerifyOutcome::Rejected);
    }
}
