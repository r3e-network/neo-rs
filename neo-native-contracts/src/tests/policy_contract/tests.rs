use super::*;
use neo_primitives::UInt256;
use neo_storage::StorageItem;
use neo_vm::Interoperable;
use num_bigint::BigInt;

#[path = "surface.rs"]
mod surface;

/// Structural equality for StackItem compound values.
fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

fn seed_current_block(cache: &DataCache, index: u32) {
    let value = crate::LedgerContract::new()
        .serialize_hash_index_state(&UInt256::default(), index)
        .expect("current block pointer");
    cache.add(
        crate::LedgerContract::current_block_storage_key(),
        StorageItem::from_bytes(value),
    );
}

fn seed_policy_setting_key(cache: &DataCache, key: StorageKey, value: i64) {
    cache.add(
        key,
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
    );
}

fn read_policy_setting_key(cache: &DataCache, key: StorageKey) -> i64 {
    let item = cache.get(&key).expect("policy setting");
    BigInt::from_signed_bytes_le(&item.value_bytes())
        .to_i64()
        .expect("policy setting integer")
}

#[test]
fn raw_argument_parsing_uses_shared_helpers() {
    let source = include_str!("../../policy_contract/invoke.rs");

    assert!(!source.contains("fn setter_int_arg("));
    assert!(!source.contains("fn hash160_arg("));
    assert!(!source.contains("fn attribute_type_arg("));
    for helper in [
        "crate::args::raw_i64_arg",
        "crate::args::raw_u32_arg",
        "crate::args::raw_i32_arg",
        "crate::args::raw_u8_arg",
        "crate::args::raw_string_arg",
        "crate::args::raw_hash160",
    ] {
        assert!(
            source.contains(helper),
            "PolicyContract should parse raw arguments through {helper}"
        );
    }
    assert!(!source.contains("String::from_utf8("));
}

#[test]
fn blocked_account_entries_scopes_to_prefix_blocked_account() {
    let cache = DataCache::new(false);
    // Two blocked accounts plus an unrelated fee entry that must not appear.
    let a1 = UInt160::from_bytes(&[0x11; 20]).unwrap();
    let a2 = UInt160::from_bytes(&[0x22; 20]).unwrap();
    cache.add(
        PolicyContract::blocked_account_key(&a1),
        StorageItem::from_bytes(Vec::new()),
    );
    cache.add(
        PolicyContract::blocked_account_key(&a2),
        StorageItem::from_bytes(Vec::new()),
    );
    // Prefix_FeePerByte must be excluded from the blocked-account scan.
    seed_policy_setting_key(&cache, PolicyContract::fee_per_byte_key(), 1234);

    let entries = PolicyContract::new().blocked_account_entries(&cache);
    assert_eq!(entries.len(), 2);
    // Each key's suffix is [Prefix_BlockedAccount, account]; the iterator
    // strips the 1-byte prefix to yield the account hash.
    for (key, _) in &entries {
        assert_eq!(key.suffix()[0], PREFIX_BLOCKED_ACCOUNT);
        assert_eq!(key.suffix().len(), 1 + 20);
    }
}

#[test]
fn attribute_fee_validates_type_and_round_trips() {
    let cache = DataCache::new(false);
    // HighPriority (0x01) is a defined type: defaults to 0, then round-trips.
    let hp = TransactionAttributeType::HighPriority.to_byte();
    assert_eq!(
        PolicyContract::new()
            .attribute_fee(&cache, hp, false)
            .unwrap(),
        DEFAULT_ATTRIBUTE_FEE
    );
    PolicyContract::new().put_attribute_fee(&cache, hp, 5_000);
    assert_eq!(
        PolicyContract::new()
            .attribute_fee(&cache, hp, false)
            .unwrap(),
        5_000
    );

    // An undefined attribute byte is rejected regardless of the notary flag.
    assert!(
        PolicyContract::new()
            .attribute_fee(&cache, 0xFE, true)
            .is_err()
    );

    // NotaryAssisted (0x22) is gated: rejected pre-Echidna (allow=false),
    // accepted from Echidna (allow=true).
    let na = TransactionAttributeType::NotaryAssisted.to_byte();
    assert!(
        PolicyContract::new()
            .attribute_fee(&cache, na, false)
            .is_err()
    );
    assert_eq!(
        PolicyContract::new()
            .attribute_fee(&cache, na, true)
            .unwrap(),
        DEFAULT_ATTRIBUTE_FEE
    );
}

#[test]
fn exec_fee_factor_reads_default_and_round_trips_through_storage() {
    // Pre-Faun the reader returns the raw stored value; the writer's effect
    // is observed by the reader.
    let cache = DataCache::new(false);
    let err = PolicyContract::new()
        .exec_fee_factor_raw(&cache)
        .expect_err("missing ExecFeeFactor storage should fault");
    assert!(err.to_string().contains("ExecFeeFactor"), "{err}");

    seed_policy_setting_key(
        &cache,
        PolicyContract::exec_fee_factor_key(),
        i64::from(DEFAULT_EXEC_FEE_FACTOR),
    );
    assert_eq!(
        PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
        i64::from(DEFAULT_EXEC_FEE_FACTOR)
    );
    PolicyContract::new()
        .put_exec_fee_factor(&cache, 50)
        .unwrap();
    assert_eq!(
        PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
        50
    );
    // Overwrite (GetAndChange semantics).
    PolicyContract::new()
        .put_exec_fee_factor(&cache, 100)
        .unwrap();
    assert_eq!(
        PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
        100
    );
}

#[test]
fn snapshot_exec_fee_factor_divides_post_faun_pico_storage() {
    let cache = DataCache::new(false);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);

    seed_policy_setting_key(
        &cache,
        PolicyContract::exec_fee_factor_key(),
        i64::from(DEFAULT_EXEC_FEE_FACTOR) * FEE_FACTOR,
    );

    assert_eq!(
        PolicyContract::new()
            .get_exec_fee_factor_snapshot(&cache, &settings, 0)
            .unwrap(),
        DEFAULT_EXEC_FEE_FACTOR
    );
}

#[test]
fn snapshot_max_valid_until_ignores_policy_storage_before_echidna() {
    let cache = DataCache::new(false);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 100);
    seed_policy_setting_key(
        &cache,
        PolicyContract::max_valid_until_block_increment_key(),
        42,
    );
    seed_current_block(&cache, 0);

    assert_eq!(
        PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(&cache, &settings)
            .unwrap(),
        settings.max_valid_until_block_increment
    );
}

#[test]
fn snapshot_milliseconds_per_block_reads_policy_value_after_echidna() {
    // C# `NeoSystemExtensions.GetTimePerBlock`: from HF_Echidna the per-block
    // time comes from the committee-settable Policy storage (prefix 21), NOT the
    // frozen ProtocolSettings default. The consensus driver reads this each round.
    let cache = DataCache::new(false);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    // Committee changed the block time to a value that differs from the default.
    let committee_value: i64 = 7_000;
    assert_ne!(committee_value as u32, settings.milliseconds_per_block);
    seed_policy_setting_key(
        &cache,
        PolicyContract::milliseconds_per_block_key(),
        committee_value,
    );
    seed_current_block(&cache, 10);

    assert_eq!(
        PolicyContract::new()
            .get_milliseconds_per_block_snapshot(&cache, &settings)
            .unwrap(),
        committee_value as u32,
        "post-Echidna the snapshot reader must return the live Policy value"
    );
}

#[test]
fn snapshot_milliseconds_per_block_ignores_policy_storage_before_echidna() {
    // Pre-Echidna the reader returns the static ProtocolSettings default and
    // ignores any Policy storage value, matching C# `GetTimePerBlock`.
    let cache = DataCache::new(false);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 100);
    seed_policy_setting_key(&cache, PolicyContract::milliseconds_per_block_key(), 7_000);
    seed_current_block(&cache, 0);

    assert_eq!(
        PolicyContract::new()
            .get_milliseconds_per_block_snapshot(&cache, &settings)
            .unwrap(),
        settings.milliseconds_per_block,
        "pre-Echidna the reader must fall back to the ProtocolSettings default"
    );
}

#[test]
fn snapshot_milliseconds_per_block_falls_back_when_key_missing_after_echidna() {
    // C# pre-genesis fallback: Echidna active but the Policy key not yet written
    // (or the ledger has no current block) → ProtocolSettings default.
    let cache = DataCache::new(false);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    seed_current_block(&cache, 5);
    // Intentionally no milliseconds_per_block_key written.

    assert_eq!(
        PolicyContract::new()
            .get_milliseconds_per_block_snapshot(&cache, &settings)
            .unwrap(),
        settings.milliseconds_per_block,
        "missing Policy key post-Echidna must fall back to the default"
    );
}

#[test]
fn set_fee_per_byte_validation_bounds() {
    // C# SetFeePerByte accepts [0, 100000000] and rejects outside.
    assert!(PolicyContract::validate_fee_per_byte(0).is_ok());
    assert!(PolicyContract::validate_fee_per_byte(MAX_FEE_PER_BYTE).is_ok());
    assert!(PolicyContract::validate_fee_per_byte(-1).is_err());
    assert!(PolicyContract::validate_fee_per_byte(MAX_FEE_PER_BYTE + 1).is_err());
}

#[test]
fn blocked_account_key_block_then_unblock_storage_effect() {
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[4u8; 20]).unwrap();
    let key = PolicyContract::blocked_account_key(&account);
    // Not blocked initially.
    assert!(cache.get(&key).is_none());
    // Block (add) then unblock (delete) — the exact storage effect the
    // isBlocked / unblockAccount arms rely on.
    cache.add(key.clone(), StorageItem::from_bytes(vec![]));
    assert!(cache.get(&key).is_some());
    cache.delete(&key);
    assert!(cache.get(&key).is_none());
}

#[test]
fn fee_per_byte_write_then_read_round_trips() {
    let cache = DataCache::new(false);
    // Writing via the setter's storage effect is observed by the getter,
    // exercising the GetAndChange (overwrite-as-Changed) semantics.
    seed_policy_setting_key(
        &cache,
        PolicyContract::fee_per_byte_key(),
        i64::from(DEFAULT_FEE_PER_BYTE),
    );
    PolicyContract::new()
        .put_fee_per_byte(&cache, 4242)
        .unwrap();
    assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 4242);
    // Overwriting an existing value is read back as the new value.
    PolicyContract::new()
        .put_fee_per_byte(&cache, 5000)
        .unwrap();
    assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 5000);
}

#[test]
fn set_storage_price_validation_bounds() {
    // C# SetStoragePrice accepts [1, MaxStoragePrice] and rejects outside.
    assert!(PolicyContract::validate_storage_price(1).is_ok());
    assert!(PolicyContract::validate_storage_price(MAX_STORAGE_PRICE).is_ok());
    assert!(PolicyContract::validate_storage_price(0).is_err());
    assert!(PolicyContract::validate_storage_price(MAX_STORAGE_PRICE + 1).is_err());
}

#[test]
fn storage_price_write_then_read_round_trips() {
    let cache = DataCache::new(false);
    seed_policy_setting_key(
        &cache,
        PolicyContract::storage_price_key(),
        DEFAULT_STORAGE_PRICE,
    );
    PolicyContract::new()
        .put_storage_price(&cache, 250_000)
        .unwrap();
    assert_eq!(
        PolicyContract::new().storage_price(&cache).unwrap(),
        250_000
    );
    PolicyContract::new()
        .put_storage_price(&cache, 1_000_000)
        .unwrap();
    assert_eq!(
        PolicyContract::new().storage_price(&cache).unwrap(),
        1_000_000
    );
}

#[test]
fn set_milliseconds_per_block_validation_bounds() {
    // C# SetMillisecondsPerBlock accepts [1, MaxMillisecondsPerBlock].
    assert!(PolicyContract::validate_milliseconds_per_block(1).is_ok());
    assert!(PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK).is_ok());
    assert!(PolicyContract::validate_milliseconds_per_block(0).is_err());
    assert!(
        PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK + 1).is_err()
    );
}

#[test]
fn milliseconds_per_block_write_persists_to_storage() {
    let cache = DataCache::new(false);
    seed_policy_setting_key(&cache, PolicyContract::milliseconds_per_block_key(), 15_000);
    PolicyContract::new()
        .put_milliseconds_per_block(&cache, 7_000)
        .unwrap();
    // Read back the raw storage value (the engine-aware getter adds the
    // ProtocolSettings default, which isn't needed once a value is stored).
    assert_eq!(
        read_policy_setting_key(&cache, PolicyContract::milliseconds_per_block_key()),
        7_000
    );
}

#[test]
fn max_chain_param_setter_range_bounds() {
    // C# MaxMaxValidUntilBlockIncrement = 86400, MaxMaxTraceableBlocks = 2102400.
    assert!(PolicyContract::validate_max_valid_until_block_increment(1).is_ok());
    assert!(
        PolicyContract::validate_max_valid_until_block_increment(
            MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT
        )
        .is_ok()
    );
    assert!(PolicyContract::validate_max_valid_until_block_increment(0).is_err());
    assert!(
        PolicyContract::validate_max_valid_until_block_increment(
            MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT + 1
        )
        .is_err()
    );

    assert!(PolicyContract::validate_max_traceable_blocks(1).is_ok());
    assert!(PolicyContract::validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS).is_ok());
    assert!(PolicyContract::validate_max_traceable_blocks(0).is_err());
    assert!(PolicyContract::validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS + 1).is_err());
}

#[test]
fn max_chain_param_writes_persist_to_storage() {
    let cache = DataCache::new(false);
    seed_policy_setting_key(
        &cache,
        PolicyContract::max_valid_until_block_increment_key(),
        DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT as i64,
    );
    PolicyContract::new()
        .put_max_valid_until_block_increment(&cache, 5_000)
        .unwrap();
    assert_eq!(
        read_policy_setting_key(
            &cache,
            PolicyContract::max_valid_until_block_increment_key()
        ),
        5_000
    );
    seed_policy_setting_key(
        &cache,
        PolicyContract::max_traceable_blocks_key(),
        2_102_400,
    );
    PolicyContract::new()
        .put_max_traceable_blocks(&cache, 1_000_000)
        .unwrap();
    assert_eq!(
        read_policy_setting_key(&cache, PolicyContract::max_traceable_blocks_key()),
        1_000_000
    );
}

#[test]
fn is_blocked_checks_storage_existence() {
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[3u8; 20]).unwrap();
    let key = PolicyContract::blocked_account_key(&account);
    // Not blocked until a record exists.
    assert!(cache.get(&key).is_none());
    cache.add(key.clone(), StorageItem::from_bytes(vec![]));
    assert!(cache.get(&key).is_some());
}

#[test]
fn is_contract_blocked_trait_reflects_blocked_list() {
    // Regression: the engine's contract-invocation gate (contracts.rs) calls
    // the NativeContract::is_contract_blocked TRAIT method. It must reflect
    // the blocked-account list rather than the default Ok(false) — otherwise
    // a blocked contract could be invoked, diverging from C#.
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
    let policy = PolicyContract::new();
    assert!(
        !<PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash).unwrap()
    );
    cache.add(
        PolicyContract::blocked_account_key(&hash),
        StorageItem::from_bytes(vec![]),
    );
    assert!(
        <PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash).unwrap()
    );
}

#[test]
fn fee_per_byte_reads_storage_with_default() {
    let cache = DataCache::new(false);
    let err = PolicyContract::new()
        .fee_per_byte(&cache)
        .expect_err("missing FeePerByte storage should fault");
    assert!(err.to_string().contains("FeePerByte"), "{err}");

    // A configured value is read back from the BigInteger storage item.
    seed_policy_setting_key(&cache, PolicyContract::fee_per_byte_key(), 4242);
    assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 4242);
}

#[test]
fn storage_price_reads_storage_with_default() {
    let cache = DataCache::new(false);
    let err = PolicyContract::new()
        .storage_price(&cache)
        .expect_err("missing StoragePrice storage should fault");
    assert!(err.to_string().contains("StoragePrice"), "{err}");

    seed_policy_setting_key(&cache, PolicyContract::storage_price_key(), 250_000);
    assert_eq!(
        PolicyContract::new().storage_price(&cache).unwrap(),
        250_000
    );
}

#[test]
fn echidna_policy_settings_require_initialized_storage() {
    let cache = DataCache::new(false);
    seed_current_block(&cache, 0);
    let mut settings = neo_config::ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    let engine = ApplicationEngine::new_with_native_contract_provider(
        neo_primitives::TriggerType::Application,
        None,
        std::sync::Arc::new(cache),
        None,
        settings.clone(),
        0,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");

    for (name, result) in [
        (
            "MillisecondsPerBlock",
            PolicyContract::new().read_milliseconds_per_block(&engine),
        ),
        (
            "MaxValidUntilBlockIncrement",
            PolicyContract::new().read_max_valid_until_block_increment(&engine),
        ),
        (
            "MaxTraceableBlocks",
            PolicyContract::new()
                .max_traceable_blocks(&engine)
                .map(i64::from),
        ),
    ] {
        let err = result.expect_err("missing Echidna policy storage should fault");
        assert!(err.to_string().contains(name), "{err}");
    }

    let snapshot = engine.snapshot_cache();
    assert_eq!(
        PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(&snapshot, &settings)
            .unwrap(),
        settings.max_valid_until_block_increment
    );
    assert_eq!(
        PolicyContract::new()
            .get_max_traceable_blocks_snapshot(&snapshot, &settings)
            .unwrap(),
        settings.max_traceable_blocks
    );
}

#[test]
fn whitelisted_contract_struct_round_trips() {
    // C# WhitelistedContract.ToStackItem/FromStackItem: a Struct of
    // [ContractHash, Method, ArgCount, FixedFee].
    let view = WhitelistedContractView {
        contract_hash: UInt160::from_bytes(&[0x42; 20]).unwrap(),
        method: "balanceOf".to_string(),
        arg_count: 1,
        fixed_fee: 123_456,
    };
    let bytes = PolicyContract::encode_whitelisted_contract(&view).unwrap();
    let expected_item = StackItem::from_struct(vec![
        StackItem::from_byte_string(view.contract_hash.to_bytes()),
        StackItem::from_byte_string(view.method.as_bytes().to_vec()),
        StackItem::from_int(BigInt::from(view.arg_count)),
        StackItem::from_int(BigInt::from(view.fixed_fee)),
    ]);
    let expected =
        BinarySerializer::serialize(&expected_item, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(bytes, expected);
    let decoded = PolicyContract::decode_whitelisted_contract(&bytes).unwrap();
    assert_eq!(decoded, view);
    let produced_item = Interoperable::to_stack_item(&view).unwrap();
    assert!(
        stack_item_struct_eq(&produced_item, &expected_item),
        "structural StackItem mismatch: {produced_item:?} vs {expected_item:?}"
    );

    let mut trait_decoded = WhitelistedContractView {
        contract_hash: UInt160::from_bytes(&[0x00; 20]).unwrap(),
        method: String::new(),
        arg_count: 0,
        fixed_fee: 0,
    };
    Interoperable::from_stack_item(&mut trait_decoded, expected_item).unwrap();
    assert_eq!(trait_decoded, view);
}

#[test]
fn whitelisted_contract_storage_uses_stack_item_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let source = include_str!("../../policy_contract/storage/whitelist.rs");
    let decoder = slice_between(
        source,
        "fn decode_whitelisted_contract",
        "fn encode_whitelisted_contract",
    );
    assert!(decoder.contains("decode_stack_item"));
    assert!(decoder.contains("WhitelistedContractView::from_stack_item"));
    assert!(!decoder.contains("BinarySerializer::deserialize("));

    let encoder = slice_between(
        source,
        "fn encode_whitelisted_contract",
        "fn whitelist_fee_entries",
    );
    assert!(encoder.contains("encode_storage_struct"));
    assert!(!encoder.contains("StackItem::from_struct"));
    assert!(!encoder.contains("BinarySerializer::serialize("));
}

#[test]
fn committee_cache_reader_uses_stack_item_projection() {
    let source = include_str!("../../policy_contract/storage/recovery.rs");
    let start = source
        .find("fn read_neo_committee_sorted")
        .expect("committee reader exists");
    let end = source[start..]
        .find("fn assert_almost_full_committee")
        .map(|offset| start + offset)
        .expect("assert_almost_full_committee follows committee reader");
    let helper = &source[start..end];

    assert!(helper.contains("decode_stack_item"));
    assert!(helper.contains("CachedCommittee::from_stack_item"));
    assert!(!helper.contains("BinarySerializer::deserialize("));
    assert!(!helper.contains("StackItem::Array"));
    assert!(!helper.contains("StackItem::Struct"));
}

#[test]
fn whitelist_fee_key_is_prefix_hash_and_big_endian_offset() {
    // C# CreateStorageKey(Prefix_WhitelistedFeeContracts, contractHash,
    // methodDescriptor.Offset): [16] ++ hash(20) ++ offset as big-endian i32.
    let hash = UInt160::from_bytes(&[0xAB; 20]).unwrap();
    let prefix = PolicyContract::whitelist_contract_prefix_key(&hash);
    assert_eq!(prefix.id(), PolicyContract::ID);
    assert_eq!(prefix.suffix().len(), 1 + 20);
    assert_eq!(prefix.suffix()[0], PREFIX_WHITELISTED_FEE_CONTRACTS);
    assert_eq!(&prefix.suffix()[1..], &[0xAB; 20]);

    let key = PolicyContract::whitelist_fee_key(&hash, 0x0102_0304);
    let suffix = key.suffix();
    assert_eq!(suffix.len(), 1 + 20 + 4);
    assert_eq!(suffix[0], PREFIX_WHITELISTED_FEE_CONTRACTS);
    assert_eq!(&suffix[1..21], &[0xAB; 20]);
    assert_eq!(&suffix[21..25], &[0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn whitelist_fee_entries_scope_to_prefix() {
    let cache = DataCache::new(false);
    let h1 = UInt160::from_bytes(&[0x11; 20]).unwrap();
    let h2 = UInt160::from_bytes(&[0x22; 20]).unwrap();
    let entry = |hash: &UInt160, method: &str| {
        PolicyContract::encode_whitelisted_contract(&WhitelistedContractView {
            contract_hash: *hash,
            method: method.to_string(),
            arg_count: 0,
            fixed_fee: 5,
        })
        .unwrap()
    };
    cache.add(
        PolicyContract::whitelist_fee_key(&h1, 0),
        StorageItem::from_bytes(entry(&h1, "a")),
    );
    cache.add(
        PolicyContract::whitelist_fee_key(&h2, 7),
        StorageItem::from_bytes(entry(&h2, "b")),
    );
    // An unrelated blocked-account record must not appear.
    cache.add(
        PolicyContract::blocked_account_key(&h1),
        StorageItem::from_bytes(Vec::new()),
    );

    let entries = PolicyContract::new().whitelist_fee_entries(&cache);
    assert_eq!(entries.len(), 2);
    for (key, _) in &entries {
        assert_eq!(key.suffix()[0], PREFIX_WHITELISTED_FEE_CONTRACTS);
        assert_eq!(key.suffix().len(), 1 + 20 + 4);
    }
}

#[test]
fn native_hashes_cannot_be_blocked() {
    // C# BlockAccountInternal: IsNative(account) -> fault. All 11 canonical
    // native hashes must be covered; a regular account must not.
    for spec in crate::standard_native_contract_specs() {
        assert!(
            crate::is_standard_native_contract_hash(&spec.hash),
            "{} ({}) is native",
            spec.name,
            spec.hash
        );
    }
    assert!(!crate::is_standard_native_contract_hash(
        &UInt160::from_bytes(&[0x42; 20]).unwrap()
    ));
}

#[test]
fn remaining_time_message_matches_csharp_format() {
    // C# RecoverFund's ternary chain: days -> "{d}d {h}h {m}m",
    // hours -> "{h}h {m}m {s}s", minutes -> "{m}m {s}s", else "{s}s".
    let ms =
        |d: i64, h: i64, m: i64, s: i64| d * 86_400_000 + h * 3_600_000 + m * 60_000 + s * 1_000;
    assert_eq!(
        PolicyContract::format_remaining_time(&BigInt::from(ms(2, 3, 4, 5))),
        "2d 3h 4m"
    );
    assert_eq!(
        PolicyContract::format_remaining_time(&BigInt::from(ms(0, 3, 4, 5))),
        "3h 4m 5s"
    );
    assert_eq!(
        PolicyContract::format_remaining_time(&BigInt::from(ms(0, 0, 4, 5))),
        "4m 5s"
    );
    assert_eq!(
        PolicyContract::format_remaining_time(&BigInt::from(ms(0, 0, 0, 5))),
        "5s"
    );
    assert_eq!(
        PolicyContract::format_remaining_time(&BigInt::from(999)),
        "0s"
    );
}

#[test]
fn required_recover_fund_time_is_one_year_of_milliseconds() {
    // C# RequiredTimeForRecoverFund = 365 * 24 * 60 * 60 * 1_000UL.
    assert_eq!(REQUIRED_TIME_FOR_RECOVER_FUND, 31_536_000_000);
}
