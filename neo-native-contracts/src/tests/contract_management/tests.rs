use super::*;
use neo_config::Hardfork;
use neo_primitives::{CallFlags, ContractParameterType};
use neo_storage::StorageItem;
use neo_vm::StackItem;

#[test]
fn native_contract_surface() {
    let c = ContractManagement::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "getContract",
            "getContractById",
            "getMinimumDeploymentFee",
            "isContract",
            "hasMethod",
            "setMinimumDeploymentFee",
            "getContractHashes",
            "destroy",
            "deploy",
            "deploy",
            "update",
            "update"
        ]
    );
    // getContractHashes is a safe, ReadStates, no-arg iterator reader.
    let hashes = c
        .methods()
        .iter()
        .find(|m| m.name == "getContractHashes")
        .unwrap();
    assert!(hashes.safe && hashes.active_in.is_none());
    assert!(hashes.parameters.is_empty());
    assert_eq!(hashes.return_type, ContractParameterType::InteropInterface);
    assert_eq!(hashes.required_call_flags, CallFlags::READ_STATES.bits());
    // The committee-gated setter: not safe, States, Integer -> Void.
    let setter = c
        .methods()
        .iter()
        .find(|m| m.name == "setMinimumDeploymentFee")
        .unwrap();
    assert!(!setter.safe);
    assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(setter.return_type, ContractParameterType::Void);
    assert_eq!(setter.cpu_fee, 1 << 15);
    assert!(setter.active_in.is_none());
    let has_method = c.methods().iter().find(|m| m.name == "hasMethod").unwrap();
    assert!(has_method.active_in.is_none());
    assert_eq!(has_method.return_type, ContractParameterType::Boolean);
    assert_eq!(has_method.parameters.len(), 3);

    let get_contract = c
        .methods()
        .iter()
        .find(|m| m.name == "getContract")
        .unwrap();
    assert_eq!(
        get_contract.parameters,
        vec![ContractParameterType::Hash160]
    );
    assert_eq!(get_contract.return_type, ContractParameterType::Array);
    assert_eq!(get_contract.cpu_fee, 1 << 15);
    assert!(get_contract.safe && get_contract.active_in.is_none());

    let by_id = c
        .methods()
        .iter()
        .find(|m| m.name == "getContractById")
        .unwrap();
    assert_eq!(by_id.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(by_id.return_type, ContractParameterType::Array);
    assert_eq!(by_id.cpu_fee, 1 << 15);

    let is_contract = c.methods().iter().find(|m| m.name == "isContract").unwrap();
    assert_eq!(is_contract.parameters, vec![ContractParameterType::Hash160]);
    assert_eq!(is_contract.return_type, ContractParameterType::Boolean);
    assert_eq!(is_contract.cpu_fee, 1 << 14);
    assert_eq!(is_contract.active_in, Some(Hardfork::HfEchidna));

    let mut hardforks = std::collections::HashMap::new();
    hardforks.insert(Hardfork::HfEchidna, 100);
    let settings = neo_config::ProtocolSettings {
        hardforks,
        ..neo_config::ProtocolSettings::csharp_default()
    };
    let pre_echidna_state =
        neo_execution::native_contract::build_native_contract_state(&c, &settings, 0);
    assert!(ContractManagement::abi_has_method(
        &pre_echidna_state.manifest,
        "hasMethod",
        3
    ));
    assert!(!ContractManagement::abi_has_method(
        &pre_echidna_state.manifest,
        "isContract",
        1
    ));

    // destroy(): not safe, States|AllowNotify, no params, Void, no hardfork
    // (C# [ContractMethod(CpuFee = 1 << 15,
    // RequiredCallFlags = CallFlags.States | CallFlags.AllowNotify)]).
    let destroys: Vec<_> = c.methods().iter().filter(|m| m.name == "destroy").collect();
    assert_eq!(destroys.len(), 1);
    let destroy = destroys[0];
    assert!(!destroy.safe);
    assert_eq!(
        destroy.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert!(destroy.parameters.is_empty());
    assert_eq!(destroy.return_type, ContractParameterType::Void);
    assert_eq!(destroy.cpu_fee, 1 << 15);
    assert!(destroy.active_in.is_none());
    assert!(destroy.deprecated_in.is_none());

    // deploy x2 / update x2: C# [ContractMethod(RequiredCallFlags =
    // CallFlags.States | CallFlags.AllowNotify)] — CpuFee/StorageFee 0
    // (fees are charged inside the body), not safe, no hardfork gate.
    let deploys: Vec<_> = c.methods().iter().filter(|m| m.name == "deploy").collect();
    assert_eq!(deploys.len(), 2);
    let updates: Vec<_> = c.methods().iter().filter(|m| m.name == "update").collect();
    assert_eq!(updates.len(), 2);
    for method in deploys.iter().chain(updates.iter()) {
        assert!(!method.safe);
        assert_eq!(method.cpu_fee, 0);
        assert_eq!(method.storage_fee, 0);
        assert_eq!(
            method.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert!(method.active_in.is_none());
        assert_eq!(method.parameters[0], ContractParameterType::ByteArray);
        assert_eq!(method.parameters[1], ContractParameterType::ByteArray);
    }
    // Overloads: (nef, manifest) and (nef, manifest, data: Any).
    assert_eq!(deploys[0].parameters.len(), 2);
    assert_eq!(deploys[1].parameters.len(), 3);
    assert_eq!(deploys[1].parameters[2], ContractParameterType::Any);
    assert_eq!(updates[0].parameters.len(), 2);
    assert_eq!(updates[1].parameters.len(), 3);
    assert_eq!(updates[1].parameters[2], ContractParameterType::Any);
    // deploy returns the new ContractState (Array); update is Void.
    assert!(
        deploys
            .iter()
            .all(|m| m.return_type == ContractParameterType::Array)
    );
    assert!(
        updates
            .iter()
            .all(|m| m.return_type == ContractParameterType::Void)
    );
}

#[test]
fn storage_key_helpers_match_csharp_layout() {
    let hash = UInt160::from_bytes(&[0x42; 20]).unwrap();

    let contract_key = ContractManagement::contract_storage_key(&hash);
    assert_eq!(contract_key.id(), ContractManagement::ID);
    assert_eq!(contract_key.suffix()[0], PREFIX_CONTRACT);
    assert_eq!(&contract_key.suffix()[1..], hash.to_bytes().as_slice());

    let id_key = ContractManagement::contract_id_storage_key(0x0102_0304);
    assert_eq!(id_key.id(), ContractManagement::ID);
    assert_eq!(id_key.suffix(), &[PREFIX_CONTRACT_HASH, 1, 2, 3, 4]);

    let id_prefix = ContractManagement::contract_id_prefix_key();
    assert_eq!(id_prefix.id(), ContractManagement::ID);
    assert_eq!(id_prefix.suffix(), &[PREFIX_CONTRACT_HASH]);

    let minimum_fee_key = ContractManagement::minimum_deployment_fee_key();
    assert_eq!(minimum_fee_key.id(), ContractManagement::ID);
    assert_eq!(minimum_fee_key.suffix(), &[PREFIX_MINIMUM_DEPLOYMENT_FEE]);

    let next_id_key = ContractManagement::next_available_id_key();
    assert_eq!(next_id_key.id(), ContractManagement::ID);
    assert_eq!(next_id_key.suffix(), &[PREFIX_NEXT_AVAILABLE_ID]);
}

#[test]
fn clean_whitelist_storage_decode_uses_stack_value_projection() {
    let source = include_str!("../../contract_management/operations/storage.rs");
    let start = source
        .find("fn policy_clean_whitelist")
        .expect("policy_clean_whitelist exists");
    let end = source[start..]
        .find("fn read_required_i64_setting")
        .map(|offset| start + offset)
        .expect("following helper exists");
    let helper = &source[start..end];

    assert!(helper.contains("decode_stack_value"));
    assert!(helper.contains("StructDecoder"));
    assert!(!helper.contains("BinarySerializer::deserialize("));
    assert!(!helper.contains("StackItem::Struct"));
}

#[test]
fn get_contract_miss_returns_none() {
    // C# `GetContract` returns null for an unknown hash; the invoke arm maps
    // `None` to an empty payload, which the engine decodes to StackItem::Null.
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
    assert!(
        ContractManagement::get_contract_from_snapshot(&cache, &hash)
            .unwrap()
            .is_none()
    );
}

#[test]
fn get_contract_by_id_miss_returns_none() {
    // C# `GetContractById` returns null when the id has no hash-index entry;
    // the invoke arm maps that to an empty payload (StackItem::Null).
    let cache = DataCache::new(false);
    assert!(
        ContractManagement::get_contract_by_id_from_snapshot(&cache, 42)
            .unwrap()
            .is_none()
    );
}

#[test]
fn contract_hash_entries_scopes_to_prefix_contract_hash() {
    let cache = DataCache::new(false);
    // Two Prefix_ContractHash entries (id -> hash) plus an unrelated
    // Prefix_Contract entry that must NOT appear in the iterator's backing set.
    let k1 = ContractManagement::contract_id_storage_key(1);
    let k2 = ContractManagement::contract_id_storage_key(2);
    cache.add(k1, StorageItem::from_bytes(vec![0xAA; 20]));
    cache.add(k2, StorageItem::from_bytes(vec![0xBB; 20]));
    cache.add(
        ContractManagement::contract_storage_key(&UInt160::zero()),
        StorageItem::from_bytes(vec![1]),
    );

    let entries = ContractManagement::new().contract_hash_entries(&cache);
    assert_eq!(
        entries.len(),
        2,
        "only Prefix_ContractHash entries are included"
    );
    // Forward-seek order: id 1 before id 2 (big-endian id keys sort ascending).
    assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xAA; 20]);
    assert_eq!(entries[1].1.value_bytes().to_vec(), vec![0xBB; 20]);
}

#[test]
fn contract_hash_entries_skips_native_negative_ids() {
    // C# GetContractHashes filters `ReadInt32BigEndian(key.Key[1..]) >= 0`:
    // native contracts (negative ids) never appear in the iterator.
    let cache = DataCache::new(false);
    for id in [-1i32, -11] {
        let key = ContractManagement::contract_id_storage_key(id);
        cache.add(key, StorageItem::from_bytes(vec![0xCC; 20]));
    }
    let user = ContractManagement::contract_id_storage_key(1);
    cache.add(user, StorageItem::from_bytes(vec![0xDD; 20]));

    let entries = ContractManagement::new().contract_hash_entries(&cache);
    assert_eq!(entries.len(), 1, "native (negative-id) entries are skipped");
    assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xDD; 20]);
    // id 0 is the boundary: C# keeps `Id >= 0`.
    let zero = ContractManagement::contract_id_storage_key(0);
    cache.add(zero, StorageItem::from_bytes(vec![0xEE; 20]));
    assert_eq!(
        ContractManagement::new()
            .contract_hash_entries(&cache)
            .len(),
        2
    );
}

#[test]
fn get_contract_by_id_round_trips_through_the_id_index() {
    // Deploy-shaped fixture: the per-contract record (prefix 8) plus the
    // big-endian id -> hash index entry (prefix 12), as written by C#
    // Deploy; GetContractById resolves the id through the index and then
    // dereferences the hash.
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[0x42u8; 20]).unwrap();
    let state = ContractState::new_native(7, hash, "TestUserContract".to_string());
    cache.add(
        ContractManagement::contract_storage_key(&hash),
        StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
    );
    cache.add(
        ContractManagement::contract_id_storage_key(7),
        StorageItem::from_bytes(hash.to_bytes().to_vec()),
    );

    let fetched = ContractManagement::get_contract_by_id_from_snapshot(&cache, 7)
        .unwrap()
        .expect("id 7 resolves to the deployed contract");
    assert_eq!(fetched.id, 7);
    assert_eq!(fetched.hash, hash);
    // A different id still misses.
    assert!(
        ContractManagement::get_contract_by_id_from_snapshot(&cache, 8)
            .unwrap()
            .is_none()
    );
}

#[test]
fn get_contract_by_id_ignores_legacy_little_endian_index_like_csharp_v3100() {
    // C# v3.10 uses StorageKey.Create(id, prefix, int), which appends the
    // contract id in big-endian form. A little-endian compatibility key is
    // not a valid v3.10 lookup path.
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[0x24u8; 20]).unwrap();
    let state = ContractState::new_native(7, hash, "LegacyIndexFixture".to_string());
    cache.add(
        ContractManagement::contract_storage_key(&hash),
        StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
    );
    // Legacy entry written with a LITTLE-endian id suffix (historical bug);
    // modern entries use big-endian. `contract_hash_entries` must still skip it.
    let legacy_key = StorageKey::create_with_bytes(
        ContractManagement::ID,
        PREFIX_CONTRACT_HASH,
        &7i32.to_le_bytes(),
    );
    cache.add(
        legacy_key,
        StorageItem::from_bytes(hash.to_bytes().to_vec()),
    );

    assert!(
        ContractManagement::get_contract_by_id_from_snapshot(&cache, 7)
            .unwrap()
            .is_none()
    );
}

#[test]
fn has_method_resolves_contract_from_snapshot() {
    use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition};
    // The hasMethod invoke arm = GetContract(hash) -> Abi.GetMethod(name,
    // pcount) != null; exercise the same composition over a seeded record.
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[0x51u8; 20]).unwrap();
    let mut state = ContractState::new_native(9, hash, "HasMethodFixture".to_string());
    state.manifest.abi.methods.push(ContractMethodDescriptor {
        name: "transfer".to_string(),
        parameters: vec![ContractParameterDefinition::default(); 4],
        ..Default::default()
    });
    cache.add(
        ContractManagement::contract_storage_key(&hash),
        StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
    );

    let fetched = ContractManagement::get_contract_from_snapshot(&cache, &hash)
        .unwrap()
        .expect("contract record resolves");
    // Positive: exact pcount and the -1 wildcard.
    assert!(ContractManagement::abi_has_method(
        &fetched.manifest,
        "transfer",
        4
    ));
    assert!(ContractManagement::abi_has_method(
        &fetched.manifest,
        "transfer",
        -1
    ));
    // Negative: wrong pcount / unknown name.
    assert!(!ContractManagement::abi_has_method(
        &fetched.manifest,
        "transfer",
        3
    ));
    assert!(!ContractManagement::abi_has_method(
        &fetched.manifest,
        "balanceOf",
        -1
    ));
    // Missing contract -> C# returns false before any ABI lookup.
    let absent = UInt160::from_bytes(&[0x52u8; 20]).unwrap();
    assert!(
        ContractManagement::get_contract_from_snapshot(&cache, &absent)
            .unwrap()
            .is_none()
    );
}

#[test]
fn has_method_rejects_invalid_utf8_method_name_like_csharp() {
    // C# NativeContract.Invoke converts string parameters through
    // StackItem.GetString(), so invalid UTF-8 faults instead of being repaired.
    let cache = DataCache::new(false);
    let mut engine = ApplicationEngine::new(
        neo_primitives::TriggerType::Application,
        None,
        std::sync::Arc::new(cache),
        None,
        neo_config::ProtocolSettings::default(),
        0,
        None,
    )
    .expect("engine builds");
    let hash = UInt160::from_bytes(&[0x51u8; 20]).unwrap();
    let err = ContractManagement::new()
        .invoke(
            &mut engine,
            "hasMethod",
            &[
                hash.to_bytes().to_vec(),
                vec![0xFF],
                BigInt::from(0).to_signed_bytes_le(),
            ],
        )
        .expect_err("invalid UTF-8 method names must fault");
    assert!(err.to_string().contains("bad method name"), "{err}");
}

#[test]
fn invoke_argument_parsing_uses_shared_raw_helpers() {
    fn arm_between<'a>(source: &'a str, arm: &str, next_arm: &str) -> &'a str {
        let start = source.find(arm).expect("invoke arm exists");
        let end = source[start..]
            .find(next_arm)
            .map(|offset| start + offset)
            .expect("following invoke arm exists");
        &source[start..end]
    }

    let source = include_str!("../../contract_management/invoke.rs");
    let by_id = arm_between(
        source,
        "\"getContractById\" =>",
        "\"getMinimumDeploymentFee\"",
    );
    assert!(by_id.contains("crate::args::raw_i32_arg"));
    assert!(!by_id.contains("BigInt::from_signed_bytes_le(args"));

    let has_method = arm_between(source, "\"hasMethod\" =>", "// Both deploy overloads");
    assert!(has_method.contains("crate::args::raw_string_arg"));
    assert!(has_method.contains("crate::args::raw_i32_arg"));
    assert!(!has_method.contains("String::from_utf8("));
    assert!(!has_method.contains("BigInt::from_signed_bytes_le(args"));
}

#[test]
fn is_native_contract_hash_covers_all_eleven_natives() {
    for spec in crate::standard_native_contract_specs() {
        assert!(
            crate::is_standard_native_contract_hash(&spec.hash),
            "{} is native",
            spec.name
        );
    }
    let user = UInt160::from_bytes(&[0x99u8; 20]).unwrap();
    assert!(!crate::is_standard_native_contract_hash(&user));
}

#[test]
fn policy_blocked_account_key_matches_policy_layout() {
    // The cross-native blocked-account key must match PolicyContract's own
    // layout: (PolicyContract.ID, [Prefix_BlockedAccount, account]).
    let account = UInt160::from_bytes(&[0x77u8; 20]).unwrap();
    let key = crate::PolicyContract::blocked_account_key(&account);
    assert_eq!(key.id, crate::PolicyContract::ID);
    assert_eq!(key.suffix().len(), 1 + UInt160::LENGTH);
    assert_eq!(&key.suffix()[1..], account.to_bytes().as_slice());
}

#[test]
fn set_minimum_deployment_fee_write_round_trips() {
    // The setter's storage effect (overwrite Prefix_MinimumDeploymentFee) is
    // observed by the getMinimumDeploymentFee reader, matching C#
    // GetAndChange(...).Set(value).
    let cache = DataCache::new(false);
    cache.add(
        ContractManagement::minimum_deployment_fee_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_MINIMUM_DEPLOYMENT_FEE,
        ))),
    );
    // Zero is permitted (C# rejects only value < 0).
    ContractManagement::new()
        .put_minimum_deployment_fee(&cache, &BigInt::from(0))
        .unwrap();
    assert_eq!(
        storage_key_int(&cache, ContractManagement::minimum_deployment_fee_key()),
        Some(BigInt::from(0))
    );
    // Overwrite with a positive fee (GetAndChange semantics).
    ContractManagement::new()
        .put_minimum_deployment_fee(&cache, &BigInt::from(25_00000000i64))
        .unwrap();
    assert_eq!(
        storage_key_int(&cache, ContractManagement::minimum_deployment_fee_key()),
        Some(BigInt::from(25_00000000i64))
    );
}

#[test]
fn abi_has_method_matches_name_and_pcount() {
    use neo_manifest::{ContractManifest, ContractMethodDescriptor, ContractParameterDefinition};
    let mut manifest = ContractManifest::new("test".to_string());
    manifest.abi.methods.push(ContractMethodDescriptor {
        name: "transfer".to_string(),
        parameters: vec![ContractParameterDefinition::default(); 4],
        ..Default::default()
    });

    // Exact (name, count) match.
    assert!(ContractManagement::abi_has_method(&manifest, "transfer", 4));
    // Wrong count -> no match.
    assert!(!ContractManagement::abi_has_method(
        &manifest, "transfer", 3
    ));
    // pcount == -1 matches any count.
    assert!(ContractManagement::abi_has_method(
        &manifest, "transfer", -1
    ));
    // Unknown name -> no match.
    assert!(!ContractManagement::abi_has_method(
        &manifest,
        "balanceOf",
        -1
    ));
    // Empty manifest -> no match.
    assert!(!ContractManagement::abi_has_method(
        &ContractManifest::new("e".to_string()),
        "transfer",
        -1
    ));
}

#[test]
fn is_contract_checks_storage_existence() {
    let cache = DataCache::new(false);
    let hash = UInt160::from_bytes(&[8u8; 20]).unwrap();
    assert!(!ContractManagement::is_contract(&cache, &hash));
    cache.add(
        ContractManagement::contract_storage_key(&hash),
        StorageItem::from_bytes(vec![1]),
    );
    assert!(ContractManagement::is_contract(&cache, &hash));
}

#[test]
fn contract_state_marshals_to_five_element_array() {
    // getContract's hit path serializes the same 5-field Array
    // (id, updateCounter, hash, nef, manifest) as C# ContractState.ToStackItem.
    let state = ContractState::default();
    let legacy_item = StackItem::try_from(state.to_stack_value()).unwrap();
    let expected =
        BinarySerializer::serialize(&legacy_item, &ExecutionEngineLimits::default()).unwrap();
    let bytes = ContractManagement::contract_state_to_bytes(&state, "test").unwrap();
    assert_eq!(bytes, expected);
    assert!(!bytes.is_empty());
    let decoded =
        BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
    match decoded {
        StackItem::Array(array) => assert_eq!(array.items().len(), 5),
        other => panic!("expected Array, got {other:?}"),
    }

    let source = include_str!("../../contract_management/operations/storage.rs");
    let start = source
        .find("fn contract_state_to_bytes")
        .expect("contract_state_to_bytes helper exists");
    let end = source[start..]
        .find("fn contract_hash_entries")
        .map(|offset| start + offset)
        .expect("contract_hash_entries follows contract_state_to_bytes");
    let helper = &source[start..end];

    assert!(helper.contains("encode_storage_struct"));
    assert!(!helper.contains("to_stack_item"));
    assert!(!helper.contains("BinarySerializer::serialize("));
}

#[test]
fn minimum_deployment_fee_requires_initialized_storage() {
    let cache = DataCache::new(false);
    let mut engine = ApplicationEngine::new(
        neo_primitives::TriggerType::Application,
        None,
        std::sync::Arc::new(cache),
        None,
        neo_config::ProtocolSettings::default(),
        0,
        None,
    )
    .expect("engine builds");

    let err = ContractManagement::new()
        .invoke(&mut engine, "getMinimumDeploymentFee", &[])
        .expect_err("missing minimum deployment fee storage should fault");
    assert!(err.to_string().contains("MinimumDeploymentFee"), "{err}");
}

/// A minimal deployable manifest: one `main()` method at offset 0 (the
/// ABI must be non-empty, C# `ContractAbi.FromJson`).
fn deployable_manifest(name: &str) -> ContractManifest {
    use neo_manifest::ContractMethodDescriptor;
    let mut manifest = ContractManifest::new(name.to_string());
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "main".to_string(),
            vec![],
            ContractParameterType::Void,
            0,
            true,
        )
        .expect("method descriptor"),
    );
    manifest
}

#[test]
fn next_available_id_requires_initialized_storage_then_increments() {
    // C# GetNextAvailableId: return the stored value, write value + 1.
    let cache = DataCache::new(false);
    let err = ContractManagement::new()
        .get_next_available_id(&cache)
        .expect_err("missing next available id storage should fault");
    assert!(err.to_string().contains("NextAvailableId"), "{err}");

    cache.add(
        ContractManagement::next_available_id_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_NEXT_AVAILABLE_ID,
        ))),
    );
    assert_eq!(
        ContractManagement::new()
            .get_next_available_id(&cache)
            .unwrap(),
        1
    );
    assert_eq!(
        ContractManagement::new()
            .get_next_available_id(&cache)
            .unwrap(),
        2
    );
    assert_eq!(
        storage_key_int(&cache, ContractManagement::next_available_id_key()),
        Some(BigInt::from(3))
    );
}

#[test]
fn check_script_against_abi_validates_offsets_and_uniqueness() {
    use neo_manifest::{ContractEventDescriptor, ContractMethodDescriptor};
    let method = |name: &str, offset: i32| {
        ContractMethodDescriptor::new(
            name.to_string(),
            vec![],
            ContractParameterType::Void,
            offset,
            true,
        )
        .unwrap()
    };
    let ret_script = vec![neo_vm_rs::OpCode::RET.byte()];

    // A method at offset 0 (RET) passes in both strict and lazy modes.
    let abi = ContractAbi::new(vec![method("main", 0)], vec![]);
    assert!(ContractManagement::check_script_against_abi(&ret_script, &abi, true).is_ok());
    assert!(ContractManagement::check_script_against_abi(&ret_script, &abi, false).is_ok());

    // An out-of-range offset fails in both modes (C# `ip >= Length`).
    let abi_oob = ContractAbi::new(vec![method("main", 9)], vec![]);
    assert!(ContractManagement::check_script_against_abi(&ret_script, &abi_oob, true).is_err());
    assert!(ContractManagement::check_script_against_abi(&ret_script, &abi_oob, false).is_err());

    // PUSHDATA1 [len 1] [0x40]: offset 2 sits INSIDE the operand. The
    // strict (post-Basilisk) Script rejects non-boundary offsets, while the
    // pre-Basilisk lazy Script parses the byte at 2 as RET and accepts —
    // the exact C# Script strict-mode divergence.
    let pushdata = vec![0x0C, 0x01, 0x40];
    let abi_mid = ContractAbi::new(vec![method("main", 2)], vec![]);
    assert!(ContractManagement::check_script_against_abi(&pushdata, &abi_mid, true).is_err());
    assert!(ContractManagement::check_script_against_abi(&pushdata, &abi_mid, false).is_ok());

    // Duplicate (name, pcount) pairs fail (C# methodDictionary build).
    let abi_dup = ContractAbi::new(vec![method("main", 0), method("main", 0)], vec![]);
    assert!(ContractManagement::check_script_against_abi(&ret_script, &abi_dup, true).is_err());

    // Duplicate event names fail (C# events ToDictionary).
    let abi_dup_events = ContractAbi::new(
        vec![method("main", 0)],
        vec![
            ContractEventDescriptor::new("Changed".to_string(), vec![]).unwrap(),
            ContractEventDescriptor::new("Changed".to_string(), vec![]).unwrap(),
        ],
    );
    assert!(
        ContractManagement::check_script_against_abi(&ret_script, &abi_dup_events, true).is_err()
    );
}

#[test]
fn manifest_is_valid_checks_serialization_and_group_signatures() {
    use neo_crypto::ECPoint;
    use neo_manifest::ContractGroup;
    let limits = ExecutionEngineLimits::default();
    let hash = UInt160::from_bytes(&[0x21u8; 20]).unwrap();

    // No groups: valid (the stack-item projection serializes within limits).
    assert!(ContractManagement::manifest_is_valid(
        &deployable_manifest("Valid"),
        &limits,
        &hash
    ));

    // A group whose signature does not verify against the contract hash
    // makes the manifest invalid (C# Groups.All(u => u.IsValid(hash))).
    let pub_key = ECPoint::from_bytes(
        &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap(),
    )
    .unwrap();
    let mut bad = deployable_manifest("Valid");
    bad.groups.push(ContractGroup::new(pub_key, vec![0xAB; 64]));
    assert!(!ContractManagement::manifest_is_valid(&bad, &limits, &hash));
}

#[test]
fn parse_manifest_checked_enforces_csharp_parse_gates() {
    // Empty payload (C# "Manifest length cannot be zero").
    assert!(ContractManagement::parse_manifest_checked(&[], "deploy").is_err());
    // Over the u16::MAX byte cap of C# ContractManifest.Parse.
    let oversized = vec![b' '; MAX_MANIFEST_LENGTH + 1];
    assert!(ContractManagement::parse_manifest_checked(&oversized, "deploy").is_err());
    // Not UTF-8 / not JSON.
    assert!(ContractManagement::parse_manifest_checked(&[0xFF, 0xFE], "deploy").is_err());
    // Structurally invalid: an empty ABI (C# ContractAbi.FromJson throws).
    let empty_abi = ContractManifest::new("NoMethods".to_string())
        .to_json()
        .unwrap()
        .to_string()
        .into_bytes();
    assert!(ContractManagement::parse_manifest_checked(&empty_abi, "deploy").is_err());
    // A valid manifest parses and keeps its name + ABI.
    let bytes = deployable_manifest("RoundTrip")
        .to_json()
        .unwrap()
        .to_string()
        .into_bytes();
    let parsed = ContractManagement::parse_manifest_checked(&bytes, "deploy").unwrap();
    assert_eq!(parsed.name, "RoundTrip");
    assert_eq!(parsed.abi.methods.len(), 1);
}

#[test]
fn parse_nef_checked_validates_container_and_checksum() {
    // Empty payload (C# "NEF file length cannot be zero").
    assert!(ContractManagement::parse_nef_checked(&[], "deploy").is_err());
    // A valid NEF3 container round-trips.
    let nef = NefFile::new("unit-test".to_string(), vec![neo_vm_rs::OpCode::RET.byte()]);
    let bytes = nef.to_bytes();
    let parsed = ContractManagement::parse_nef_checked(&bytes, "deploy").unwrap();
    assert_eq!(parsed.checksum, nef.checksum);
    assert_eq!(parsed.script, vec![neo_vm_rs::OpCode::RET.byte()]);
    // Corrupting the trailing checksum fails the parse (the C#
    // AsSerializable<NefFile> checksum verifier).
    let mut corrupted = bytes;
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    assert!(ContractManagement::parse_nef_checked(&corrupted, "deploy").is_err());
}
