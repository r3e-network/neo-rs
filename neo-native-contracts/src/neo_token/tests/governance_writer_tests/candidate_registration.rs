use super::*;

#[test]
fn register_then_unregister_candidate_round_trip() {
    let pubkey = candidate_pubkey();
    let pubkey_bytes = pubkey.to_bytes();
    let account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let snapshot = seeded_snapshot();

    // Register (signed by the candidate's account) -> Registered with 0 votes.
    let state = call(
        Arc::clone(&snapshot),
        account,
        &pubkey_bytes,
        "registerCandidate",
    );
    assert_eq!(state, VmState::HALT, "registerCandidate must HALT");
    let item = snapshot
        .get(&NeoToken::candidate_key(&pubkey))
        .expect("candidate entry written");
    let (registered, votes) = NeoToken::decode_candidate_state(&item.value_bytes()).unwrap();
    assert!(registered, "candidate is Registered");
    assert_eq!(votes, BigInt::from(0));
    assert_eq!(
        NeoToken::new()
            .read_registered_candidates(&snapshot)
            .unwrap()
            .len(),
        1
    );

    // Unregister -> the zero-vote entry is removed.
    let state2 = call(
        Arc::clone(&snapshot),
        account,
        &pubkey_bytes,
        "unregisterCandidate",
    );
    assert_eq!(state2, VmState::HALT, "unregisterCandidate must HALT");
    assert!(
        snapshot.get(&NeoToken::candidate_key(&pubkey)).is_none(),
        "zero-vote candidate entry removed"
    );
}

#[test]
fn unregister_candidate_deletes_stale_voter_reward_entry() {
    // C# `CheckCandidate` (NeoToken.cs:191): unregistering a candidate with
    // no remaining votes deletes BOTH the candidate AND the
    // `Prefix_VoterRewardPerCommittee` entry.
    let pubkey = candidate_pubkey();
    let pubkey_bytes = pubkey.to_bytes();
    let account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let snapshot = seeded_snapshot();

    snapshot.update(
        NeoToken::candidate_key(&pubkey),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(0)).unwrap()),
    );
    let reward_key = NeoToken::voter_reward_per_committee_key(&pubkey);
    snapshot.add(
        reward_key.clone(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(123_456))),
    );

    let state = call(
        Arc::clone(&snapshot),
        account,
        &pubkey_bytes,
        "unregisterCandidate",
    );
    assert_eq!(state, VmState::HALT, "unregisterCandidate must HALT");
    assert!(
        snapshot.get(&NeoToken::candidate_key(&pubkey)).is_none(),
        "candidate entry removed"
    );
    assert!(
        snapshot.get(&reward_key).is_none(),
        "stale voter-reward entry removed"
    );
}

#[test]
fn register_candidate_requires_the_candidate_witness() {
    let pubkey = candidate_pubkey();
    let pubkey_bytes = pubkey.to_bytes();
    let wrong = UInt160::from_bytes(&[0x09; 20]).unwrap();
    let snapshot = seeded_snapshot();

    let state = call(
        Arc::clone(&snapshot),
        wrong,
        &pubkey_bytes,
        "registerCandidate",
    );
    assert_eq!(state, VmState::HALT);
    assert!(
        snapshot.get(&NeoToken::candidate_key(&pubkey)).is_none(),
        "no candidate registered without its witness"
    );
}
