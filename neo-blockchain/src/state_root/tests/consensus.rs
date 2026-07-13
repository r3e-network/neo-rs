use super::{StateRootVoteCollector, aggregate_state_root_witness, sign_state_root};
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_primitives::{UInt160, UInt256};
use neo_state_service::StateRoot;
use neo_vm::script_builder::RedeemScript;

const NETWORK: u32 = 0x4E45_4F4E;

/// N distinct StateValidator keypairs (private key, ECPoint).
fn validators(n: usize) -> Vec<([u8; 32], ECPoint)> {
    (0..n)
        .map(|i| {
            // Deterministic, distinct, valid secp256r1 keys.
            let mut sk = [0u8; 32];
            sk[31] = (i as u8) + 1;
            let pk = Secp256r1Crypto::derive_public_key(&sk).expect("derive pubkey");
            (sk, ECPoint::from_bytes(&pk).expect("ecpoint"))
        })
        .collect()
}

#[test]
fn m_votes_aggregate_into_the_state_validators_multisig_witness() {
    let vs = validators(4); // N=4 -> M = bft_threshold(4) = 3
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let m = RedeemScript::bft_threshold(pubkeys.len());
    assert_eq!(m, 3);

    let mut root = StateRoot::new_current(7, UInt256::from([0xABu8; 32]));
    let mut collector = StateRootVoteCollector::new();

    // First M-1 votes: no witness yet.
    for (idx, (sk, _)) in vs.iter().enumerate().take(m - 1) {
        let sig = sign_state_root(&mut root, sk, NETWORK).expect("sign");
        assert!(
            collector
                .add_vote(&mut root, idx, sig.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    // The M-th valid vote yields the aggregated, signed root.
    let idx = m - 1;
    let sig = sign_state_root(&mut root, &vs[idx].0, NETWORK).expect("sign");
    let signed = collector
        .add_vote(&mut root, idx, sig.to_vec(), &pubkeys, NETWORK)
        .expect("M valid votes must aggregate");

    let witness = signed.witness().expect("signed root carries a witness");
    // The witness's verification script is the StateValidators BFT multisig, so
    // its hash equals the BFT address checked by state-root verification.
    let expected = RedeemScript::bft_address(&pubkeys).expect("bft address");
    assert_eq!(UInt160::from_script(&witness.verification_script), expected);
    // Invocation pushes exactly M signatures (M * (1 opcode + 1 len + 64 bytes)).
    assert_eq!(witness.invocation_script.len(), m * (1 + 1 + 64));
}

#[test]
fn a_vote_with_a_wrong_signature_is_rejected() {
    let vs = validators(4);
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let mut root = StateRoot::new_current(7, UInt256::from([0xABu8; 32]));
    let mut collector = StateRootVoteCollector::new();

    // Validator 0's slot, but signed by validator 1's key -> invalid.
    let bogus = sign_state_root(&mut root, &vs[1].0, NETWORK).expect("sign");
    assert!(
        collector
            .add_vote(&mut root, 0, bogus.to_vec(), &pubkeys, NETWORK)
            .is_none(),
        "a signature that does not match the claimed validator index must be dropped"
    );
    // And a too-short signature.
    assert!(
        aggregate_state_root_witness(&Default::default(), &pubkeys).is_none(),
        "no votes -> no witness"
    );
}

#[test]
fn votes_for_competing_roots_at_the_same_index_cannot_mix() {
    let vs = validators(4); // N=4 -> M=3
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let mut root_a = StateRoot::new_current(7, UInt256::from([0xAA; 32]));
    let mut root_b = StateRoot::new_current(7, UInt256::from([0xBB; 32]));
    let mut collector = StateRootVoteCollector::new();

    for (idx, (sk, _)) in vs.iter().enumerate().take(2) {
        let sig = sign_state_root(&mut root_a, sk, NETWORK).expect("sign root A");
        assert!(
            collector
                .add_vote(&mut root_a, idx, sig.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    // This is the third signature at index 7, but only the first for root B.
    // A height-only pool would combine it with root A's signatures.
    let sig = sign_state_root(&mut root_b, &vs[2].0, NETWORK).expect("sign root B");
    assert!(
        collector
            .add_vote(&mut root_b, 2, sig.to_vec(), &pubkeys, NETWORK)
            .is_none(),
        "signatures for a competing root must not satisfy the threshold"
    );

    let sig = sign_state_root(&mut root_a, &vs[2].0, NETWORK).expect("sign root A");
    let signed_a = collector
        .add_vote(&mut root_a, 2, sig.to_vec(), &pubkeys, NETWORK)
        .expect("root A has three matching signatures");
    assert_eq!(signed_a.root_hash(), root_a.root_hash());

    for (idx, (sk, _)) in vs.iter().enumerate().take(2) {
        let sig = sign_state_root(&mut root_b, sk, NETWORK).expect("sign root B");
        let result = collector.add_vote(&mut root_b, idx, sig.to_vec(), &pubkeys, NETWORK);
        if idx == 0 {
            assert!(result.is_none());
        } else {
            let signed_b = result.expect("root B has three matching signatures");
            assert_eq!(signed_b.root_hash(), root_b.root_hash());
        }
    }
}

#[test]
fn votes_for_distinct_root_versions_cannot_mix() {
    let vs = validators(4); // N=4 -> M=3
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let root_hash = UInt256::from([0xAA; 32]);
    let mut version_zero = StateRoot::new(0, 7, root_hash);
    let mut version_one = StateRoot::new(1, 7, root_hash);
    let mut collector = StateRootVoteCollector::new();

    for (idx, (sk, _)) in vs.iter().enumerate().take(2) {
        let sig = sign_state_root(&mut version_zero, sk, NETWORK).expect("sign version zero");
        assert!(
            collector
                .add_vote(&mut version_zero, idx, sig.to_vec(), &pubkeys, NETWORK,)
                .is_none()
        );
    }

    let sig = sign_state_root(&mut version_one, &vs[2].0, NETWORK).expect("sign version one");
    assert!(
        collector
            .add_vote(&mut version_one, 2, sig.to_vec(), &pubkeys, NETWORK,)
            .is_none(),
        "signatures for a different wire version must not satisfy the threshold"
    );
}

#[test]
fn votes_for_distinct_block_indexes_cannot_mix() {
    let vs = validators(4); // N=4 -> M=3
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let root_hash = UInt256::from([0xAA; 32]);
    let mut index_seven = StateRoot::new_current(7, root_hash);
    let mut index_eight = StateRoot::new_current(8, root_hash);
    let mut collector = StateRootVoteCollector::new();

    for (idx, (sk, _)) in vs.iter().enumerate().take(2) {
        let sig = sign_state_root(&mut index_seven, sk, NETWORK).expect("sign index seven");
        assert!(
            collector
                .add_vote(&mut index_seven, idx, sig.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    let sig = sign_state_root(&mut index_eight, &vs[2].0, NETWORK).expect("sign index eight");
    assert!(
        collector
            .add_vote(&mut index_eight, 2, sig.to_vec(), &pubkeys, NETWORK)
            .is_none(),
        "signatures for a different block index must not satisfy the threshold"
    );

    let sig = sign_state_root(&mut index_seven, &vs[2].0, NETWORK).expect("sign index seven");
    assert!(
        collector
            .add_vote(&mut index_seven, 2, sig.to_vec(), &pubkeys, NETWORK)
            .is_some(),
        "three signatures for the same block index must aggregate"
    );
}

#[test]
fn votes_for_different_networks_cannot_mix() {
    let vs = validators(4);
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, point)| point.clone()).collect();
    let mut root = StateRoot::new_current(7, UInt256::from([0xAA; 32]));
    let mut collector = StateRootVoteCollector::new();

    for (index, (private_key, _)) in vs.iter().enumerate().take(2) {
        let signature = sign_state_root(&mut root, private_key, NETWORK).expect("sign root");
        assert!(
            collector
                .add_vote(&mut root, index, signature.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    let other_network = NETWORK ^ 1;
    let signature =
        sign_state_root(&mut root, &vs[2].0, other_network).expect("sign other network");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &pubkeys, other_network,)
            .is_none(),
        "votes accepted under another network must not enter the existing pool"
    );

    let signature = sign_state_root(&mut root, &vs[2].0, NETWORK).expect("sign root");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &pubkeys, NETWORK)
            .is_some()
    );
}

#[test]
fn votes_for_reordered_validators_cannot_mix() {
    let vs = validators(4);
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, point)| point.clone()).collect();
    let mut root = StateRoot::new_current(7, UInt256::from([0xAA; 32]));
    let mut collector = StateRootVoteCollector::new();

    for (index, (private_key, _)) in vs.iter().enumerate().take(2) {
        let signature = sign_state_root(&mut root, private_key, NETWORK).expect("sign root");
        assert!(
            collector
                .add_vote(&mut root, index, signature.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    let mut reordered = pubkeys.clone();
    reordered.swap(2, 3);
    let signature = sign_state_root(&mut root, &vs[3].0, NETWORK).expect("sign reordered slot");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &reordered, NETWORK)
            .is_none(),
        "a reordered validator context must not enter the existing pool"
    );

    let signature = sign_state_root(&mut root, &vs[2].0, NETWORK).expect("sign root");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &pubkeys, NETWORK)
            .is_some()
    );
}

#[test]
fn votes_for_changed_validators_cannot_mix() {
    let vs = validators(5);
    let pubkeys: Vec<ECPoint> = vs[..4].iter().map(|(_, point)| point.clone()).collect();
    let mut root = StateRoot::new_current(7, UInt256::from([0xAA; 32]));
    let mut collector = StateRootVoteCollector::new();

    for (index, (private_key, _)) in vs.iter().enumerate().take(2) {
        let signature = sign_state_root(&mut root, private_key, NETWORK).expect("sign root");
        assert!(
            collector
                .add_vote(&mut root, index, signature.to_vec(), &pubkeys, NETWORK)
                .is_none()
        );
    }

    let mut changed = pubkeys.clone();
    changed[2] = vs[4].1.clone();
    let signature = sign_state_root(&mut root, &vs[4].0, NETWORK).expect("sign changed slot");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &changed, NETWORK)
            .is_none(),
        "a changed validator context must not enter the existing pool"
    );

    let signature = sign_state_root(&mut root, &vs[2].0, NETWORK).expect("sign root");
    assert!(
        collector
            .add_vote(&mut root, 2, signature.to_vec(), &pubkeys, NETWORK)
            .is_some()
    );
}

#[test]
fn pruning_removes_every_competing_root_below_the_index() {
    let vs = validators(4); // N=4 -> M=3
    let pubkeys: Vec<ECPoint> = vs.iter().map(|(_, p)| p.clone()).collect();
    let mut old_a = StateRoot::new_current(7, UInt256::from([0xAA; 32]));
    let mut old_b = StateRoot::new_current(7, UInt256::from([0xBB; 32]));
    let mut retained = StateRoot::new_current(8, UInt256::from([0xCC; 32]));
    let mut collector = StateRootVoteCollector::new();

    for (idx, (private_key, _)) in vs.iter().enumerate().take(2) {
        for root in [&mut old_a, &mut old_b, &mut retained] {
            let sig = sign_state_root(root, private_key, NETWORK).expect("sign root");
            assert!(
                collector
                    .add_vote(root, idx, sig.to_vec(), &pubkeys, NETWORK)
                    .is_none()
            );
        }
    }

    collector.prune_below(8);

    for root in [&mut old_a, &mut old_b] {
        let sig = sign_state_root(root, &vs[2].0, NETWORK).expect("sign old root");
        assert!(
            collector
                .add_vote(root, 2, sig.to_vec(), &pubkeys, NETWORK)
                .is_none(),
            "every pool below the pruning index must be discarded"
        );
    }

    let sig = sign_state_root(&mut retained, &vs[2].0, NETWORK).expect("sign retained root");
    assert!(
        collector
            .add_vote(&mut retained, 2, sig.to_vec(), &pubkeys, NETWORK)
            .is_some(),
        "pools at the pruning index must be retained"
    );
}
