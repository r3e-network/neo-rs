use super::*;

#[test]
fn native_contract_surface() {
    let c = NeoToken::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "symbol",
            "decimals",
            "totalSupply",
            "balanceOf",
            "transfer",
            "getGasPerBlock",
            "getRegisterPrice",
            "getCommittee",
            "getCommitteeAddress",
            "getAccountState",
            "unclaimedGas",
            "getNextBlockValidators",
            "getCandidates",
            "getAllCandidates",
            "getCandidateVote",
            "setRegisterPrice",
            "setGasPerBlock",
            "registerCandidate",   // V0 (genesis, DeprecatedIn Echidna)
            "registerCandidate",   // V1 (ActiveIn Echidna, +AllowNotify)
            "unregisterCandidate", // V0
            "unregisterCandidate", // V1
            "vote",                // V0
            "vote",                // V1
            "onNEP17Payment"
        ]
    );
    // The governance writers: not safe, States, Integer -> Void, CpuFee 1<<15.
    for name in ["setRegisterPrice", "setGasPerBlock"] {
        let w = c.methods().iter().find(|m| m.name == name).unwrap();
        assert!(!w.safe);
        assert_eq!(w.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(w.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(w.return_type, ContractParameterType::Void);
        assert_eq!(w.cpu_fee, 1 << 15);
    }
    // Candidate writers: dual registration (C# V0/V1). V0 is genesis-active
    // with States + DeprecatedIn Echidna; V1 is ActiveIn Echidna and adds
    // AllowNotify. registerCandidate has no manifest CpuFee, unregister is 1<<16.
    let states = CallFlags::STATES.bits();
    let notify_flags = CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits();
    for (name, fee) in [
        ("registerCandidate", 0i64),
        ("unregisterCandidate", 1 << 16),
    ] {
        let versions: Vec<&NativeMethod> = c.methods().iter().filter(|m| m.name == name).collect();
        assert_eq!(versions.len(), 2, "{name} is a dual V0/V1 registration");
        let (v0, v1) = (versions[0], versions[1]);
        assert!(!v0.safe && !v1.safe, "{name} not safe");
        assert_eq!(
            v0.parameters,
            vec![ContractParameterType::PublicKey],
            "{name} params"
        );
        assert_eq!(
            v0.return_type,
            ContractParameterType::Boolean,
            "{name} return"
        );
        assert_eq!(v0.cpu_fee, fee, "{name} cpu_fee");
        // V0: genesis-active, States, deprecated at Echidna.
        assert_eq!(v0.required_call_flags, states, "{name} V0 flags");
        assert_eq!(v0.active_in, None, "{name} V0 genesis-active");
        assert_eq!(
            v0.deprecated_in,
            Some(Hardfork::HfEchidna),
            "{name} V0 deprecated"
        );
        // V1: active at Echidna, States|AllowNotify.
        assert_eq!(v1.required_call_flags, notify_flags, "{name} V1 flags");
        assert_eq!(v1.active_in, Some(Hardfork::HfEchidna), "{name} V1 active");
    }
    let acct = c
        .methods()
        .iter()
        .find(|m| m.name == "getAccountState")
        .unwrap();
    assert_eq!(acct.parameters, vec![ContractParameterType::Hash160]);
    assert_eq!(acct.return_type, ContractParameterType::Array);
    assert_eq!(acct.cpu_fee, 1 << 15);
    let nbv = c
        .methods()
        .iter()
        .find(|m| m.name == "getNextBlockValidators")
        .unwrap();
    assert_eq!(nbv.return_type, ContractParameterType::Array);
    assert_eq!(nbv.cpu_fee, 1 << 16);
    assert!(nbv.parameters.is_empty());
    let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
    assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
    let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
    assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());

    let committee = c
        .methods()
        .iter()
        .find(|m| m.name == "getCommittee")
        .unwrap();
    assert_eq!(committee.cpu_fee, 1 << 16);
    assert_eq!(committee.return_type, ContractParameterType::Array);
    assert!(committee.active_in.is_none());
    let addr = c
        .methods()
        .iter()
        .find(|m| m.name == "getCommitteeAddress")
        .unwrap();
    assert_eq!(addr.cpu_fee, 1 << 16);
    assert_eq!(addr.return_type, ContractParameterType::Hash160);
    assert_eq!(addr.active_in, Some(Hardfork::HfCockatrice));
    // getAllCandidates: safe ungated iterator reader (ReadStates, CpuFee
    // 1<<22, no params, InteropInterface).
    let all_cand = c
        .methods()
        .iter()
        .find(|m| m.name == "getAllCandidates")
        .unwrap();
    assert!(all_cand.safe);
    assert_eq!(all_cand.cpu_fee, 1 << 22);
    assert_eq!(all_cand.required_call_flags, CallFlags::READ_STATES.bits());
    assert!(all_cand.parameters.is_empty());
    assert_eq!(
        all_cand.return_type,
        ContractParameterType::InteropInterface
    );
    assert!(all_cand.active_in.is_none());
    // onNEP17Payment: Echidna-gated candidate registration via GAS payment
    // — States|AllowNotify -> not safe, Void, no manifest CpuFee.
    let pay = c
        .methods()
        .iter()
        .find(|m| m.name == "onNEP17Payment")
        .unwrap();
    assert!(!pay.safe);
    assert_eq!(pay.cpu_fee, 0);
    assert_eq!(pay.required_call_flags, notify_flags);
    assert_eq!(
        pay.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::Any
        ]
    );
    assert_eq!(pay.return_type, ContractParameterType::Void);
    assert_eq!(pay.active_in, Some(Hardfork::HfEchidna));
}

#[test]
fn storage_key_helpers_match_csharp_layout() {
    let pubkey = ECPoint::from_bytes(&hex(
        "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
    ))
    .unwrap();
    let pubkey_bytes = pubkey.to_bytes();

    let voters_count = NeoToken::voters_count_key();
    assert_eq!(voters_count.id(), NeoToken::ID);
    assert_eq!(voters_count.suffix(), &[PREFIX_VOTERS_COUNT]);

    let register_price = NeoToken::register_price_key();
    assert_eq!(register_price.id(), NeoToken::ID);
    assert_eq!(register_price.suffix(), &[PREFIX_REGISTER_PRICE]);

    let committee = NeoToken::committee_key();
    assert_eq!(committee.id(), NeoToken::ID);
    assert_eq!(committee.suffix(), &[PREFIX_COMMITTEE]);

    let gas_prefix = NeoToken::gas_per_block_prefix_key();
    assert_eq!(gas_prefix.id(), NeoToken::ID);
    assert_eq!(gas_prefix.suffix(), &[PREFIX_GAS_PER_BLOCK]);

    let gas_record = NeoToken::gas_per_block_key(0x0102_0304);
    assert_eq!(gas_record.id(), NeoToken::ID);
    assert_eq!(gas_record.suffix(), &[PREFIX_GAS_PER_BLOCK, 1, 2, 3, 4]);
    assert!(gas_record.suffix().starts_with(gas_prefix.suffix()));

    let candidate_prefix = NeoToken::candidate_prefix_key();
    assert_eq!(candidate_prefix.id(), NeoToken::ID);
    assert_eq!(candidate_prefix.suffix(), &[PREFIX_CANDIDATE]);

    let candidate = NeoToken::candidate_key(&pubkey);
    assert_eq!(candidate.id(), NeoToken::ID);
    assert_eq!(candidate.suffix()[0], PREFIX_CANDIDATE);
    assert_eq!(&candidate.suffix()[1..], pubkey_bytes.as_slice());

    let reward = NeoToken::voter_reward_per_committee_key(&pubkey);
    assert_eq!(reward.id(), NeoToken::ID);
    assert_eq!(reward.suffix()[0], PREFIX_VOTER_REWARD_PER_COMMITTEE);
    assert_eq!(&reward.suffix()[1..], pubkey_bytes.as_slice());
}

use crate::test_support::{hex, sample_committee, seed_committee};

#[test]
fn committee_threshold_is_majority() {
    // m = n - (n - 1) / 2.
    assert_eq!(NeoToken::committee_threshold(1), 1);
    assert_eq!(NeoToken::committee_threshold(3), 2);
    assert_eq!(NeoToken::committee_threshold(4), 3);
    assert_eq!(NeoToken::committee_threshold(7), 4);
    assert_eq!(NeoToken::committee_threshold(21), 11);
}

#[test]
fn committee_read_decodes_and_sorts() {
    let cache = DataCache::new(false);
    let points = sample_committee();
    seed_committee(&cache, &points);

    // Decoded points round-trip (stored order).
    let read = NeoToken::new().read_committee_points(&cache).unwrap();
    assert_eq!(read, points);

    // getCommittee returns them sorted ascending (C# OrderBy).
    let mut expected = points.clone();
    expected.sort();
    assert_eq!(NeoToken::new().committee_sorted(&cache).unwrap(), expected);
}

#[test]
fn next_block_validators_takes_count_then_sorts() {
    let cache = DataCache::new(false);
    let points = sample_committee(); // 3 stored points
    seed_committee(&cache, &points);

    // Take the first 2 (stored order), then sort ascending.
    let result = NeoToken::new().next_block_validators(&cache, 2).unwrap();
    let mut expected: Vec<ECPoint> = points[..2].to_vec();
    expected.sort();
    assert_eq!(result, expected);

    // A count >= committee size returns all members, sorted.
    let mut all_expected = points.clone();
    all_expected.sort();
    assert_eq!(
        NeoToken::new().next_block_validators(&cache, 10).unwrap(),
        all_expected
    );
}

#[test]
fn next_block_validator_account_matches_sorted_validator_account() {
    let cache = DataCache::new(false);
    let points = sample_committee();
    seed_committee(&cache, &points);
    let neo = NeoToken::new();
    let validators = neo.next_block_validators(&cache, 2).unwrap();

    assert_eq!(
        neo.next_block_validator_account(&cache, 2, 1).unwrap(),
        crate::neo_token::candidate_signature_account(&validators[1])
    );
    assert!(neo.next_block_validator_account(&cache, 2, 2).is_err());
}

#[test]
fn candidates_filters_registered_and_decodes_votes() {
    use neo_storage::StorageItem;
    let cache = DataCache::new(false);
    let points = sample_committee(); // 3 valid points

    // p0 registered w/ 100 votes, p1 unregistered, p2 registered w/ 50 votes.
    for (pk, registered, votes) in [
        (&points[0], true, 100i64),
        (&points[1], false, 0),
        (&points[2], true, 50),
    ] {
        let state = StackItem::from_struct(vec![
            StackItem::from_bool(registered),
            StackItem::from_int(votes),
        ]);
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        let key = NeoToken::candidate_key(pk);
        cache.add(key, StorageItem::from_bytes(bytes));
    }

    let candidates = NeoToken::new().read_registered_candidates(&cache).unwrap();
    // Only the two registered candidates are returned.
    assert_eq!(candidates.len(), 2);
    let by_key: std::collections::HashMap<Vec<u8>, BigInt> = candidates
        .iter()
        .map(|(pk, v)| (pk.to_bytes(), v.clone()))
        .collect();
    assert_eq!(by_key.get(&points[0].to_bytes()), Some(&BigInt::from(100)));
    assert_eq!(by_key.get(&points[2].to_bytes()), Some(&BigInt::from(50)));
    assert!(!by_key.contains_key(&points[1].to_bytes()));
}

#[test]
fn neo_public_array_return_encoders_use_stack_value_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let points = sample_committee();
    let large_votes = (BigInt::from(i64::MAX) + BigInt::from(1u8)) * BigInt::from(2u8);
    let candidates = vec![
        (points[0].clone(), BigInt::from(100)),
        (points[1].clone(), large_votes.clone()),
    ];

    let legacy_candidates = StackItem::from_array(vec![
        StackItem::from_struct(vec![
            StackItem::from_byte_string(points[0].to_bytes()),
            StackItem::from_int(100),
        ]),
        StackItem::from_struct(vec![
            StackItem::from_byte_string(points[1].to_bytes()),
            StackItem::from_int(large_votes),
        ]),
    ]);
    let expected_candidates =
        BinarySerializer::serialize(&legacy_candidates, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        NeoToken::candidates_to_array_bytes(&candidates).unwrap(),
        expected_candidates
    );

    let legacy_points = StackItem::from_array(
        points
            .iter()
            .map(|point| StackItem::from_byte_string(point.to_bytes()))
            .collect::<Vec<_>>(),
    );
    let expected_points =
        BinarySerializer::serialize(&legacy_points, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        NeoToken::points_to_array_bytes(&points).unwrap(),
        expected_points
    );

    let candidate_storage_source = include_str!("../../neo_token/storage/candidates.rs");
    let points_storage_source = include_str!("../../neo_token/storage/points.rs");
    let persist_source = include_str!("../../neo_token/persist.rs");
    let candidate_encoder = slice_between(
        candidate_storage_source,
        "fn candidates_to_array_bytes",
        "fn candidate_is_blocked",
    );
    assert!(candidate_encoder.contains("StackValue::Array"));
    assert!(candidate_encoder.contains("StackValue::Struct"));
    assert!(candidate_encoder.contains("serialize_stack_value_default"));
    assert!(!candidate_encoder.contains("StackItem::from_array"));
    assert!(!candidate_encoder.contains("BinarySerializer::serialize("));

    let points_value_projector = slice_between(
        points_storage_source,
        "fn points_to_stack_value",
        "fn points_to_array_bytes",
    );
    assert!(points_value_projector.contains("StackValue::Array"));
    assert!(!points_value_projector.contains("StackItem::from_array"));

    let points_bytes_encoder = slice_between(
        points_storage_source,
        "fn points_to_array_bytes",
        "fn points_to_stack_item",
    );
    assert!(points_bytes_encoder.contains("points_to_stack_value"));
    assert!(points_bytes_encoder.contains("serialize_stack_value_default"));
    assert!(!points_bytes_encoder.contains("StackItem::from_array"));
    assert!(!points_bytes_encoder.contains("BinarySerializer::serialize("));

    let points_item_adapter =
        slice_between(points_storage_source, "fn points_to_stack_item", "\n}");
    assert!(points_item_adapter.contains("points_to_stack_value"));
    assert!(points_item_adapter.contains("StackItem::try_from"));
    assert!(!points_item_adapter.contains("StackItem::from_array"));

    let on_persist = slice_between(
        persist_source,
        "fn on_persist_native",
        "fn post_persist_native",
    );
    assert!(on_persist.contains("points_to_stack_item"));
    assert!(!on_persist.contains("StackItem::from_array"));
}

#[test]
fn zero_bigint_storage_writes_match_csharp_empty_bytes() {
    // C# StorageItem stores BigInteger.ToByteArrayStandard(): EMPTY bytes for
    // zero (num-bigint's to_signed_bytes_le would give [0x00] — a raw stored-
    // bytes / state-root divergence). _votersCount can legitimately reach 0
    // when the last voter un-votes; gasPerBlock can be set to 0.
    let cache = DataCache::new(false);
    NeoToken::new().write_voters_count(&cache, &BigInt::from(0));
    let stored = cache
        .get(&NeoToken::voters_count_key())
        .expect("entry written");
    assert!(
        stored.value_bytes().is_empty(),
        "zero votersCount stores empty bytes"
    );
    assert_eq!(NeoToken::new().read_voters_count(&cache), BigInt::from(0));

    NeoToken::new().put_gas_per_block(&cache, 7, &BigInt::from(0));
    let key = NeoToken::gas_per_block_key(7);
    let stored = cache.get(&key).expect("entry written");
    assert!(
        stored.value_bytes().is_empty(),
        "zero gasPerBlock stores empty bytes"
    );

    // Non-zero values keep the signed-LE form.
    NeoToken::new().write_voters_count(&cache, &BigInt::from(300));
    let stored = cache
        .get(&NeoToken::voters_count_key())
        .expect("entry written");
    assert_eq!(
        stored.value_bytes().as_ref(),
        BigInt::from(300).to_signed_bytes_le()
    );
}

#[test]
fn calculate_bonus_matches_csharp_testcalculatebonus() {
    // C# UT_NeoToken.TestCalculateBonus "Normal 1": balance 100, no vote,
    // BalanceHeight 0, the genesis 5-GAS gasPerBlock record at index 0, end
    // 100 -> 100 * (5e8 * 100) * 10 / 100 / 100_000_000 = 5000.
    let cache = DataCache::new(false);
    NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
    let holder = NeoAccountStateView {
        balance: BigInt::from(100),
        balance_height: 0,
        vote_to: None,
        last_gas_per_vote: BigInt::from(0),
    };
    assert_eq!(
        NeoToken::new()
            .calculate_bonus(&cache, &holder, 100)
            .unwrap(),
        BigInt::from(5000)
    );

    // balance == 0 -> 0; BalanceHeight >= end -> 0; balance < 0 -> fault.
    let zero = NeoAccountStateView {
        balance: BigInt::from(0),
        ..clone_view(&holder)
    };
    assert_eq!(
        NeoToken::new().calculate_bonus(&cache, &zero, 100).unwrap(),
        BigInt::from(0)
    );
    let future = NeoAccountStateView {
        balance_height: 100,
        ..clone_view(&holder)
    };
    assert_eq!(
        NeoToken::new()
            .calculate_bonus(&cache, &future, 100)
            .unwrap(),
        BigInt::from(0)
    );
    let negative = NeoAccountStateView {
        balance: BigInt::from(-100),
        ..clone_view(&holder)
    };
    assert!(
        NeoToken::new()
            .calculate_bonus(&cache, &negative, 100)
            .is_err()
    );
}

fn clone_view(v: &NeoAccountStateView) -> NeoAccountStateView {
    NeoAccountStateView {
        balance: v.balance.clone(),
        balance_height: v.balance_height,
        vote_to: v.vote_to.clone(),
        last_gas_per_vote: v.last_gas_per_vote.clone(),
    }
}
