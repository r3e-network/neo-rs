//! Tests for the NeoToken native contract.
//!
//! Extracted from `neo_token.rs` to keep the production module focused.
//! The `use super::*;` below re-exports the production items
//! (`NeoToken`, `NeoAccountStateView`, `CandidateState`, etc.) so the
//! inner test modules' own `use super::*;` resolves to them.

use super::*;

#[cfg(test)]
mod tests {
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
            let versions: Vec<&NativeMethod> =
                c.methods().iter().filter(|m| m.name == name).collect();
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

    use crate::test_support::{hex, sample_committee, seed_committee};

    /// Stores a committee cache (Array of `Struct[pubkey, votes]`) under
    /// `Prefix_Committee`, mirroring C# `CachedCommittee.ToStackItem`.
    fn seed_committee_local(cache: &DataCache, points: &[ECPoint]) {
        use neo_storage::StorageItem;
        let array = StackItem::from_array(
            points
                .iter()
                .map(|p| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(p.to_bytes()),
                        StackItem::from_int(0),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default()).unwrap();
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(bytes),
        );
    }

    fn seed_register_price(cache: &DataCache, price: i64) {
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_REGISTER_PRICE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(price))),
        );
    }

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
            let bytes =
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
            let key = StorageKey::create_with_bytes(NeoToken::ID, PREFIX_CANDIDATE, &pk.to_bytes());
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
            BinarySerializer::serialize(&legacy_candidates, &ExecutionEngineLimits::default())
                .unwrap();
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

        let source = include_str!("mod.rs");
        let candidate_encoder = slice_between(
            source,
            "fn candidates_to_array_bytes",
            "fn points_to_array_bytes",
        );
        assert!(candidate_encoder.contains("StackValue::Array"));
        assert!(candidate_encoder.contains("StackValue::Struct"));
        assert!(candidate_encoder.contains("serialize_stack_value_default"));
        assert!(!candidate_encoder.contains("StackItem::from_array"));
        assert!(!candidate_encoder.contains("BinarySerializer::serialize("));

        let points_value_projector = slice_between(
            source,
            "fn points_to_stack_value",
            "fn points_to_array_bytes",
        );
        assert!(points_value_projector.contains("StackValue::Array"));
        assert!(!points_value_projector.contains("StackItem::from_array"));

        let points_bytes_encoder = slice_between(
            source,
            "fn points_to_array_bytes",
            "fn points_to_stack_item",
        );
        assert!(points_bytes_encoder.contains("points_to_stack_value"));
        assert!(points_bytes_encoder.contains("serialize_stack_value_default"));
        assert!(!points_bytes_encoder.contains("StackItem::from_array"));
        assert!(!points_bytes_encoder.contains("BinarySerializer::serialize("));

        let points_item_adapter =
            slice_between(source, "fn points_to_stack_item", "fn committee_threshold");
        assert!(points_item_adapter.contains("points_to_stack_value"));
        assert!(points_item_adapter.contains("StackItem::try_from"));
        assert!(!points_item_adapter.contains("StackItem::from_array"));

        let on_persist = slice_between(source, "fn on_persist", "fn post_persist");
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
        let key = StorageKey::create_with_uint32(NeoToken::ID, PREFIX_GAS_PER_BLOCK, 7);
        let stored = cache
            .get(&key)
            .expect("entry written");
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

    #[test]
    fn neo_account_state_decodes_struct_fields() {
        // Struct[Balance, BalanceHeight, VoteTo(null), LastGasPerVote].
        let item = StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(100)),
            StackItem::from_int(BigInt::from(42)),
            StackItem::null(),
            StackItem::from_int(BigInt::from(7)),
        ]);
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
        let state = NeoToken::decode_neo_account_state(&bytes).unwrap();
        assert_eq!(state.balance, BigInt::from(100));
        assert_eq!(state.balance_height, 42);
        assert!(state.vote_to.is_none());
        assert_eq!(state.last_gas_per_vote, BigInt::from(7));
    }

    #[test]
    fn neo_account_state_interoperable_projection_matches_csharp_shape() {
        let points = sample_committee();
        let state = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 42,
            vote_to: Some(points[0].clone()),
            last_gas_per_vote: BigInt::from(-7),
        };
        let expected = StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(100)),
            StackItem::from_int(BigInt::from(42)),
            StackItem::from_byte_string(points[0].to_bytes()),
            StackItem::from_int(BigInt::from(-7)),
        ]);

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        let expected_bytes =
            BinarySerializer::serialize(&expected, &ExecutionEngineLimits::default()).unwrap();
        let trait_bytes = BinarySerializer::serialize_stack_value_default(&trait_value).unwrap();
        assert_eq!(trait_bytes, expected_bytes);

        let mut parsed = NeoAccountStateView {
            balance: BigInt::from(0),
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::from(0),
        };
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed.balance, state.balance);
        assert_eq!(parsed.balance_height, state.balance_height);
        assert_eq!(parsed.vote_to, state.vote_to);
        assert_eq!(parsed.last_gas_per_vote, state.last_gas_per_vote);
    }

    #[test]
    fn candidate_state_interoperable_projection_matches_csharp_shape() {
        let state = CandidateState::new(true, BigInt::from(123));
        let expected = StackItem::from_struct(vec![
            StackItem::from_bool(true),
            StackItem::from_int(BigInt::from(123)),
        ]);

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        let expected_bytes =
            BinarySerializer::serialize(&expected, &ExecutionEngineLimits::default()).unwrap();
        let trait_bytes = BinarySerializer::serialize_stack_value_default(&trait_value).unwrap();
        assert_eq!(trait_bytes, expected_bytes);

        let mut parsed = CandidateState::new(false, BigInt::from(0));
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn cached_committee_interoperable_projection_matches_csharp_shape() {
        let points = sample_committee();
        let members = vec![
            (points[0].clone(), BigInt::from(100)),
            (points[1].clone(), BigInt::from(-1)),
        ];
        let state = CachedCommittee::new(members.clone());
        let expected = StackItem::from_array(vec![
            StackItem::from_struct(vec![
                StackItem::from_byte_string(points[0].to_bytes()),
                StackItem::from_int(BigInt::from(100)),
            ]),
            StackItem::from_struct(vec![
                StackItem::from_byte_string(points[1].to_bytes()),
                StackItem::from_int(BigInt::from(-1)),
            ]),
        ]);

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        let expected_bytes =
            BinarySerializer::serialize(&expected, &ExecutionEngineLimits::default()).unwrap();
        let trait_bytes = BinarySerializer::serialize_stack_value_default(&trait_value).unwrap();
        assert_eq!(trait_bytes, expected_bytes);

        let mut parsed = CachedCommittee::new(Vec::new());
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed.into_members(), members);
    }

    #[test]
    fn neo_storage_codecs_match_csharp_stackitem_bytes() {
        let points = sample_committee();
        let large_votes = BigInt::parse_bytes(b"9223372036854775808", 10).unwrap();
        let account = NeoAccountStateView {
            balance: large_votes.clone(),
            balance_height: 42,
            vote_to: Some(points[0].clone()),
            last_gas_per_vote: BigInt::from(-7),
        };

        let expected_account = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_int(account.balance.clone()),
                StackItem::from_int(BigInt::from(account.balance_height)),
                StackItem::from_byte_string(points[0].to_bytes()),
                StackItem::from_int(account.last_gas_per_vote.clone()),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let encoded_account = NeoToken::encode_neo_account_state(&account).unwrap();
        assert_eq!(encoded_account, expected_account);
        let decoded_account = NeoToken::decode_neo_account_state(&encoded_account).unwrap();
        assert_eq!(decoded_account.balance, account.balance);
        assert_eq!(decoded_account.balance_height, account.balance_height);
        assert_eq!(decoded_account.vote_to, account.vote_to);
        assert_eq!(decoded_account.last_gas_per_vote, account.last_gas_per_vote);

        let expected_candidate = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_bool(true),
                StackItem::from_int(large_votes.clone()),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let encoded_candidate = NeoToken::encode_candidate_state(true, &large_votes).unwrap();
        assert_eq!(encoded_candidate, expected_candidate);
        assert_eq!(
            NeoToken::decode_candidate_state(&encoded_candidate).unwrap(),
            (true, large_votes.clone())
        );

        let members = vec![
            (points[0].clone(), large_votes.clone()),
            (points[1].clone(), BigInt::from(-1)),
        ];
        let expected_committee = BinarySerializer::serialize(
            &StackItem::from_array(
                members
                    .iter()
                    .map(|(point, votes)| {
                        StackItem::from_struct(vec![
                            StackItem::from_byte_string(point.to_bytes()),
                            StackItem::from_int(votes.clone()),
                        ])
                    })
                    .collect::<Vec<_>>(),
            ),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let encoded_committee = NeoToken::encode_committee(&members).unwrap();
        assert_eq!(encoded_committee, expected_committee);

        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(encoded_committee),
        );
        assert_eq!(
            NeoToken::new().read_committee_with_votes(&cache).unwrap(),
            members
        );
    }

    #[test]
    fn neo_storage_codecs_use_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("mod.rs");
        let account_decoder = slice_between(
            source,
            "fn decode_neo_account_state",
            "fn encode_neo_account_state",
        );
        assert!(account_decoder.contains("deserialize_stack_value_with_limits"));
        assert!(account_decoder.contains("NeoAccountStateView::from_stack_value"));
        assert!(!account_decoder.contains("StackValue::Struct"));
        assert!(!account_decoder.contains("stack_value_as_bigint"));
        assert!(!account_decoder.contains("BinarySerializer::deserialize("));

        let account_encoder =
            slice_between(source, "fn encode_neo_account_state", "fn voters_count_key");
        assert!(account_encoder.contains("state.to_stack_value"));
        assert!(account_encoder.contains("serialize_stack_value_default"));
        assert!(!account_encoder.contains("StackValue::Struct"));
        assert!(!account_encoder.contains("StackItem::from_struct"));
        assert!(!account_encoder.contains("BinarySerializer::serialize("));

        let committee_reader = slice_between(
            source,
            "fn read_committee_with_votes",
            "fn read_committee_points",
        );
        assert!(committee_reader.contains("deserialize_stack_value_with_limits"));
        assert!(committee_reader.contains("CachedCommittee::from_stack_value"));
        assert!(!committee_reader.contains("StackValue::Array"));
        assert!(!committee_reader.contains("StackValue::Struct"));
        assert!(!committee_reader.contains("stack_value_as_bigint"));
        assert!(!committee_reader.contains("BinarySerializer::deserialize("));

        let committee_encoder =
            slice_between(source, "fn encode_committee", "fn should_refresh_committee");
        assert!(committee_encoder.contains("CachedCommittee::new"));
        assert!(committee_encoder.contains("to_stack_value"));
        assert!(committee_encoder.contains("serialize_stack_value_default"));
        assert!(!committee_encoder.contains("StackValue::Array"));
        assert!(!committee_encoder.contains("StackValue::Struct"));
        assert!(!committee_encoder.contains("StackItem::from_array"));
        assert!(!committee_encoder.contains("BinarySerializer::serialize("));

        let candidate_decoder = slice_between(
            source,
            "fn decode_candidate_state",
            "fn encode_candidate_state",
        );
        assert!(candidate_decoder.contains("deserialize_stack_value_with_limits"));
        assert!(candidate_decoder.contains("CandidateState::from_stack_value"));
        assert!(!candidate_decoder.contains("StackValue::Struct"));
        assert!(!candidate_decoder.contains("stack_value_as_bigint"));
        assert!(!candidate_decoder.contains("BinarySerializer::deserialize("));

        let candidate_encoder =
            slice_between(source, "fn encode_candidate_state", "fn candidate_key");
        assert!(candidate_encoder.contains("CandidateState::new"));
        assert!(candidate_encoder.contains("to_stack_value"));
        assert!(candidate_encoder.contains("serialize_stack_value_default"));
        assert!(!candidate_encoder.contains("StackValue::Struct"));
        assert!(!candidate_encoder.contains("StackItem::from_struct"));
        assert!(!candidate_encoder.contains("BinarySerializer::serialize("));
    }

    #[test]
    fn neo_storage_codecs_reject_truncated_records_like_csharp() {
        let short_account = BinarySerializer::serialize(
            &StackItem::from_struct(vec![StackItem::from_int(1)]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let account_err = match NeoToken::decode_neo_account_state(&short_account) {
            Ok(_) => panic!("NeoAccountState indexes four fields"),
            Err(err) => err,
        };
        assert!(
            account_err.to_string().contains("at least 4 fields"),
            "{account_err}"
        );

        let extra_account = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_int(1),
                StackItem::from_int(2),
                StackItem::null(),
                StackItem::from_int(3),
                StackItem::from_int(4),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let account = NeoToken::decode_neo_account_state(&extra_account)
            .expect("C# NeoAccountState ignores extra fields");
        assert_eq!(account.balance, BigInt::from(1));
        assert_eq!(account.balance_height, 2);
        assert_eq!(account.vote_to, None);
        assert_eq!(account.last_gas_per_vote, BigInt::from(3));

        let short_candidate = BinarySerializer::serialize(
            &StackItem::from_struct(vec![StackItem::from_bool(true)]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let candidate_err = NeoToken::decode_candidate_state(&short_candidate)
            .expect_err("CandidateState indexes two fields");
        assert!(
            candidate_err.to_string().contains("at least 2 fields"),
            "{candidate_err}"
        );

        let extra_candidate = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_bool(false),
                StackItem::from_int(9),
                StackItem::from_int(10),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        assert_eq!(
            NeoToken::decode_candidate_state(&extra_candidate).unwrap(),
            (false, BigInt::from(9))
        );

        let points = sample_committee();
        let short_committee = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_struct(vec![
                StackItem::from_byte_string(points[0].to_bytes()),
            ])]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(short_committee),
        );
        let committee_err = NeoToken::new()
            .read_committee_with_votes(&cache)
            .expect_err("CachedCommittee indexes two fields per element");
        assert!(
            committee_err.to_string().contains("at least 2 fields"),
            "{committee_err}"
        );

        let extra_committee = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_struct(vec![
                StackItem::from_byte_string(points[0].to_bytes()),
                StackItem::from_int(11),
                StackItem::from_int(12),
            ])]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(extra_committee),
        );
        assert_eq!(
            NeoToken::new().read_committee_with_votes(&cache).unwrap(),
            vec![(points[0].clone(), BigInt::from(11))]
        );
    }

    #[test]
    fn candidate_vote_is_votes_or_minus_one() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let points = sample_committee();

        // No entry at all -> -1.
        assert_eq!(
            NeoToken::new().candidate_vote(&cache, &points[0]).unwrap(),
            BigInt::from(-1)
        );

        let store = |pk: &ECPoint, registered: bool, votes: i64| {
            let state = StackItem::from_struct(vec![
                StackItem::from_bool(registered),
                StackItem::from_int(votes),
            ]);
            let bytes =
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
            let key = StorageKey::create_with_bytes(NeoToken::ID, PREFIX_CANDIDATE, &pk.to_bytes());
            cache.add(key, StorageItem::from_bytes(bytes));
        };

        // Registered -> its votes; unregistered -> -1 even with a stored entry.
        store(&points[0], true, 250);
        store(&points[1], false, 999);
        assert_eq!(
            NeoToken::new().candidate_vote(&cache, &points[0]).unwrap(),
            BigInt::from(250)
        );
        assert_eq!(
            NeoToken::new().candidate_vote(&cache, &points[1]).unwrap(),
            BigInt::from(-1)
        );
    }

    #[test]
    fn committee_address_matches_multisig_script_hash() {
        let cache = DataCache::new(false);
        let points = sample_committee();
        seed_committee(&cache, &points);

        // For n=3, m=2; the address is the 2-of-3 multisig script hash. The
        // builder sorts the keys the same way C# CreateMultiSigRedeemScript does.
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                2, &points,
            )
                .unwrap();
        assert_eq!(
            NeoToken::new().compute_committee_address(&cache).unwrap(),
            UInt160::from_script(&script)
        );
    }

    #[test]
    fn committee_address_uninitialized_errors() {
        // C# indexes snapshot[Prefix_Committee] and throws when absent.
        let cache = DataCache::new(false);
        assert!(NeoToken::new().compute_committee_address(&cache).is_err());
        assert!(NeoToken::new().read_committee_points(&cache).is_err());
    }

    #[test]
    fn committee_address_trait_override_feeds_the_engine_seam() {
        // The `NativeContract::committee_address` override is what the engine's
        // check_committee_witness reaches through the provider seam; it must
        // return the computed address (Some), and fault on a missing committee.
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let neo = NeoToken::new();
        assert_eq!(
            NativeContract::committee_address(&neo, &cache).unwrap(),
            Some(NeoToken::new().compute_committee_address(&cache).unwrap())
        );
        assert!(NativeContract::committee_address(&neo, &DataCache::new(false)).is_err());
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[2u8; 20]).unwrap();
        assert_eq!(
            crate::read_nep17_balance(&cache, NeoToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }

    #[test]
    fn total_supply_returns_constant_not_storage_slot() {
        use std::sync::Arc;

        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(NeoToken::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(42))),
        );
        let mut engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            Arc::new(cache),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");

        let result = NativeContract::invoke(&NeoToken::new(), &mut engine, "totalSupply", &[])
            .expect("totalSupply succeeds");

        assert_eq!(
            BigInt::from_signed_bytes_le(&result),
            BigInt::from(NEO_TOTAL_AMOUNT)
        );
    }

    #[test]
    fn register_price_requires_initialized_storage() {
        let cache = DataCache::new(false);

        let err = NeoToken::new()
            .register_price(&cache)
            .expect_err("missing RegisterPrice storage must fault");
        assert!(err.to_string().contains("RegisterPrice"));

        seed_register_price(&cache, 500 * 100_000_000);
        assert_eq!(
            NeoToken::new().register_price(&cache).unwrap(),
            500 * 100_000_000
        );
    }

    #[test]
    fn gas_per_block_reads_default_and_storage() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);

        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 100),
            BigInt::from(DEFAULT_GAS_PER_BLOCK)
        );

        // gas-per-block backward seek: record at index 10 applies from 10 on.
        let key = StorageKey::create_with_uint32(NeoToken::ID, PREFIX_GAS_PER_BLOCK, 10);
        cache.add(
            key,
            StorageItem::from_bytes(BigInt::from(3 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 9),
            BigInt::from(DEFAULT_GAS_PER_BLOCK)
        );
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 20),
            BigInt::from(3 * 100_000_000i64)
        );
    }

    #[test]
    fn set_register_price_write_round_trips() {
        // The setRegisterPrice storage effect (overwrite Prefix_RegisterPrice) is
        // observed by the getRegisterPrice reader, matching C#
        // GetAndChange(_registerPrice).Set(price).
        let cache = DataCache::new(false);
        seed_register_price(&cache, DEFAULT_REGISTER_PRICE);
        NeoToken::new()
            .put_register_price(&cache, 500 * 100_000_000)
            .unwrap();
        assert_eq!(
            NeoToken::new().register_price(&cache).unwrap(),
            500 * 100_000_000
        );
        // Overwrite (GetAndChange semantics), not insert-once.
        NeoToken::new()
            .put_register_price(&cache, 2000 * 100_000_000)
            .unwrap();
        assert_eq!(
            NeoToken::new().register_price(&cache).unwrap(),
            2000 * 100_000_000
        );
    }

    #[test]
    fn set_gas_per_block_write_round_trips() {
        // The setGasPerBlock storage effect (a Prefix_GasPerBlock record at a
        // big-endian uint index) is observed by gas_per_block_at's backward seek:
        // a record at index N applies from N onward, never before.
        let cache = DataCache::new(false);
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 50),
            BigInt::from(DEFAULT_GAS_PER_BLOCK)
        );

        NeoToken::new().put_gas_per_block(&cache, 10, &BigInt::from(7 * 100_000_000i64));
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 9),
            BigInt::from(DEFAULT_GAS_PER_BLOCK)
        );
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 10),
            BigInt::from(7 * 100_000_000i64)
        );
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 100),
            BigInt::from(7 * 100_000_000i64)
        );

        // Overwrite at the same index (GetAndChange semantics).
        NeoToken::new().put_gas_per_block(&cache, 10, &BigInt::from(2 * 100_000_000i64));
        assert_eq!(
            NeoToken::new().gas_per_block_at(&cache, 10),
            BigInt::from(2 * 100_000_000i64)
        );
    }

    #[test]
    fn account_state_returns_stored_struct_or_none() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[5u8; 20]).unwrap();

        // Absent -> None (invoke maps it to an empty payload = null).
        assert!(
            NeoToken::new()
                .read_account_state(&cache, &account)
                .is_none()
        );

        // Store a NeoAccountState struct [balance, height, voteTo(Null),
        // lastGasPerVote] and read its raw bytes back unchanged.
        let state = StackItem::from_struct(vec![
            StackItem::from_int(123),
            StackItem::from_int(7),
            StackItem::null(),
            StackItem::from_int(0),
        ]);
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        let key = StorageKey::create_with_uint160(NeoToken::ID, crate::NEP17_PREFIX_ACCOUNT, &account);
        cache.add(key, StorageItem::from_bytes(bytes.clone()));
        assert_eq!(
            NeoToken::new().read_account_state(&cache, &account),
            Some(bytes.clone())
        );
        // The returned bytes deserialize to the 4-field struct.
        match BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
            .unwrap()
        {
            StackItem::Struct(s) => assert_eq!(s.items().len(), 4),
            other => panic!("expected Struct, got {other:?}"),
        }
    }
}

/// Reusable harness proving a witness-gated native call can be exercised
/// end-to-end in a unit test — the prerequisite for verifying NeoToken's
/// governance writers (`registerCandidate` / `vote` / …), which all gate on
/// `engine.check_witness_hash`. A direct `invoke(...)` call has no execution
/// context, so the witness check only works through the VM: load a script that
/// reaches `System.Runtime.CheckWitness` into an `ApplicationEngine` whose
/// script container is a transaction carrying the relevant signer.
#[cfg(test)]
mod witness_harness_tests {
    use neo_config::ProtocolSettings;
    use neo_execution::ApplicationEngine;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_primitives::{CallFlags, TriggerType, UInt160, Verifiable, WitnessScope};
    use neo_storage::DataCache;
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// Builds a script that calls `System.Runtime.CheckWitness(hash)`.
    fn check_witness_script(hash: &UInt160) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&hash.to_array());
        builder
            .emit_syscall("System.Runtime.CheckWitness")
            .expect("CheckWitness syscall");
        builder.to_array()
    }

    /// Runs `script` through a fresh Application-trigger engine whose container
    /// is a transaction signed (Global scope) by each hash in `signers`.
    /// Returns the final VM state and the boolean on top of the result stack.
    fn run_signed(script: Vec<u8>, signers: &[UInt160]) -> (VmState, bool) {
        let mut tx = Transaction::new();
        tx.set_signers(
            signers
                .iter()
                .map(|h| Signer::new(*h, WitnessScope::GLOBAL))
                .collect(),
        );
        tx.set_witnesses(signers.iter().map(|_| Witness::empty()).collect());
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(script, CallFlags::READ_ONLY, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let top = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok())
            .unwrap_or(false);
        (state, top)
    }

    #[test]
    fn checkwitness_true_for_signer_false_for_others() {
        let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let stranger = UInt160::from_bytes(&[0x22; 20]).unwrap();

        // The signed hash → CheckWitness true.
        let (state, ok) = run_signed(check_witness_script(&signer), &[signer]);
        assert_eq!(state, VmState::HALT, "script must HALT");
        assert!(ok, "CheckWitness must be true for a Global-scope signer");

        // A different hash → CheckWitness false (still a clean HALT).
        let (state2, ok2) = run_signed(check_witness_script(&stranger), &[signer]);
        assert_eq!(state2, VmState::HALT, "script must HALT");
        assert!(!ok2, "CheckWitness must be false for a non-signer");
    }
}

/// End-to-end verification of the candidate-registration writers through the VM
/// (the witness-gated script-execution path proven by `witness_harness_tests`):
/// a script `System.Contract.Call`s NeoToken with the candidate as signer, and
/// the resulting candidate state is asserted against the shared snapshot.
#[cfg(test)]
mod governance_writer_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_execution::{ApplicationEngine, Contract};
    use neo_io::{BinaryWriter, Serializable};
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_primitives::{CallFlags, TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    use crate::test_support::deploy_native;

    fn candidate_pubkey() -> ECPoint {
        // A valid secp256r1 public key (a Neo N3 standby validator).
        ECPoint::from_bytes(
            &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .unwrap(),
        )
        .unwrap()
    }

    /// Runs `method(pubkey)` on NeoToken via System.Contract.Call, signed (Global)
    /// by `signer`, against the shared `snapshot`. Returns the final VM state.
    fn call(snapshot: Arc<DataCache>, signer: UInt160, pubkey: &[u8], method: &str) -> VmState {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        builder.emit_push(pubkey);
        builder.emit_push_int(1);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(method.as_bytes());
        builder.emit_push(&NeoToken::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            ProtocolSettings::default(),
            2000_00000000, // > the 1000-GAS register price
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        engine.execute_allow_fault()
    }

    fn seeded_snapshot() -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        let neo_state = build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0);
        deploy_native(&cache, &neo_state);
        seed_register_price(&cache);
        Arc::new(cache)
    }

    fn seed_register_price(cache: &DataCache) {
        cache.add(
            NeoToken::register_price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_REGISTER_PRICE,
            ))),
        );
    }

    #[test]
    fn register_then_unregister_candidate_round_trip() {
        let pubkey = candidate_pubkey();
        let pubkey_bytes = pubkey.to_bytes();
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let snapshot = seeded_snapshot();

        // Register (signed by the candidate's account) → Registered with 0 votes.
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

        // Unregister → the zero-vote entry is removed.
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
        // `Prefix_VoterRewardPerCommittee` entry. A candidate that accrued
        // committee voter rewards then lost all votes must not leave a stale
        // reward record (a state-root divergence vs C#).
        let pubkey = candidate_pubkey();
        let pubkey_bytes = pubkey.to_bytes();
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let snapshot = seeded_snapshot();

        // Registered candidate, 0 votes, with a (stale) accrued voter-reward entry.
        snapshot.update(
            NeoToken::candidate_key(&pubkey),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(0)).unwrap(),
            ),
        );
        let reward_key = StorageKey::create_with_bytes(
            NeoToken::ID,
            PREFIX_VOTER_REWARD_PER_COMMITTEE,
            &pubkey.to_bytes(),
        );
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

        // Signed by the wrong account → no candidate is registered.
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

    #[test]
    fn vote_assigns_weight_distributes_gas_and_records_target() {
        use neo_payloads::{Block, BlockHeader};

        let candidate = candidate_pubkey();
        let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();

        crate::install();
        let cache = DataCache::new(false);
        deploy_native(
            &cache,
            &build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0),
        );
        // A registered candidate (0 votes), the voter holding 100 NEO since height
        // 0, and the genesis 5-GAS gasPerBlock record (so CalculateBonus is nonzero).
        cache.update(
            NeoToken::candidate_key(&candidate),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(0)).unwrap(),
            ),
        );
        let voter_state = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::from(0),
        };
        cache.update(
            NeoToken::neo_account_key(&voter),
            StorageItem::from_bytes(NeoToken::encode_neo_account_state(&voter_state).unwrap()),
        );
        NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        let snapshot = Arc::new(cache);

        // vote(voter, candidate), signed by the voter, in a block at index 100.
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(voter, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&candidate.to_bytes()); // voteTo (arg 1, deeper)
        builder.emit_push(&voter.to_array()); // account (arg 0, top)
        builder.emit_push_int(2);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("vote".as_bytes());
        builder.emit_push(&NeoToken::script_hash().to_array());
        builder.emit_syscall("System.Contract.Call").expect("call");

        let mut header = BlockHeader::default();
        header.set_index(100);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "vote must HALT"
        );

        // The candidate gained the voter's 100-NEO weight.
        let (_, cand_votes) = NeoToken::decode_candidate_state(
            &snapshot
                .get(&NeoToken::candidate_key(&candidate))
                .unwrap()
                .value_bytes(),
        )
        .unwrap();
        assert_eq!(cand_votes, BigInt::from(100));
        // The voter's VoteTo now points at the candidate.
        let acct = NeoToken::decode_neo_account_state(
            &NeoToken::new()
                .read_account_state(&snapshot, &voter)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(acct.vote_to, Some(candidate));
        // DistributeGas minted the 5000-datoshi CalculateBonus reward to the voter.
        let gas_key = StorageKey::create_with_uint160(
            crate::GasToken::ID,
            crate::NEP17_PREFIX_ACCOUNT,
            &voter,
        );
        let gas_item = snapshot
            .get(&gas_key)
            .expect("voter GAS account written");
        let decoded = BinarySerializer::deserialize(
            &gas_item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        let gas_balance = fields.items().first().unwrap().as_int().unwrap();
        assert_eq!(gas_balance, BigInt::from(5000));
    }

    #[test]
    fn transfer_moves_balance_and_follows_vote_weight() {
        let candidate = candidate_pubkey();
        let from = UInt160::from_bytes(&[0x0A; 20]).unwrap();
        let to = UInt160::from_bytes(&[0x0B; 20]).unwrap();

        crate::install();
        let cache = DataCache::new(false);
        deploy_native(
            &cache,
            &build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0),
        );
        // Candidate with 100 votes; `from` holds 100 NEO and votes for it.
        cache.update(
            NeoToken::candidate_key(&candidate),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(100)).unwrap(),
            ),
        );
        let from_state = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 0,
            vote_to: Some(candidate.clone()),
            last_gas_per_vote: BigInt::from(0),
        };
        cache.update(
            NeoToken::neo_account_key(&from),
            StorageItem::from_bytes(NeoToken::encode_neo_account_state(&from_state).unwrap()),
        );
        let snapshot = Arc::new(cache);

        // transfer(from, to, 30, <empty>), signed by `from`, no persisting block
        // (so DistributeGas is skipped and the test isolates the transfer/vote
        // bookkeeping).
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut b = ScriptBuilder::new();
        b.emit_push(&[]); // data (arg 3, pushed deepest)
        b.emit_push_int(30); // amount (arg 2)
        b.emit_push(&to.to_array()); // to (arg 1)
        b.emit_push(&from.to_array()); // from (arg 0, top)
        b.emit_push_int(4);
        b.emit_pack();
        b.emit_push_int(i64::from(CallFlags::ALL.bits()));
        b.emit_push("transfer".as_bytes());
        b.emit_push(&NeoToken::script_hash().to_array());
        b.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(b.to_array(), CallFlags::ALL, None)
            .expect("loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "transfer must HALT"
        );

        // Balances moved 30 NEO from `from` to `to`.
        let from_after = NeoToken::decode_neo_account_state(
            &NeoToken::new()
                .read_account_state(&snapshot, &from)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(from_after.balance, BigInt::from(70));
        let to_after = NeoToken::decode_neo_account_state(
            &NeoToken::new().read_account_state(&snapshot, &to).unwrap(),
        )
        .unwrap();
        assert_eq!(to_after.balance, BigInt::from(30));
        // The candidate's vote weight followed `from`'s reduced balance (100 -> 70).
        let (_, cand_votes) = NeoToken::decode_candidate_state(
            &snapshot
                .get(&NeoToken::candidate_key(&candidate))
                .unwrap()
                .value_bytes(),
        )
        .unwrap();
        assert_eq!(cand_votes, BigInt::from(70));
    }

    /// The GAS account storage key `(GasToken.ID, [Prefix_Account, account])`.
    fn gas_account_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(crate::GasToken::ID, crate::NEP17_PREFIX_ACCOUNT, account)
    }

    /// Seeds a GAS balance entry (`Struct[Balance]`) and a matching total supply.
    fn seed_gas(cache: &DataCache, account: &UInt160, balance: &BigInt) {
        let state = StackItem::from_struct(vec![StackItem::from_int(balance.clone())]);
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        cache.add(gas_account_key(account), StorageItem::from_bytes(bytes));
        cache.add(
            StorageKey::create(crate::GasToken::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(balance)),
        );
    }

    /// A second valid secp256r1 public key, byte-wise distinct from
    /// [`candidate_pubkey`] (a Neo N3 standby validator).
    fn other_pubkey(index: u8) -> ECPoint {
        let keys = [
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
        ];
        ECPoint::from_bytes(&hex::decode(keys[usize::from(index)]).unwrap()).unwrap()
    }

    /// C# `GetAllCandidates`: the iterator yields `Struct[pubkey, votes]` per
    /// registered candidate (RemovePrefix strips the 1-byte candidate prefix;
    /// PickField1 projects the Votes field), skipping unregistered entries and
    /// candidates whose signature-contract address PolicyContract blocks.
    #[test]
    fn get_all_candidates_iterator_filters_and_projects() {
        crate::install();
        let cache = DataCache::new(false);
        // 02df48… : registered with 7 votes -> the only iterator element.
        let kept = other_pubkey(0);
        cache.add(
            NeoToken::candidate_key(&kept),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap(),
            ),
        );
        // 03b209… : present but unregistered -> filtered out.
        cache.add(
            NeoToken::candidate_key(&candidate_pubkey()),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(false, &BigInt::from(3)).unwrap(),
            ),
        );
        // 03b8d9… : registered but its signature account is blocked -> filtered out.
        let blocked = other_pubkey(1);
        cache.add(
            NeoToken::candidate_key(&blocked),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(9)).unwrap(),
            ),
        );
        let blocked_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(blocked));
        cache.add(
            crate::PolicyContract::blocked_account_key(&blocked_account),
            StorageItem::from_bytes(Vec::new()),
        );

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(cache),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");

        let result = NativeContract::invoke(&NeoToken::new(), &mut engine, "getAllCandidates", &[])
            .expect("getAllCandidates succeeds");
        let iterator_id = u32::from_le_bytes(result.try_into().expect("4-byte iterator id"));

        assert!(engine.iterator_next(iterator_id).expect("first next"));
        let StackItem::Struct(element) = engine.iterator_value(iterator_id).expect("value") else {
            panic!("iterator element must be a Struct[pubkey, votes]");
        };
        let items = element.items();
        assert_eq!(
            items[0].as_bytes().unwrap().as_slice(),
            kept.to_bytes().as_slice()
        );
        assert_eq!(items[1].as_int().unwrap(), BigInt::from(7));
        assert!(
            !engine.iterator_next(iterator_id).expect("exhausted"),
            "single element"
        );
    }

    /// Full Echidna flow (C# NeoToken.OnNEP17Payment, NeoToken.cs:374-389):
    /// `GAS.transfer(sender -> NEO, registerPrice, data = pubkey)` registers
    /// the candidate (witnessed by its signature account) and burns the GAS
    /// from NEO's own balance, shrinking the total supply.
    #[test]
    fn on_nep17_payment_data_parser_uses_stack_value_projection() {
        let source = include_str!("mod.rs");
        let start = source
            .find("\"onNEP17Payment\" =>")
            .expect("onNEP17Payment branch exists");
        let end = source[start..]
            .find("\"unregisterCandidate\" =>")
            .map(|offset| start + offset)
            .expect("next branch exists");
        let branch = &source[start..end];

        assert!(branch.contains("deserialize_stack_value_with_limits"));
        assert!(branch.contains("to_byte_string_bytes"));
        assert!(!branch.contains("BinarySerializer::deserialize("));
    }

    #[test]
    fn on_nep17_payment_registers_candidate_and_burns_gas() {
        let pubkey = candidate_pubkey();
        let candidate_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let price = BigInt::from(DEFAULT_REGISTER_PRICE);

        crate::install();
        let cache = DataCache::new(false);
        // onNEP17Payment is Echidna-gated; an unconfigured hardfork is DISABLED
        // for method gating (C# IsHardforkEnabled), so schedule it at genesis.
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        deploy_native(
            &cache,
            &build_native_contract_state(&NeoToken, &settings, 0),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 0),
        );
        seed_register_price(&cache);
        seed_gas(&cache, &sender, &price);
        let snapshot = Arc::new(cache);

        // Signed by the GAS sender (transfer witness) AND the candidate's
        // signature account (RegisterInternal witness).
        let mut tx = Transaction::new();
        tx.set_signers(vec![
            Signer::new(sender, WitnessScope::GLOBAL),
            Signer::new(candidate_account, WitnessScope::GLOBAL),
        ]);
        tx.set_witnesses(vec![Witness::empty(), Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        // GAS.transfer(sender, NEO, price, pubkey-bytes) — args packed in reverse.
        let mut b = ScriptBuilder::new();
        b.emit_push(&pubkey.to_bytes()); // data (arg 3, pushed deepest)
        b.emit_push_int(DEFAULT_REGISTER_PRICE); // amount (arg 2)
        b.emit_push(&NeoToken::script_hash().to_array()); // to (arg 1)
        b.emit_push(&sender.to_array()); // from (arg 0, top)
        b.emit_push_int(4);
        b.emit_pack();
        b.emit_push_int(i64::from(CallFlags::ALL.bits()));
        b.emit_push("transfer".as_bytes());
        b.emit_push(&crate::GasToken::script_hash().to_array());
        b.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(b.to_array(), CallFlags::ALL, None)
            .expect("loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "transfer must HALT"
        );

        // The candidate is registered with zero votes.
        let item = snapshot
            .get(&NeoToken::candidate_key(&pubkey))
            .expect("candidate entry written");
        let (registered, votes) = NeoToken::decode_candidate_state(&item.value_bytes()).unwrap();
        assert!(registered, "candidate is Registered");
        assert_eq!(votes, BigInt::from(0));
        // The sender paid its whole balance (entry deleted by the transfer) and
        // NEO's credited balance was burned away again (entry deleted by Burn).
        assert!(
            snapshot.get(&gas_account_key(&sender)).is_none(),
            "sender spent all GAS"
        );
        assert!(
            snapshot
                .get(&gas_account_key(&NeoToken::script_hash()))
                .is_none(),
            "NEO's received GAS is burned"
        );
        // The burn shrank the total supply back to zero.
        let supply = snapshot
            .get(&StorageKey::new(
                crate::GasToken::ID,
                vec![crate::NEP17_PREFIX_TOTAL_SUPPLY],
            ))
            .expect("supply entry");
        assert_eq!(
            BigInt::from_signed_bytes_le(&supply.value_bytes()),
            BigInt::from(0)
        );
    }

    /// Direct-invocation engine with the calling script hash forced to `caller`
    /// and (optionally) `signer` witnessing the container.
    fn payment_engine(
        snapshot: Arc<DataCache>,
        caller: Option<UInt160>,
        signer: Option<UInt160>,
    ) -> ApplicationEngine {
        let container: Option<Arc<dyn Verifiable>> = signer.map(|hash| {
            let mut tx = Transaction::new();
            tx.set_signers(vec![Signer::new(hash, WitnessScope::GLOBAL)]);
            tx.set_witnesses(vec![Witness::empty()]);
            Arc::new(tx) as Arc<dyn Verifiable>
        });
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            container,
            snapshot,
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine.set_calling_script_hash(caller);
        engine
    }

    /// onNEP17Payment args `[from, amount, data]` as the dispatcher marshals
    /// them: Hash160 raw, Integer signed-LE, Any BinarySerialized.
    fn payment_args(from: &UInt160, amount: i64, data: &StackItem) -> Vec<Vec<u8>> {
        vec![
            from.to_bytes().to_vec(),
            BigInt::from(amount).to_signed_bytes_le(),
            BinarySerializer::serialize(data, &ExecutionEngineLimits::default()).unwrap(),
        ]
    }

    /// C# OnNEP17Payment faults unless the caller is the GAS contract, the
    /// amount equals the register price, `data` decodes as a secp256r1 point,
    /// and the candidate account witnesses the transaction.
    #[test]
    fn on_nep17_payment_rejects_bad_caller_amount_pubkey_and_witness() {
        crate::install();
        let pubkey = candidate_pubkey();
        let candidate_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let cache = DataCache::new(false);
        seed_register_price(&cache);
        let snapshot = Arc::new(cache);
        let neo = NeoToken::new();
        let pubkey_item = StackItem::from_byte_string(pubkey.to_bytes());

        // Caller is not GAS (here: unset) -> fault.
        let mut engine = payment_engine(Arc::clone(&snapshot), None, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("only the GAS contract"), "{err}");

        // Wrong amount -> fault.
        let gas_caller = Some(crate::GasToken::script_hash());
        let mut engine = payment_engine(Arc::clone(&snapshot), gas_caller, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE - 1, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("incorrect GAS amount"), "{err}");

        // `data` is not a public key -> fault.
        let mut engine = payment_engine(Arc::clone(&snapshot), gas_caller, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(
                &sender,
                DEFAULT_REGISTER_PRICE,
                &StackItem::from_byte_string(vec![1, 2, 3]),
            ),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bad public key"), "{err}");

        // No witness from the candidate account -> RegisterInternal returns
        // false -> C# throws "Failed to register candidate".
        let mut engine = payment_engine(Arc::clone(&snapshot), gas_caller, Some(sender));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed to register"), "{err}");
        assert!(
            snapshot.get(&NeoToken::candidate_key(&pubkey)).is_none(),
            "nothing registered"
        );
    }
}

/// Unit tests for `ComputeCommitteeMembers` (C# NeoToken.cs:622-635): the
/// turnout boundary, the standby fallback (low turnout / too few candidates,
/// zipped with registered-candidate votes), and the top-m ordering
/// (votes descending, pubkey ascending).
#[cfg(test)]
mod committee_recompute_tests {
    use super::*;
    use neo_config::ProtocolSettings;

    /// `n` distinct valid secp256r1 points (the mainnet standby committee).
    fn points(n: usize) -> Vec<ECPoint> {
        let pts = ProtocolSettings::default().standby_committee;
        assert!(pts.len() >= n, "mainnet standby committee has 21 members");
        pts.into_iter().take(n).collect()
    }

    fn settings_with_committee(committee: Vec<ECPoint>) -> ProtocolSettings {
        ProtocolSettings {
            standby_committee: committee,
            validators_count: 1,
            ..ProtocolSettings::default()
        }
    }

    fn seed_voters_count(cache: &DataCache, value: i64) {
        cache.add(
            NeoToken::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    fn seed_candidate(cache: &DataCache, pubkey: &ECPoint, votes: i64) {
        cache.add(
            NeoToken::candidate_key(pubkey),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(votes)).unwrap(),
            ),
        );
    }

    fn seed_committee_cache(cache: &DataCache, committee: &[ECPoint]) {
        let members: Vec<(ECPoint, BigInt)> = committee
            .iter()
            .cloned()
            .map(|point| (point, BigInt::from(0)))
            .collect();
        cache.add(
            StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(NeoToken::encode_committee(&members).unwrap()),
        );
    }

    #[test]
    fn should_refresh_committee_matches_csharp_modulo() {
        // C# `height % committeeMembersCount == 0`.
        assert!(NeoToken::should_refresh_committee(0, 21));
        assert!(!NeoToken::should_refresh_committee(1, 21));
        assert!(!NeoToken::should_refresh_committee(20, 21));
        assert!(NeoToken::should_refresh_committee(21, 21));
        assert!(NeoToken::should_refresh_committee(42, 21));
        // A single-member committee refreshes every block.
        assert!(NeoToken::should_refresh_committee(5, 1));
    }

    #[test]
    fn standby_fallback_below_turnout_zips_registered_votes() {
        // Turnout one NEO short of the 20% boundary: votersCount * 5 =
        // 99_999_995 < TotalAmount, so even with >= m candidates the standby
        // committee wins — each member zipped with its registered-candidate
        // votes (zero when not a candidate). C#: `voterTurnout < 0.2M`.
        let all = points(6);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby.clone());
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 19_999_999);
        seed_candidate(&cache, &standby[1], 42); // a standby member is a candidate
        seed_candidate(&cache, &all[3], 1000);
        seed_candidate(&cache, &all[4], 900);
        seed_candidate(&cache, &all[5], 800);

        let members = NeoToken::new()
            .compute_committee_members(&cache, &settings)
            .unwrap();
        assert_eq!(
            members,
            vec![
                (standby[0].clone(), BigInt::from(0)),
                (standby[1].clone(), BigInt::from(42)),
                (standby[2].clone(), BigInt::from(0)),
            ],
            "standby order is preserved; votes come from the candidate records"
        );
    }

    #[test]
    fn standby_fallback_when_fewer_candidates_than_committee() {
        // Turnout reached, but only 2 registered candidates for a 3-member
        // committee: C# `candidates.Length < settings.CommitteeMembersCount`
        // falls back to the standby committee.
        let all = points(5);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby.clone());
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        seed_candidate(&cache, &all[3], 1000);
        seed_candidate(&cache, &all[4], 900);

        let members = NeoToken::new()
            .compute_committee_members(&cache, &settings)
            .unwrap();
        let keys: Vec<ECPoint> = members.into_iter().map(|(p, _)| p).collect();
        assert_eq!(keys, standby);
    }

    #[test]
    fn top_m_at_exact_turnout_boundary_orders_votes_desc_pubkey_asc() {
        // votersCount * 5 == TotalAmount exactly: C# `voterTurnout < 0.2M` is
        // false (>= 0.2 passes), so with enough candidates the elected
        // committee is the top m by (votes DESC, pubkey ASC).
        let all = points(5);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby);
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        let (c0, c1, c2, c3) = (&all[1], &all[2], &all[3], &all[4]);
        seed_candidate(&cache, c0, 10);
        seed_candidate(&cache, c1, 7);
        seed_candidate(&cache, c2, 50);
        seed_candidate(&cache, c3, 5); // 4th candidate drops out of the top 3

        let members = NeoToken::new()
            .compute_committee_members(&cache, &settings)
            .unwrap();
        assert_eq!(
            members,
            vec![
                (c2.clone(), BigInt::from(50)),
                (c0.clone(), BigInt::from(10)),
                (c1.clone(), BigInt::from(7)),
            ]
        );
    }

    #[test]
    fn top_m_breaks_vote_ties_by_ascending_pubkey() {
        // C# `OrderByDescending(votes).ThenBy(pubkey)` — equal votes order by
        // the ECPoint comparison (X then Y), ascending.
        let all = points(4);
        let standby = vec![all[0].clone()];
        let settings = settings_with_committee(standby);
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        let (a, b) = (all[2].clone(), all[3].clone());
        seed_candidate(&cache, &a, 9);
        seed_candidate(&cache, &b, 9);

        let members = NeoToken::new()
            .compute_committee_members(&cache, &settings)
            .unwrap();
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        assert_eq!(
            members,
            vec![(lo, BigInt::from(9))],
            "m = 1 takes the lower pubkey"
        );
        drop(hi);
    }

    #[test]
    fn bft_address_uses_the_bft_multisig_threshold() {
        // C# Contract.GetBFTAddress: m = n - (n - 1) / 3 (7 validators -> 5).
        let validators = ProtocolSettings::default().standby_validators();
        assert_eq!(validators.len(), 7);
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                5,
                &validators,
            )
            .unwrap();
        assert_eq!(
            NeoToken::bft_address(&validators).unwrap(),
            UInt160::from_script(&script)
        );
    }

    #[test]
    fn next_consensus_address_recomputes_validators_only_on_refresh_height() {
        let all = points(6);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby.clone());
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &standby);
        seed_voters_count(&cache, 20_000_000);
        seed_candidate(&cache, &all[3], 100);
        seed_candidate(&cache, &all[4], 50);
        seed_candidate(&cache, &all[5], 25);

        let cached_validator = vec![standby[0].clone()];
        let elected_validator = vec![all[3].clone()];

        assert_eq!(
            NeoToken::new()
                .next_consensus_address_for_block(&cache, &settings, 1)
                .unwrap(),
            NeoToken::bft_address(&cached_validator).unwrap(),
            "off-refresh blocks use cached GetNextBlockValidators"
        );
        assert_eq!(
            NeoToken::new()
                .next_consensus_address_for_block(&cache, &settings, 3)
                .unwrap(),
            NeoToken::bft_address(&elected_validator).unwrap(),
            "refresh blocks use ComputeNextBlockValidators"
        );
    }
}

/// Engine-level tests for the block-boundary hooks: `on_persist` (committee
/// recompute + `CommitteeChanged`, C# NeoToken.cs:222-251) and `post_persist`
/// (committee GAS reward + voter-reward accrual, C# NeoToken.cs:253-284),
/// with reward values hand-computed from the C# formulas.
#[cfg(test)]
mod persist_hook_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::ApplicationEngine;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::TriggerType;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn engine_for(
        trigger: TriggerType,
        snapshot: Arc<DataCache>,
        index: u32,
        settings: ProtocolSettings,
    ) -> ApplicationEngine {
        let mut header = BlockHeader::default();
        header.set_index(index);
        ApplicationEngine::new(
            trigger,
            None,
            snapshot,
            Some(Block::from_parts(header, vec![])),
            settings,
            0,
            None,
        )
        .expect("engine builds")
    }

    fn committee_storage_key() -> StorageKey {
        StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE)
    }

    fn seed_committee_cache(cache: &DataCache, members: &[(ECPoint, BigInt)]) {
        cache.add(
            committee_storage_key(),
            StorageItem::from_bytes(NeoToken::encode_committee(members).unwrap()),
        );
    }

    fn voter_reward_key(pubkey: &ECPoint) -> StorageKey {
        StorageKey::create_with_bytes(
            NeoToken::ID,
            PREFIX_VOTER_REWARD_PER_COMMITTEE,
            &pubkey.to_bytes(),
        )
    }

    fn read_voter_reward(snapshot: &DataCache, pubkey: &ECPoint) -> Option<BigInt> {
        snapshot
            .get(&voter_reward_key(pubkey))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
    }

    fn gas_balance(snapshot: &DataCache, account: &UInt160) -> Option<BigInt> {
        let key =
            StorageKey::create_with_uint160(crate::GasToken::ID, crate::NEP17_PREFIX_ACCOUNT, account);
        let item = snapshot.get(&key)?;
        let decoded = BinarySerializer::deserialize(
            &item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        Some(fields.items().first().unwrap().as_int().unwrap())
    }

    fn signature_address(pubkey: &ECPoint) -> UInt160 {
        UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()))
    }

    #[test]
    fn on_persist_refresh_recomputes_committee_and_emits_committee_changed() {
        // Single-member committee (every block refreshes); HF_Cockatrice at 0
        // so the notification path is active. Seeded: standby K1 cached,
        // turnout exactly at the 20% boundary, candidate K2 registered with 7
        // votes -> recompute elects [K2] and emits CommitteeChanged([K1],[K2]).
        let all = ProtocolSettings::default().standby_committee;
        let (k1, k2) = (all[0].clone(), all[1].clone());
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HfCockatrice, 0u32);
        let settings = ProtocolSettings {
            standby_committee: vec![k1.clone()],
            validators_count: 1,
            hardforks,
            ..ProtocolSettings::default()
        };
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
        cache.add(
            NeoToken::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        cache.add(
            NeoToken::candidate_key(&k2),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap(),
            ),
        );
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        // The cache now holds the elected committee, CachedCommittee layout.
        let stored = snapshot
            .get(&committee_storage_key())
            .unwrap()
            .value_bytes()
            .into_owned();
        assert_eq!(
            stored,
            NeoToken::encode_committee(&[(k2.clone(), BigInt::from(7))]).unwrap()
        );

        // CommitteeChanged([prev pubkeys], [new pubkeys]).
        let notes = engine.notifications();
        assert_eq!(notes.len(), 1, "exactly one notification");
        let note = &notes[0];
        assert_eq!(note.script_hash, NeoToken::script_hash());
        assert_eq!(note.event_name, "CommitteeChanged");
        assert_eq!(note.state.len(), 2);
        let keys_of = |item: &StackItem| -> Vec<Vec<u8>> {
            let StackItem::Array(array) = item else {
                panic!("CommitteeChanged arg is not an array");
            };
            array
                .items()
                .iter()
                .map(|i| i.as_bytes().unwrap().to_vec())
                .collect()
        };
        assert_eq!(keys_of(&note.state[0]), vec![k1.to_bytes()]);
        assert_eq!(keys_of(&note.state[1]), vec![k2.to_bytes()]);
    }

    #[test]
    fn on_persist_refresh_without_cockatrice_updates_committee_silently() {
        // Same election as above, but HF_Cockatrice is unscheduled: the
        // committee cache still updates, with no notification (pre-3158
        // behavior, the C# hardfork gate).
        let all = ProtocolSettings::default().standby_committee;
        let (k1, k2) = (all[0].clone(), all[1].clone());
        let settings = ProtocolSettings {
            standby_committee: vec![k1.clone()],
            validators_count: 1,
            hardforks: HashMap::new(),
            ..ProtocolSettings::default()
        };
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
        cache.add(
            NeoToken::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        cache.add(
            NeoToken::candidate_key(&k2),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap(),
            ),
        );
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        let stored = snapshot
            .get(&committee_storage_key())
            .unwrap()
            .value_bytes()
            .into_owned();
        assert_eq!(
            stored,
            NeoToken::encode_committee(&[(k2, BigInt::from(7))]).unwrap()
        );
        assert!(
            engine.notifications().is_empty(),
            "no CommitteeChanged before Cockatrice"
        );
    }

    #[test]
    fn on_persist_skips_recompute_off_refresh_blocks() {
        // m = 3, block index 2: 2 % 3 != 0, so the committee cache must stay
        // untouched even though a recompute would elect different members.
        let all = ProtocolSettings::default().standby_committee;
        let standby = all[..3].to_vec();
        let settings = ProtocolSettings {
            standby_committee: standby.clone(),
            validators_count: 1,
            ..ProtocolSettings::default()
        };
        let seeded: Vec<(ECPoint, BigInt)> = standby
            .iter()
            .map(|p| (p.clone(), BigInt::from(0)))
            .collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &seeded);
        cache.add(
            NeoToken::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        for (i, candidate) in all[3..6].iter().enumerate() {
            cache.add(
                NeoToken::candidate_key(candidate),
                StorageItem::from_bytes(
                    NeoToken::encode_candidate_state(true, &BigInt::from(100 + i as i64)).unwrap(),
                ),
            );
        }
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 2, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        let stored = snapshot
            .get(&committee_storage_key())
            .unwrap()
            .value_bytes()
            .into_owned();
        assert_eq!(
            stored,
            NeoToken::encode_committee(&seeded).unwrap(),
            "cache untouched off refresh"
        );
        assert!(engine.notifications().is_empty());
    }

    /// Hand-computed C# PostPersistAsync values for the default settings
    /// (m = 21, n = 7) with gasPerBlock = 5 GAS:
    ///   committee reward      = 5_0000_0000 * 10 / 100        = 0.5 GAS
    ///   voterRewardOfEachCommittee
    ///     = 5e8 * 80 * 1e8 * 21 / (21 + 7) / 100              = 3e16
    ///   member 0 (validator, factor 2, 1000 votes): 2*3e16/1000 = 6e13
    ///   member 7 (non-validator, factor 1, 400 votes): 3e16/400 = 7.5e13
    #[test]
    fn post_persist_committee_and_voter_rewards_match_csharp_math() {
        let settings = ProtocolSettings::default();
        assert_eq!(settings.committee_members_count(), 21);
        assert_eq!(settings.validators_count, 7);
        let members: Vec<(ECPoint, BigInt)> = settings
            .standby_committee
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let votes = match i {
                    0 => 1000,
                    7 => 400,
                    _ => 0,
                };
                (p.clone(), BigInt::from(votes))
            })
            .collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &members);
        NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        // Pre-seed member 0's accumulator: C# `GetAndChange(key).Add(...)` is
        // read-modify-write, so the accrual must ADD to the existing value.
        cache.add(
            voter_reward_key(&members[0].0),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(5))),
        );
        let snapshot = Arc::new(cache);

        // Block 0 is a refresh block (0 % 21 == 0).
        let mut engine = engine_for(
            TriggerType::PostPersist,
            Arc::clone(&snapshot),
            0,
            settings.clone(),
        );
        NeoToken.post_persist(&mut engine).expect("post_persist");

        // committee[0 % 21] earns 0.5 GAS at its signature address.
        let member0_addr = signature_address(&members[0].0);
        assert_eq!(
            gas_balance(&snapshot, &member0_addr),
            Some(BigInt::from(50_000_000))
        );
        // The mint emitted GAS Transfer(null, member0, 0.5 GAS).
        let transfer = engine
            .notifications()
            .iter()
            .find(|n| n.event_name == "Transfer")
            .expect("committee reward Transfer");
        assert_eq!(transfer.script_hash, crate::GasToken::script_hash());
        assert!(matches!(transfer.state[0], StackItem::Null));
        assert_eq!(
            transfer.state[1].as_bytes().unwrap().to_vec(),
            member0_addr.to_bytes()
        );
        assert_eq!(
            transfer.state[2].as_int().unwrap(),
            BigInt::from(50_000_000)
        );

        // Voter-reward accruals (zoomed by VoteFactor), added to any existing value.
        assert_eq!(
            read_voter_reward(&snapshot, &members[0].0),
            Some(BigInt::from(60_000_000_000_005i64)),
            "validator voter reward: pre-seeded 5 + 2 * 3e16 / 1000"
        );
        assert_eq!(
            read_voter_reward(&snapshot, &members[7].0),
            Some(BigInt::from(75_000_000_000_000i64)),
            "non-validator voter reward: 3e16 / 400"
        );
        assert_eq!(
            read_voter_reward(&snapshot, &members[1].0),
            None,
            "zero-vote members accrue nothing"
        );
    }

    #[test]
    fn post_persist_off_refresh_blocks_only_mints_the_rotating_reward() {
        // Block 1 (1 % 21 != 0): committee[1] earns 0.5 GAS; no voter-reward
        // accrual happens even for members with votes.
        let settings = ProtocolSettings::default();
        let members: Vec<(ECPoint, BigInt)> = settings
            .standby_committee
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), BigInt::from(if i == 0 { 1000 } else { 0 })))
            .collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &members);
        NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(
            TriggerType::PostPersist,
            Arc::clone(&snapshot),
            1,
            settings.clone(),
        );
        NeoToken.post_persist(&mut engine).expect("post_persist");

        let member1_addr = signature_address(&members[1].0);
        assert_eq!(
            gas_balance(&snapshot, &member1_addr),
            Some(BigInt::from(50_000_000))
        );
        assert_eq!(
            gas_balance(&snapshot, &signature_address(&members[0].0)),
            None
        );
        assert_eq!(
            read_voter_reward(&snapshot, &members[0].0),
            None,
            "no accrual off refresh blocks"
        );
    }

    /// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEP-27 joins
    /// NEP-17 once HF_Echidna is enabled at the height — and Echidna is a
    /// manifest-refresh hardfork for NEO (C# carries it in `_usedHardforks`
    /// via the Echidna-gated method registrations).
    #[test]
    fn manifest_standards_gain_nep27_at_echidna() {
        use neo_execution::native_contract::build_native_contract_state;

        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 10);
        let before = build_native_contract_state(&NeoToken, &settings, 9);
        assert_eq!(before.manifest.supported_standards, ["NEP-17"]);
        let after = build_native_contract_state(&NeoToken, &settings, 10);
        assert_eq!(after.manifest.supported_standards, ["NEP-17", "NEP-27"]);

        assert!(NativeContract::used_hardforks(&NeoToken).contains(&Hardfork::HfEchidna));
    }
}
