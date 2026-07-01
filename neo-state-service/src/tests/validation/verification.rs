use super::*;
use neo_crypto::Crypto;
use neo_primitives::UInt256;
use neo_storage::persistence::DataCache;

struct ChangeSetStateRootCalculator;

impl StateRootCalculator for ChangeSetStateRootCalculator {
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> CoreResult<StateRoot> {
        let mut buf = Vec::new();
        for key in snapshot.get_change_set() {
            buf.extend_from_slice(&key.to_array());
        }
        let root_hash = UInt256::from(Crypto::sha256(&buf));
        Ok(StateRoot::new_current(block_index, root_hash))
    }
}

#[test]
fn accepted_when_claimed_matches_recomputed() {
    let store = Arc::new(StateStore::new());
    let calc = Arc::new(ChangeSetStateRootCalculator);
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
fn accepts_concrete_calculator_without_trait_object() {
    let store = Arc::new(StateStore::new());
    let verifier = Verifier::new(Arc::clone(&store), ChangeSetStateRootCalculator);
    let snapshot = DataCache::new(false);
    let claimed = ChangeSetStateRootCalculator
        .compute(1, &snapshot)
        .expect("calc");
    let outcome = verifier.verify(claimed, &snapshot);
    assert_eq!(outcome, VerifyOutcome::Accepted);
}

#[test]
fn rejected_when_claimed_does_not_match() {
    let store = Arc::new(StateStore::new());
    let calc = Arc::new(ChangeSetStateRootCalculator);
    let verifier = Verifier::new(Arc::clone(&store), calc);
    let snapshot = DataCache::new(false);
    let bogus = StateRoot::new_current(1, UInt256::from([0x99u8; 32]));
    let outcome = verifier.verify(bogus, &snapshot);
    assert_eq!(outcome, VerifyOutcome::Rejected);
}
