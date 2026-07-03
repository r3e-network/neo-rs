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
    // its hash equals the BFT address that verify_state_root checks against.
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
