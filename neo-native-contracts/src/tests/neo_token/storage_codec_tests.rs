use super::*;
use crate::neo_token::storage::candidate_signature_account;
use crate::test_support::{sample_committee, seed_committee};
use neo_execution::Contract;
use neo_vm::Interoperable;

fn seed_register_price(cache: &DataCache, price: i64) {
    cache.add(
        NeoToken::register_price_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(price))),
    );
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
fn candidate_state_fast_decode_accepts_canonical_shape_and_falls_back() {
    let canonical = NeoToken::encode_candidate_state(true, &BigInt::from(123)).unwrap();
    assert_eq!(
        NeoToken::decode_canonical_candidate_state(&canonical).unwrap(),
        Some((true, BigInt::from(123)))
    );
    assert_eq!(
        NeoToken::decode_candidate_state(&canonical).unwrap(),
        (true, BigInt::from(123))
    );

    let noncanonical = BinarySerializer::serialize(
        &StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(1)),
            StackItem::from_int(BigInt::from(123)),
        ]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        NeoToken::decode_canonical_candidate_state(&noncanonical).unwrap(),
        None
    );
    assert_eq!(
        NeoToken::decode_candidate_state(&noncanonical).unwrap(),
        (true, BigInt::from(123)),
        "generic decoder should preserve boolean-compatible historical records"
    );
}

#[test]
fn candidate_signature_account_cache_is_reusable() {
    let points = sample_committee();
    let first = candidate_signature_account(&points[0]);
    let second = candidate_signature_account(&points[0]);

    assert_eq!(first, second);
    assert_eq!(
        first,
        UInt160::from_script(&Contract::create_signature_redeem_script(points[0].clone()))
    );
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
        NeoToken::committee_key(),
        StorageItem::from_bytes(encoded_committee),
    );
    assert_eq!(
        NeoToken::new().read_committee_with_votes(&cache).unwrap(),
        members
    );
    assert_eq!(
        NeoToken::new().read_committee_member_at(&cache, 1).unwrap(),
        members[1]
    );

    let mut malformed_committee = NeoToken::encode_committee(&members).unwrap();
    malformed_committee.push(0xff);
    assert_eq!(
        NeoToken::decode_canonical_committee_member_at(&malformed_committee, 1).unwrap(),
        None,
        "indexed committee fast path must not accept trailing malformed bytes"
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

    let source = include_str!("../../neo_token/storage/account.rs");
    let committee_source = include_str!("../../neo_token/storage/committee.rs");
    let account_decoder = slice_between(
        source,
        "fn decode_neo_account_state",
        "fn encode_neo_account_state",
    );
    assert!(account_decoder.contains("decode_stack_value"));
    assert!(account_decoder.contains("NeoAccountStateView::from_stack_value"));
    assert!(!account_decoder.contains("StackValue::Struct"));
    assert!(!account_decoder.contains("stack_value_as_bigint"));
    assert!(!account_decoder.contains("BinarySerializer::deserialize("));

    let account_encoder = slice_between(
        source,
        "fn encode_neo_account_state",
        "fn read_account_state",
    );
    assert!(account_encoder.contains("encode_storage_struct"));
    assert!(!account_encoder.contains("StackValue::Struct"));
    assert!(!account_encoder.contains("StackItem::from_struct"));
    assert!(!account_encoder.contains("BinarySerializer::serialize("));

    let committee_reader = slice_between(
        committee_source,
        "fn read_committee_with_votes",
        "fn read_committee_points",
    );
    assert!(committee_reader.contains("decode_stack_value"));
    assert!(committee_reader.contains("CachedCommittee::from_stack_value"));
    assert!(!committee_reader.contains("StackValue::Array"));
    assert!(!committee_reader.contains("StackValue::Struct"));
    assert!(!committee_reader.contains("stack_value_as_bigint"));
    assert!(!committee_reader.contains("BinarySerializer::deserialize("));

    let committee_encoder = slice_between(
        committee_source,
        "fn encode_committee",
        "fn should_refresh_committee",
    );
    assert!(committee_encoder.contains("CachedCommittee::new"));
    assert!(committee_encoder.contains("encode_storage_struct"));
    assert!(!committee_encoder.contains("StackValue::Array"));
    assert!(!committee_encoder.contains("StackValue::Struct"));
    assert!(!committee_encoder.contains("StackItem::from_array"));
    assert!(!committee_encoder.contains("BinarySerializer::serialize("));

    assert!(
        committee_source.contains("fn compute_committee_address")
            && committee_source.contains("COMMITTEE_ADDRESS_CACHE"),
        "committee witness checks should reuse the byte-keyed committee address cache"
    );

    let candidate_source = include_str!("../../neo_token/storage/candidates.rs");
    let candidate_decoder = slice_between(
        candidate_source,
        "fn decode_candidate_state",
        "fn encode_candidate_state",
    );
    assert!(candidate_decoder.contains("decode_stack_value"));
    assert!(candidate_decoder.contains("CandidateState::from_stack_value"));
    assert!(!candidate_decoder.contains("StackValue::Struct"));
    assert!(!candidate_decoder.contains("stack_value_as_bigint"));
    assert!(!candidate_decoder.contains("BinarySerializer::deserialize("));

    let candidate_encoder = slice_between(
        candidate_source,
        "fn encode_candidate_state",
        "/// C# `GetCandidatesInternal`",
    );
    assert!(candidate_encoder.contains("CandidateState::new"));
    assert!(candidate_encoder.contains("encode_storage_struct"));
    assert!(!candidate_encoder.contains("StackValue::Struct"));
    assert!(!candidate_encoder.contains("StackItem::from_struct"));
    assert!(!candidate_encoder.contains("BinarySerializer::serialize("));

    assert!(
        candidate_source.contains("HashMap<ECPoint, UInt160>"),
        "signature-account cache should key by ECPoint to avoid Vec allocation on cache hits"
    );
    let signature_account_cache = slice_between(
        candidate_source,
        "fn candidate_signature_account",
        "fn committee_candidate_order",
    );
    assert!(
        !signature_account_cache.contains("pubkey.to_bytes()"),
        "cache hits should borrow the ECPoint key directly"
    );

    let top_registered_candidates = slice_between(
        candidate_source,
        "fn top_registered_candidates",
        "/// Decodes a `CandidateState` storage value",
    );
    let candidate_scan = slice_between(
        top_registered_candidates,
        "for (key, item)",
        "counts.record(top.len() as u64)",
    );
    let key_length_guard = candidate_scan
        .find("key_bytes.len() < 34")
        .expect("candidate scan keeps malformed-key length guard");
    let state_decode = candidate_scan
        .find("Self::decode_candidate_state")
        .expect("candidate scan decodes CandidateState");
    let pubkey_decode = candidate_scan
        .find("ECPoint::from_bytes")
        .expect("candidate scan decodes candidate public keys");
    let blocked_lookup = candidate_scan
        .find("candidate_is_blocked_in")
        .expect("candidate scan checks blocked accounts");
    assert!(
        key_length_guard < state_decode,
        "short malformed candidate keys should skip state decoding"
    );
    assert!(
        state_decode < pubkey_decode,
        "unregistered candidate rows should skip ECPoint decompression on the committee scan hot path"
    );
    assert!(
        state_decode < blocked_lookup,
        "candidate state should gate blocked-account lookup"
    );

    let registered_candidate_entries = slice_between(
        candidate_source,
        "fn registered_candidate_entries",
        "/// [`registered_candidate_entries`] projected",
    );
    let registered_key_length_guard = registered_candidate_entries
        .find("key_bytes.len() < 34")
        .expect("registered candidate scan keeps malformed-key length guard");
    let registered_state_decode = registered_candidate_entries
        .find("Self::decode_candidate_state")
        .expect("registered candidate scan decodes CandidateState");
    let registered_pubkey_decode = registered_candidate_entries
        .find("ECPoint::from_bytes")
        .expect("registered candidate scan decodes candidate public keys");
    let registered_blocked_lookup = registered_candidate_entries
        .find("candidate_is_blocked_in")
        .expect("registered candidate scan checks blocked accounts");
    assert!(
        registered_key_length_guard < registered_state_decode,
        "short malformed candidate keys should skip registered-candidate state decoding"
    );
    assert!(
        registered_state_decode < registered_pubkey_decode,
        "unregistered candidate rows should skip ECPoint decompression in registered candidate scans"
    );
    assert!(
        registered_state_decode < registered_blocked_lookup,
        "registered candidate state should gate blocked-account lookup"
    );
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
        NeoToken::committee_key(),
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
        NeoToken::committee_key(),
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
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        let key = NeoToken::candidate_key(pk);
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
fn committee_address_cache_tracks_changed_committee_bytes() {
    let cache = DataCache::new(false);
    let points = sample_committee();
    seed_committee(&cache, &points);
    let first = NeoToken::new().compute_committee_address(&cache).unwrap();

    let replacement = &points[..2];
    let replacement_item = StackItem::from_array(
        replacement
            .iter()
            .map(|point| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(point.to_bytes()),
                    StackItem::from_int(BigInt::from(0)),
                ])
            })
            .collect::<Vec<_>>(),
    );
    let replacement_bytes =
        BinarySerializer::serialize(&replacement_item, &ExecutionEngineLimits::default()).unwrap();
    cache.update(
        NeoToken::committee_key(),
        StorageItem::from_bytes(replacement_bytes),
    );

    let second = NeoToken::new().compute_committee_address(&cache).unwrap();
    let replacement_script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            NeoToken::committee_threshold(replacement.len()),
            replacement,
        )
        .unwrap();

    assert_ne!(first, second, "changed committee bytes must miss the cache");
    assert_eq!(second, UInt160::from_script(&replacement_script));
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
        NeoToken::new().balance_of(&cache, &account).unwrap(),
        BigInt::from(0)
    );
}

#[test]
fn total_supply_returns_constant_not_storage_slot() {
    use std::sync::Arc;

    let cache = DataCache::new(false);
    cache.add(
        NeoToken::total_supply_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(42))),
    );
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        neo_primitives::TriggerType::Application,
        None,
        Arc::new(cache),
        None,
        ProtocolSettings::default(),
        10_000_000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
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
    let key = NeoToken::gas_per_block_key(10);
    cache.add(
        key,
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            3 * 100_000_000i64,
        ))),
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
    let key = NeoToken::account_key(&account);
    cache.add(key, StorageItem::from_bytes(bytes.clone()));
    assert_eq!(
        NeoToken::new().read_account_state(&cache, &account),
        Some(bytes.clone())
    );
    // The returned bytes deserialize to the 4-field struct.
    match BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap() {
        StackItem::Struct(s) => assert_eq!(s.items().len(), 4),
        other => panic!("expected Struct, got {other:?}"),
    }
}
