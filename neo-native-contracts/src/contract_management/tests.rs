//! Tests for the ContractManagement native contract.
//!
//! Extracted from `contract_management.rs` to keep the production module
//! focused. The `use super::*;` below re-exports the production items so
//! the inner test modules' own `use super::*;` resolves to them.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
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
    fn clean_whitelist_storage_decode_uses_stack_value_projection() {
        let source = include_str!("mod.rs");
        let start = source
            .find("fn policy_clean_whitelist")
            .expect("policy_clean_whitelist exists");
        let end = source[start..]
            .find("fn read_required_i64_setting")
            .map(|offset| start + offset)
            .expect("following helper exists");
        let helper = &source[start..end];

        assert!(helper.contains("deserialize_stack_value_with_limits"));
        assert!(helper.contains("StackValue::Struct"));
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
        let k1 = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 1);
        let k2 = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 2);
        cache.add(k1, StorageItem::from_bytes(vec![0xAA; 20]));
        cache.add(
            k2,
            StorageItem::from_bytes(vec![0xBB; 20]),
        );
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
            let key = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, id);
            cache.add(key, StorageItem::from_bytes(vec![0xCC; 20]));
        }
        let user = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 1);
        cache.add(user, StorageItem::from_bytes(vec![0xDD; 20]));

        let entries = ContractManagement::new().contract_hash_entries(&cache);
        assert_eq!(entries.len(), 1, "native (negative-id) entries are skipped");
        assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xDD; 20]);
        // id 0 is the boundary: C# keeps `Id >= 0`.
        let zero = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 0);
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
    fn is_native_contract_hash_covers_all_eleven_natives() {
        for spec in crate::standard_native_contract_specs() {
            assert!(
                ContractManagement::is_native_contract_hash(&spec.hash),
                "{} is native",
                spec.name
            );
        }
        let user = UInt160::from_bytes(&[0x99u8; 20]).unwrap();
        assert!(!ContractManagement::is_native_contract_hash(&user));
    }

    #[test]
    fn policy_blocked_account_key_matches_policy_layout() {
        // The cross-native blocked-account key must match PolicyContract's own
        // layout: (PolicyContract.ID, [Prefix_BlockedAccount(15), account]).
        let account = UInt160::from_bytes(&[0x77u8; 20]).unwrap();
        let key = crate::PolicyContract::blocked_account_key(&account);
        assert_eq!(key.id, crate::PolicyContract::ID);
        assert_eq!(key.suffix()[0], POLICY_PREFIX_BLOCKED_ACCOUNT);
        assert_eq!(&key.suffix()[1..], account.to_bytes().as_slice());
    }

    #[test]
    fn set_minimum_deployment_fee_write_round_trips() {
        // The setter's storage effect (overwrite Prefix_MinimumDeploymentFee) is
        // observed by the getMinimumDeploymentFee reader, matching C#
        // GetAndChange(...).Set(value).
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        // Zero is permitted (C# rejects only value < 0).
        ContractManagement::new()
            .put_minimum_deployment_fee(&cache, &BigInt::from(0))
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            0
        );
        // Overwrite with a positive fee (GetAndChange semantics).
        ContractManagement::new()
            .put_minimum_deployment_fee(&cache, &BigInt::from(25_00000000i64))
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            25_00000000
        );
    }

    #[test]
    fn abi_has_method_matches_name_and_pcount() {
        use neo_manifest::{
            ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
        };
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

        let source = include_str!("mod.rs");
        let start = source
            .find("fn contract_state_to_bytes")
            .expect("contract_state_to_bytes helper exists");
        let end = source[start..]
            .find("fn contract_hash_entries")
            .map(|offset| start + offset)
            .expect("contract_hash_entries follows contract_state_to_bytes");
        let helper = &source[start..end];

        assert!(helper.contains("to_stack_value"));
        assert!(helper.contains("serialize_stack_value_default"));
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
            StorageKey::create(ContractManagement::ID, PREFIX_NEXT_AVAILABLE_ID),
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
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_NEXT_AVAILABLE_ID,
                DEFAULT_NEXT_AVAILABLE_ID
            )
            .unwrap(),
            3
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
        assert!(
            ContractManagement::check_script_against_abi(&ret_script, &abi_oob, false).is_err()
        );

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
            ContractManagement::check_script_against_abi(&ret_script, &abi_dup_events, true)
                .is_err()
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
            &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .unwrap(),
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
}

/// Engine-level tests for `destroy` and its `Policy.BlockAccountInternal` /
/// `Policy.CleanWhitelist` ports, using the witness-gated script-execution
/// harness proven in `neo_token::governance_writer_tests`.
#[cfg(test)]
mod destroy_engine_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_io::BinaryWriter;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// Writes a serialized contract record under `Prefix_Contract ++ hash`.
    fn put_contract_record(cache: &DataCache, state: &ContractState) {
        cache.add(
            ContractManagement::contract_storage_key(&state.hash),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );
    }

    /// Builds the entry script `System.Contract.Call(CM, "destroy", [])`.
    fn destroy_script() -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("destroy".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    fn engine_for(
        snapshot: Arc<DataCache>,
        persisting_block: Option<Block>,
        settings: ProtocolSettings,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            persisting_block,
            settings,
            100_00000000,
            None,
        )
        .expect("engine builds")
    }

    #[test]
    fn destroy_removes_record_index_storage_and_blocks_hash() {
        crate::install();
        let cache = DataCache::new(false);
        // Seed the ContractManagement native record so System.Contract.Call
        // resolves the callee.
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // The entry script IS the calling contract: pin its hash, then deploy
        // a user contract under that hash (record + id index + one storage
        // row + one Policy whitelist entry).
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let user = ContractState::new_native(7, self_hash, "SelfDestructFixture".to_string());
        put_contract_record(&cache, &user);
        let index_key = ContractManagement::contract_id_storage_key(7);
        cache.add(
            index_key.clone(),
            StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
        );
        let user_row = StorageKey::new(7, vec![0x01]);
        cache.add(user_row.clone(), StorageItem::from_bytes(vec![0xEE]));
        // A whitelist entry for the contract (C# WhitelistedContract
        // Struct[ContractHash, Method, ArgCount, FixedFee]) that CleanWhitelist
        // must remove and report.
        // Layout: [PREFIX, self_hash160, 0i32_be].
        let mut wl_suffix = Vec::with_capacity(20 + 4);
        wl_suffix.extend_from_slice(&self_hash.to_bytes());
        wl_suffix.extend_from_slice(&0i32.to_be_bytes());
        let wl_key = StorageKey::create_with_bytes(
            crate::PolicyContract::ID,
            POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS,
            &wl_suffix,
        );
        let wl_value = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_byte_string(self_hash.to_bytes()),
                StackItem::from_byte_string("transfer".as_bytes().to_vec()),
                StackItem::from_int(4),
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(wl_key.clone(), StorageItem::from_bytes(wl_value));
        let snapshot = Arc::new(cache);

        // Default MainNet schedules Faun at 8,800,000, so height 0 runs the
        // pre-Faun BlockAccountInternal branch (empty blocked value).
        // The destroy path reads the persisting block's timestamp, so the
        // engine needs a persisting block fixture (height 0, pre-Faun).
        let mut persisting_header = BlockHeader::default();
        persisting_header.set_index(0);
        persisting_header.set_timestamp(1_700_000_000_000);
        let persisting_block = Some(Block::from_parts(persisting_header, vec![]));
        let mut engine =
            engine_for(Arc::clone(&snapshot), persisting_block, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "destroy must HALT"
        );

        // The contract record, id index, and contract storage are gone.
        assert!(
            snapshot
                .get(&ContractManagement::contract_storage_key(&self_hash))
                .is_none(),
            "contract record deleted"
        );
        assert!(
            snapshot.get(&index_key).is_none(),
            "id->hash index entry deleted"
        );
        assert!(
            snapshot.get(&user_row).is_none(),
            "contract storage deleted"
        );
        // The destroyed hash is locked via Policy's blocked-account entry,
        // pre-Faun with an EMPTY value (C# StorageItem([])).
        let blocked = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&self_hash))
            .expect("destroyed contract is blocked");
        assert!(
            blocked.value_bytes().is_empty(),
            "pre-Faun blocked value is empty"
        );
        // The whitelist entry was cleaned.
        assert!(snapshot.get(&wl_key).is_none(), "whitelist entry deleted");

        // Events: Policy's WhitelistFeeChanged for the cleaned entry, then
        // ContractManagement's Destroy with the destroyed hash.
        let notifications = engine.notifications();
        let destroy_event = notifications
            .iter()
            .find(|n| n.event_name == "Destroy")
            .expect("Destroy event emitted");
        assert_eq!(destroy_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            destroy_event.state[0].as_bytes().unwrap(),
            self_hash.to_bytes().to_vec()
        );
        let wl_event = notifications
            .iter()
            .find(|n| n.event_name == "WhitelistFeeChanged")
            .expect("WhitelistFeeChanged event emitted");
        assert_eq!(wl_event.script_hash, crate::PolicyContract::script_hash());
        assert_eq!(wl_event.state[1].as_bytes().unwrap(), b"transfer".to_vec());
        assert_eq!(wl_event.state[2].as_int().unwrap(), BigInt::from(4));
        assert!(matches!(wl_event.state[3], StackItem::Null));
    }

    #[test]
    fn destroy_is_a_noop_for_a_non_contract_caller() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let snapshot = Arc::new(cache);

        // No contract record for the calling script: C# `if (contract is null)
        // return;` — a successful no-op that writes nothing.
        let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "no-op destroy HALTs"
        );
        assert!(
            snapshot
                .get(&crate::PolicyContract::blocked_account_key(&self_hash))
                .is_none(),
            "no blocked-account entry for a no-op destroy"
        );
        assert!(
            engine
                .notifications()
                .iter()
                .all(|n| n.event_name != "Destroy"),
            "no Destroy event for a no-op destroy"
        );
    }

    #[test]
    fn block_account_internal_faun_writes_timestamp_and_is_idempotent() {
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_123_456);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        let account = UInt160::from_bytes(&[0x33u8; 20]).unwrap();
        // First block: post-Faun the entry stores GetTime() (the persisting
        // block's timestamp) for Policy's recoverFund.
        assert!(
            crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
        let item = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_123_456i64)
        );
        // Already blocked -> false, nothing rewritten (C# returns early).
        assert!(
            !crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
    }

    #[test]
    fn block_account_internal_rejects_native_hashes() {
        crate::install();
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        // C#: "Cannot block a native contract."
        let neo_hash = *crate::hashes::NEO_TOKEN_HASH;
        let err = crate::PolicyContract::new()
            .block_account_internal(&mut engine, &neo_hash)
            .unwrap_err();
        assert!(err.to_string().contains("native"));
        assert!(
            snapshot
                .get(&crate::PolicyContract::blocked_account_key(&neo_hash))
                .is_none()
        );
    }

    #[test]
    fn block_account_internal_faun_runs_vote_transition_for_neo_holders() {
        // C# BlockAccountInternal post-Faun runs NEO.VoteInternal(account,
        // null): for a NEO-holding account the full vote transition executes
        // (here a no-op un-vote — the account votes for nobody), then the
        // blocked entry is written with the persisting block's timestamp.
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_000_000);
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[0x44u8; 20]).unwrap();
        // Seed a NeoToken account state holding 100 NEO.
        let neo_key =
            StorageKey::create_with_uint160(crate::NeoToken::ID, crate::NEP17_PREFIX_ACCOUNT, &account);
        let neo_state = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_int(100),
                StackItem::from_int(0),
                StackItem::Null,
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(neo_key, StorageItem::from_bytes(neo_state));
        let snapshot = Arc::new(cache);
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        assert!(
            crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
        let item = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written after the vote transition");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_000_000i64),
            "entry stores GetTime() for recoverFund"
        );
    }
}

/// Engine-level tests for `deploy` / `update`, using the witness-gated
/// script-execution harness proven in `neo_token::governance_writer_tests`:
/// the entry script does `System.Contract.Call(CM, method, args)` against a
/// snapshot seeded with the ContractManagement native record.
#[cfg(test)]
mod deploy_update_engine_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition};
    use neo_payloads::signer::Signer;
    use neo_payloads::witness::Witness;
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::{OpCode, VmState};
    use std::sync::Arc;

    /// The deploying transaction's sender (first signer).
    const SENDER: [u8; 20] = [0x07; 20];

    /// Writes a serialized contract record under `Prefix_Contract ++ hash`.
    fn put_contract_record(cache: &DataCache, state: &ContractState) {
        cache.add(
            ContractManagement::contract_storage_key(&state.hash),
            StorageItem::from_bytes(
                ContractManagement::serialize_contract_record(state).expect("record bytes"),
            ),
        );
    }

    fn seed_contract_management_settings(cache: &DataCache) {
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_NEXT_AVAILABLE_ID),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
    }

    /// Snapshot seeded with the ContractManagement native record so
    /// `System.Contract.Call` resolves the callee.
    fn seeded_snapshot() -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        seed_contract_management_settings(&cache);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        Arc::new(cache)
    }

    fn faun_from_genesis_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        settings
    }

    /// The smallest NEF that parses: a single RET at offset 0.
    fn minimal_nef() -> NefFile {
        NefFile::new("e2e-test".to_string(), vec![OpCode::RET.byte()])
    }

    /// A minimal deployable manifest: `main()` at offset 0.
    fn deployable_manifest(name: &str) -> ContractManifest {
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

    /// JSON payload for a manifest (what a deploying transaction carries).
    fn manifest_json(manifest: &ContractManifest) -> Vec<u8> {
        manifest
            .to_json()
            .expect("manifest json")
            .to_string()
            .into_bytes()
    }

    fn engine_for(
        snapshot: Arc<DataCache>,
        settings: ProtocolSettings,
        sender: UInt160,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            settings,
            1000_00000000, // covers the 10-GAS minimum deployment fee
            None,
        )
        .expect("engine builds")
    }

    /// Runs `System.Contract.Call(CM, "deploy", [nef, manifest(, data)])` and
    /// returns the final VM state plus the engine (for fee / notification /
    /// result-stack assertions).
    fn run_deploy(
        snapshot: &Arc<DataCache>,
        settings: ProtocolSettings,
        sender: UInt160,
        nef_bytes: &[u8],
        manifest_bytes: &[u8],
        data: Option<&[u8]>,
        flags: CallFlags,
    ) -> (VmState, ApplicationEngine) {
        let mut builder = ScriptBuilder::new();
        // Args are pushed deepest-first (argN-1 .. arg0) before PACK.
        let argc = if let Some(data) = data {
            builder.emit_push(data);
            3
        } else {
            2
        };
        builder.emit_push(manifest_bytes);
        builder.emit_push(nef_bytes);
        builder.emit_push_int(argc);
        builder.emit_pack();
        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push("deploy".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = engine_for(Arc::clone(snapshot), settings, sender);
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    /// Builds the self-update entry script
    /// `System.Contract.Call(CM, "update", [nef?, manifest?])`; `None` pushes
    /// the C# `null` argument.
    fn update_script(
        nef_bytes: Option<&[u8]>,
        manifest_bytes: Option<&[u8]>,
        flags: CallFlags,
    ) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        // arg1 (manifest) deepest, then arg0 (nef) on top, then PACK 2.
        match manifest_bytes {
            Some(bytes) => {
                builder.emit_push(bytes);
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL);
            }
        }
        match nef_bytes {
            Some(bytes) => {
                builder.emit_push(bytes);
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL);
            }
        }
        builder.emit_push_int(2);
        builder.emit_pack();
        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push("update".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    /// Runs a self-update entry script whose pinned hash is `self_hash`.
    fn run_update(
        snapshot: &Arc<DataCache>,
        script: Vec<u8>,
        self_hash: UInt160,
    ) -> (VmState, ApplicationEngine) {
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let mut engine = engine_for(Arc::clone(snapshot), ProtocolSettings::default(), sender);
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    #[test]
    fn deploy_writes_record_and_index_charges_fee_and_notifies() {
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest = deployable_manifest("DeployFixture");

        let (state, engine) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::HALT, "deploy must HALT");

        // The record lands at GetContractHash(sender, nef.CheckSum, name) and
        // round-trips through the shared reader.
        let expected_hash = Helper::get_contract_hash(&sender, nef.checksum, "DeployFixture");
        let deployed = ContractManagement::get_contract_from_snapshot(&snapshot, &expected_hash)
            .unwrap()
            .expect("deployed record exists");
        assert_eq!(
            deployed.id, 1,
            "first user contract takes the genesis next-id"
        );
        assert_eq!(deployed.update_counter, 0);
        assert_eq!(deployed.hash, expected_hash);
        assert_eq!(deployed.nef.checksum, nef.checksum);
        assert_eq!(deployed.manifest.name, "DeployFixture");

        // The big-endian id -> hash index entry.
        let index = snapshot
            .get(&ContractManagement::contract_id_storage_key(1))
            .expect("id index entry written");
        assert_eq!(
            index.value_bytes().to_vec(),
            expected_hash.to_bytes().to_vec()
        );
        // The next-available-id counter advanced to 2.
        assert_eq!(
            crate::read_storage_int(
                &snapshot,
                ContractManagement::ID,
                PREFIX_NEXT_AVAILABLE_ID,
                DEFAULT_NEXT_AVAILABLE_ID
            )
            .unwrap(),
            2
        );

        // The 10-GAS minimum deployment fee dominates this tiny payload and
        // was charged (C# AddFee(max(StoragePrice * size, MinimumFee))).
        assert!(
            engine.fee_consumed() >= DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            "deployment fee charged: {}",
            engine.fee_consumed()
        );

        // The Deploy notification carries the new hash.
        let notifications = engine.notifications();
        let deploy_event = notifications
            .iter()
            .find(|n| n.event_name == "Deploy")
            .expect("Deploy event emitted");
        assert_eq!(deploy_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            deploy_event.state[0].as_bytes().unwrap(),
            expected_hash.to_bytes().to_vec()
        );

        // deploy returns the new ContractState as the 5-field Array.
        let result = engine.result_stack().peek(0).expect("deploy result");
        let StackItem::Array(items) = result else {
            panic!("deploy must return an Array, got {result:?}");
        };
        assert_eq!(items.items().len(), 5);
        assert_eq!(
            items.items()[2].as_bytes().unwrap(),
            expected_hash.to_bytes().to_vec(),
            "field 2 is the contract hash"
        );
    }

    #[test]
    fn deploy_hash_is_deterministic_and_duplicates_fault() {
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest = deployable_manifest("DeterministicFixture");
        let manifest_bytes = manifest_json(&manifest);

        let (first, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(first, VmState::HALT);

        // Same sender + NEF checksum + name -> the same hash, so the second
        // deploy hits "Contract Already Exists" and faults.
        let (duplicate, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(duplicate, VmState::FAULT, "duplicate deploy must fault");

        // A different manifest NAME moves the hash: deploys fresh with id 2.
        let renamed = deployable_manifest("DeterministicFixtureB");
        let (second, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&renamed),
            None,
            CallFlags::ALL,
        );
        assert_eq!(second, VmState::HALT);
        let hash_a = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixture");
        let hash_b = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixtureB");
        assert_ne!(hash_a, hash_b);
        let second_state = ContractManagement::get_contract_from_snapshot(&snapshot, &hash_b)
            .unwrap()
            .expect("second contract deployed");
        assert_eq!(second_state.id, 2, "ids allocate sequentially");
    }

    #[test]
    fn deploy_runs_the_declared_deploy_callback_with_data() {
        // The contract script: `main()` = RET at 0; `_deploy(data, update)` at
        // `deploy_offset` stores [0xEE] under key [0x77] in the contract's own
        // storage — observable proof the queued callback executed.
        let mut script = ScriptBuilder::new();
        script.emit_opcode(OpCode::RET);
        let deploy_offset = script.len() as i32;
        script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
        script.emit_push(&[0xEE]); // value (deepest)
        script.emit_push(&[0x77]); // key
        script
            .emit_syscall("System.Storage.GetContext")
            .expect("GetContext");
        script.emit_syscall("System.Storage.Put").expect("Put");
        script.emit_opcode(OpCode::RET);
        let nef = NefFile::new("e2e-test".to_string(), script.to_array());

        let mut manifest = deployable_manifest("CallbackFixture");
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "_deploy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "data".to_string(),
                        ContractParameterType::Any,
                    )
                    .unwrap(),
                    ContractParameterDefinition::new(
                        "update".to_string(),
                        ContractParameterType::Boolean,
                    )
                    .unwrap(),
                ],
                ContractParameterType::Void,
                deploy_offset,
                false,
            )
            .expect("_deploy descriptor"),
        );

        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            Some(&[0xAB]), // deploy(nef, manifest, data) overload
            CallFlags::ALL,
        );
        assert_eq!(
            state,
            VmState::HALT,
            "deploy with _deploy callback must HALT"
        );

        // The callback wrote into the new contract's storage space (id 1).
        let row = snapshot
            .get(&StorageKey::new(1, vec![0x77]))
            .expect("_deploy callback wrote the marker row");
        assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
    }

    #[test]
    fn deploy_callback_local_storage_syscalls_use_csharp_parameter_order() {
        // HF_Faun local storage syscalls follow the same reflection binder order
        // as C#: parameter 0 is on top of the stack. Local.Put(key, value) must
        // pop key before value; Local.Find(prefix, options) must pop prefix
        // before options.
        let mut script = ScriptBuilder::new();
        script.emit_opcode(OpCode::RET);
        let deploy_offset = script.len() as i32;
        script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
        script.emit_push(&[0xEE]); // value (deeper)
        script.emit_push(&[0x77]); // key (top)
        script
            .emit_syscall("System.Storage.Local.Put")
            .expect("Local.Put");
        script.emit_push_int(0); // options (deeper)
        script.emit_push(&[0x77]); // prefix (top)
        script
            .emit_syscall("System.Storage.Local.Find")
            .expect("Local.Find");
        script.emit_opcode(OpCode::DROP);
        script.emit_opcode(OpCode::RET);
        let nef = NefFile::new("e2e-test".to_string(), script.to_array());

        let mut manifest = deployable_manifest("LocalStorageCallbackFixture");
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "_deploy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "data".to_string(),
                        ContractParameterType::Any,
                    )
                    .unwrap(),
                    ContractParameterDefinition::new(
                        "update".to_string(),
                        ContractParameterType::Boolean,
                    )
                    .unwrap(),
                ],
                ContractParameterType::Void,
                deploy_offset,
                false,
            )
            .expect("_deploy descriptor"),
        );

        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, _) = run_deploy(
            &snapshot,
            faun_from_genesis_settings(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            Some(&[0xAB]),
            CallFlags::ALL,
        );
        assert_eq!(
            state,
            VmState::HALT,
            "local storage callback must follow C# syscall parameter order"
        );

        let row = snapshot
            .get(&StorageKey::new(1, vec![0x77]))
            .expect("Local.Put wrote under the key argument");
        assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
        assert!(
            snapshot.get(&StorageKey::new(1, vec![0xEE])).is_none(),
            "Local.Put must not swap key and value"
        );
    }

    #[test]
    fn deploy_skips_the_callback_when_not_declared() {
        // The minimal fixture declares no `_deploy`: C# OnDeployAsync skips
        // the call (md is null) but still emits Deploy. Nothing is written
        // into the new contract's storage space.
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, engine) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &minimal_nef().to_bytes(),
            &manifest_json(&deployable_manifest("NoCallback")),
            Some(&[0xAB]),
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::HALT);
        assert!(
            engine
                .notifications()
                .iter()
                .any(|n| n.event_name == "Deploy")
        );
        let contract_rows: Vec<_> = snapshot
            .find(
                Some(&StorageKey::new(1, Vec::new())),
                SeekDirection::Forward,
            )
            .collect();
        assert!(
            contract_rows.is_empty(),
            "no _deploy, no contract storage writes"
        );
    }

    #[test]
    fn deploy_validation_failures_fault() {
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest_bytes = manifest_json(&deployable_manifest("FaultFixture"));

        // Empty NEF payload.
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &[],
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "empty NEF must fault");

        // Empty manifest payload.
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &[],
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "empty manifest must fault");

        // A corrupted NEF checksum.
        let mut corrupted = nef.to_bytes();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xFF;
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &corrupted,
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "bad NEF checksum must fault");

        // The target hash is Policy-blocked (C# "has been blocked").
        let snapshot = seeded_snapshot();
        let blocked_hash = Helper::get_contract_hash(&sender, nef.checksum, "FaultFixture");
        snapshot.add(
            crate::PolicyContract::blocked_account_key(&blocked_hash),
            StorageItem::from_bytes(Vec::new()),
        );
        let (state, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "blocked target hash must fault");
        assert!(
            ContractManagement::get_contract_from_snapshot(&snapshot, &blocked_hash)
                .unwrap()
                .is_none(),
            "no record written for a blocked deploy"
        );
    }

    #[test]
    fn deploy_post_aspidochelone_requires_call_flags_all() {
        // Schedule HF_Aspidochelone from genesis: a deploy carrying only
        // States|AllowNotify (the method's minimum) must fault, while
        // CallFlags.All succeeds (C# refs #2653 / #2673).
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfAspidochelone, 0);
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest_bytes = manifest_json(&deployable_manifest("AspidoFixture"));

        let (restricted, _) = run_deploy(
            &seeded_snapshot(),
            settings.clone(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::STATES | CallFlags::ALLOW_NOTIFY,
        );
        assert_eq!(
            restricted,
            VmState::FAULT,
            "partial flags must fault post-Aspidochelone"
        );

        let (full, _) = run_deploy(
            &seeded_snapshot(),
            settings,
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(
            full,
            VmState::HALT,
            "CallFlags.All deploy succeeds post-Aspidochelone"
        );
    }

    #[test]
    fn update_bumps_counter_swaps_payloads_and_notifies() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // The entry script IS the updating contract: pin its hash and seed its
        // record (id 7) plus the id index entry.
        let new_nef = NefFile::new("updated-compiler".to_string(), vec![OpCode::RET.byte()]);
        let new_manifest = deployable_manifest("SelfUpdateFixture");
        let script = update_script(
            Some(&new_nef.to_bytes()),
            Some(&manifest_json(&new_manifest)),
            CallFlags::ALL,
        );
        let self_hash = UInt160::from_script(&script);
        let fixture = ContractState::new(
            7,
            self_hash,
            minimal_nef(),
            deployable_manifest("SelfUpdateFixture"),
        );
        put_contract_record(&cache, &fixture);
        let index_key = ContractManagement::contract_id_storage_key(7);
        cache.add(
            index_key.clone(),
            StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
        );
        let snapshot = Arc::new(cache);

        let (state, engine) = run_update(&snapshot, script, self_hash);
        assert_eq!(state, VmState::HALT, "update must HALT");

        // Same id + hash, UpdateCounter bumped, NEF and manifest swapped.
        let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
            .unwrap()
            .expect("updated record exists");
        assert_eq!(updated.id, 7, "id is preserved");
        assert_eq!(updated.hash, self_hash, "hash is preserved");
        assert_eq!(updated.update_counter, 1, "UpdateCounter bumped");
        assert_eq!(updated.nef.compiler, "updated-compiler");
        assert_eq!(updated.nef.checksum, new_nef.checksum);
        assert_eq!(updated.manifest.name, "SelfUpdateFixture");
        // The id index entry is untouched.
        assert_eq!(
            snapshot
                .get(&index_key)
                .expect("index intact")
                .value_bytes()
                .to_vec(),
            self_hash.to_bytes().to_vec()
        );

        // The storage fee on the payload was charged (no minimum-fee floor).
        let payload_len = (new_nef.to_bytes().len() + manifest_json(&new_manifest).len()) as i64;
        assert!(engine.fee_consumed() >= i64::from(engine.storage_price()) * payload_len);

        // The Update notification carries the contract hash.
        let notifications = engine.notifications();
        let update_event = notifications
            .iter()
            .find(|n| n.event_name == "Update")
            .expect("Update event emitted");
        assert_eq!(update_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            update_event.state[0].as_bytes().unwrap(),
            self_hash.to_bytes().to_vec()
        );
    }

    #[test]
    fn update_with_null_nef_keeps_the_old_nef() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // update(null, manifest): only the manifest changes (one extra
        // supported standard); the NEF stays byte-identical.
        let mut new_manifest = deployable_manifest("NullNefFixture");
        new_manifest.supported_standards = vec!["NEP-17".to_string()];
        let script = update_script(None, Some(&manifest_json(&new_manifest)), CallFlags::ALL);
        let self_hash = UInt160::from_script(&script);
        let original_nef = minimal_nef();
        let fixture = ContractState::new(
            3,
            self_hash,
            original_nef.clone(),
            deployable_manifest("NullNefFixture"),
        );
        put_contract_record(&cache, &fixture);
        let snapshot = Arc::new(cache);

        let (state, _) = run_update(&snapshot, script, self_hash);
        assert_eq!(state, VmState::HALT, "manifest-only update must HALT");
        let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
            .unwrap()
            .expect("record exists");
        assert_eq!(updated.update_counter, 1);
        assert_eq!(updated.nef.checksum, original_nef.checksum, "NEF unchanged");
        assert_eq!(updated.nef.compiler, original_nef.compiler);
        assert_eq!(
            updated.manifest.supported_standards,
            vec!["NEP-17".to_string()]
        );
    }

    #[test]
    fn update_validation_failures_fault() {
        crate::install();

        // Both args null.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(None, None, CallFlags::ALL);
            let self_hash = UInt160::from_script(&script);
            put_contract_record(
                &cache,
                &ContractState::new(4, self_hash, minimal_nef(), deployable_manifest("BothNull")),
            );
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "null nef + null manifest must fault");
        }

        // The caller has no contract record.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                Some(&minimal_nef().to_bytes()),
                Some(&manifest_json(&deployable_manifest("NoRecord"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "non-contract caller must fault");
        }

        // The manifest name cannot change.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                None,
                Some(&manifest_json(&deployable_manifest("RenamedFixture"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            put_contract_record(
                &cache,
                &ContractState::new(
                    5,
                    self_hash,
                    minimal_nef(),
                    deployable_manifest("OriginalFixture"),
                ),
            );
            let snapshot = Arc::new(cache);
            let (state, _) = run_update(&snapshot, script, self_hash);
            assert_eq!(state, VmState::FAULT, "renaming must fault");
            // The seeded record is untouched (the name check precedes writes).
            let unchanged = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
                .unwrap()
                .expect("record still present");
            assert_eq!(unchanged.manifest.name, "OriginalFixture");
            assert_eq!(unchanged.update_counter, 0);
        }

        // The update counter is saturated at u16::MAX.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                Some(&minimal_nef().to_bytes()),
                Some(&manifest_json(&deployable_manifest("MaxedFixture"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            let mut fixture = ContractState::new(
                6,
                self_hash,
                minimal_nef(),
                deployable_manifest("MaxedFixture"),
            );
            fixture.update_counter = u16::MAX;
            put_contract_record(&cache, &fixture);
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "maxed update counter must fault");
        }
    }
}

/// `ContractManagement::initialize` / `ContractManagement::on_persist` against
/// the C# oracle (ContractManagement.cs:53-118): the genesis counter seeds, the
/// native deployment records + `Deploy` notifications, the hardfork manifest
/// refresh (`Update`), and the hardfork-parameterized re-initializations.
#[cfg(test)]
mod persist_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use neo_config::ProtocolSettings;
    use neo_payloads::{Block, Header};
    use neo_primitives::TriggerType;

    /// C# `PolicyContract.Prefix_ExecFeeFactor`.
    const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    /// C# `PolicyContract.Prefix_BlockedAccount`.
    const POLICY_PREFIX_BLOCKED_ACCOUNT: u8 = 15;
    /// C# `PolicyContract.Prefix_AttributeFee`.
    const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
    /// C# `Notary.Prefix_MaxNotValidBeforeDelta`.
    const NOTARY_PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;

    fn settings_with(hardforks: &[(Hardfork, u32)]) -> ProtocolSettings {
        ProtocolSettings {
            hardforks: hardforks.iter().copied().collect::<HashMap<_, _>>(),
            ..ProtocolSettings::default()
        }
    }

    fn on_persist_engine(
        snapshot: &Arc<DataCache>,
        settings: &ProtocolSettings,
        index: u32,
        timestamp: u64,
    ) -> ApplicationEngine {
        let mut header = Header::new();
        header.set_index(index);
        header.set_timestamp(timestamp);
        let block = Block::from_parts(header, Vec::new());
        ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(snapshot),
            Some(block),
            settings.clone(),
            0,
            None,
        )
        .expect("engine builds")
    }

    fn storage_int(snapshot: &DataCache, id: i32, key: Vec<u8>) -> Option<BigInt> {
        snapshot
            .get(&StorageKey::new(id, key))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
    }

    /// C# `ContractManagement.InitializeAsync` (ContractManagement.cs:53-61):
    /// genesis seeds MinimumDeploymentFee = 10 GAS and NextAvailableId = 1.
    #[test]
    fn initialize_seeds_deployment_fee_and_next_id() {
        let settings = settings_with(&[]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::initialize(&ContractManagement::new(), &mut engine).expect("initialize");

        assert_eq!(
            storage_int(
                &snapshot,
                ContractManagement::ID,
                vec![PREFIX_MINIMUM_DEPLOYMENT_FEE],
            ),
            Some(BigInt::from(10_00000000i64))
        );
        assert_eq!(
            storage_int(
                &snapshot,
                ContractManagement::ID,
                vec![PREFIX_NEXT_AVAILABLE_ID],
            ),
            Some(BigInt::from(1))
        );
        // The counter then hands out 1, 2, ... (C# GetNextAvailableId).
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&snapshot)
                .unwrap(),
            1
        );
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&snapshot)
                .unwrap(),
            2
        );
    }

    /// C# `ContractManagement.OnPersistAsync` at genesis: every genesis-active
    /// native gets a `Prefix_Contract` record (UpdateCounter 0), a
    /// `Prefix_ContractHash` id index entry, and a `Deploy` notification, in
    /// the canonical contract order. Natives activating at an unscheduled
    /// hardfork (Notary/Treasury here) are not deployed (C# IsInitializeBlock
    /// skips unconfigured hardforks).
    #[test]
    fn on_persist_writes_genesis_records_and_deploy_notifications() {
        let settings = settings_with(&[]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("on_persist");

        let genesis_native_names = [
            "ContractManagement",
            "StdLib",
            "CryptoLib",
            "LedgerContract",
            "NeoToken",
            "GasToken",
            "PolicyContract",
            "RoleManagement",
            "OracleContract",
        ];
        // C# interleaves native initialization with deployment: the
        // genesis-active NEO/GAS initializers emit Transfer before their
        // corresponding Deploy notifications.
        let notifications = engine.notifications();
        assert_eq!(notifications.len(), genesis_native_names.len() + 2);
        assert_eq!(notifications[0].event_name, "Deploy");
        assert_eq!(
            notifications[0].state[0].as_bytes().unwrap(),
            crate::ContractManagement::script_hash().to_bytes()
        );
        assert_eq!(notifications[1].event_name, "Deploy");
        assert_eq!(
            notifications[1].state[0].as_bytes().unwrap(),
            crate::StdLib::script_hash().to_bytes()
        );
        assert_eq!(notifications[2].event_name, "Deploy");
        assert_eq!(
            notifications[2].state[0].as_bytes().unwrap(),
            crate::CryptoLib::script_hash().to_bytes()
        );
        assert_eq!(notifications[3].event_name, "Deploy");
        assert_eq!(
            notifications[3].state[0].as_bytes().unwrap(),
            crate::LedgerContract::script_hash().to_bytes()
        );
        assert_eq!(notifications[4].event_name, "Transfer");
        assert_eq!(notifications[4].script_hash, crate::NeoToken::script_hash());
        assert_eq!(notifications[5].event_name, "Deploy");
        assert_eq!(
            notifications[5].state[0].as_bytes().unwrap(),
            crate::NeoToken::script_hash().to_bytes()
        );
        assert_eq!(notifications[6].event_name, "Transfer");
        assert_eq!(notifications[6].script_hash, crate::GasToken::script_hash());
        assert_eq!(notifications[7].event_name, "Deploy");
        assert_eq!(
            notifications[7].state[0].as_bytes().unwrap(),
            crate::GasToken::script_hash().to_bytes()
        );
        let deploy_notifications = notifications
            .iter()
            .filter(|notification| notification.event_name == "Deploy");
        for (notification, contract) in deploy_notifications.zip(NATIVE_CONTRACTS.iter()) {
            assert_eq!(notification.event_name, "Deploy");
            assert_eq!(notification.script_hash, ContractManagement::script_hash());
            assert_eq!(
                notification.state[0].as_bytes().unwrap(),
                contract.hash().to_bytes(),
                "Deploy order follows the canonical contract order"
            );
        }

        for (contract, name) in NATIVE_CONTRACTS.iter().zip(genesis_native_names.iter()) {
            assert_eq!(contract.name(), *name, "canonical registration order");
            let state = ContractManagement::get_contract_from_snapshot(&snapshot, &contract.hash())
                .unwrap()
                .unwrap_or_else(|| panic!("{name} record missing"));
            assert_eq!(state.id, contract.id());
            assert_eq!(state.hash, contract.hash());
            assert_eq!(state.update_counter, 0);
            assert_eq!(state.manifest.name, *name);
            // The id -> hash index dereferences back to the same record.
            let by_id =
                ContractManagement::get_contract_by_id_from_snapshot(&snapshot, contract.id())
                    .unwrap()
                    .unwrap_or_else(|| panic!("{name} id index missing"));
            assert_eq!(by_id.hash, contract.hash());
        }

        // Unscheduled ActiveIn hardforks: no record, no notification.
        assert!(
            ContractManagement::get_contract_from_snapshot(
                &snapshot,
                &crate::Notary::script_hash()
            )
            .unwrap()
            .is_none()
        );
        assert!(
            ContractManagement::get_contract_from_snapshot(
                &snapshot,
                &crate::Treasury::script_hash()
            )
            .unwrap()
            .is_none()
        );

        // A later non-hardfork block is a complete no-op.
        let mut engine = on_persist_engine(&snapshot, &settings, 1, 1000);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("block 1");
        assert!(engine.notifications().is_empty());
    }

    /// The HF_Echidna activation block (ContractManagement.cs:93-115): natives
    /// whose used hardforks include Echidna get their stored record refreshed
    /// (UpdateCounter++ + the height-composed NEF/manifest) and an `Update`
    /// notification; Notary (ActiveIn = Echidna) is deployed fresh; Policy's
    /// Echidna re-initialization (PolicyContract.cs:144-152) seeds the
    /// NotaryAssisted attribute fee and migrates the block-time settings.
    #[test]
    fn echidna_block_refreshes_manifests_and_runs_policy_reinitialization() {
        let settings = settings_with(&[(Hardfork::HfEchidna, 100)]);
        let snapshot = Arc::new(DataCache::new(false));
        // Genesis deployment pass.
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");

        // Pre-Echidna NEO manifest: NEP-17 only, no onNEP17Payment.
        let neo_hash = crate::NeoToken::script_hash();
        let pre = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
            .unwrap()
            .unwrap();
        assert_eq!(pre.manifest.supported_standards, ["NEP-17"]);
        assert!(!ContractManagement::abi_has_method(
            &pre.manifest,
            "onNEP17Payment",
            3
        ));

        // The Echidna activation block.
        let mut engine = on_persist_engine(&snapshot, &settings, 100, 100_000);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("echidna block");

        // NEO: refreshed in place — UpdateCounter 1, NEP-27 joins, the Echidna
        // ABI method appears, id/hash unchanged.
        let post = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
            .unwrap()
            .unwrap();
        assert_eq!(post.update_counter, 1);
        assert_eq!(post.id, crate::NeoToken::ID);
        assert_eq!(post.manifest.supported_standards, ["NEP-17", "NEP-27"]);
        assert!(ContractManagement::abi_has_method(
            &post.manifest,
            "onNEP17Payment",
            3
        ));

        // Notary: deployed fresh at its activation block.
        let notary = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::Notary::script_hash(),
        )
        .unwrap()
        .expect("Notary deploys at Echidna");
        assert_eq!(notary.update_counter, 0);

        // GAS carries no Echidna-gated metadata: untouched, no notification.
        let gas = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::GasToken::script_hash(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(gas.update_counter, 0);
        let gas_hash_bytes = crate::GasToken::script_hash().to_bytes();
        assert!(
            engine
                .notifications()
                .iter()
                .all(|n| n.state[0].as_bytes().unwrap() != gas_hash_bytes)
        );

        // Notification kinds: Update for refreshed natives, Deploy for Notary.
        let kinds: HashMap<Vec<u8>, String> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    n.state[0].as_bytes().unwrap().to_vec(),
                    n.event_name.clone(),
                )
            })
            .collect();
        assert_eq!(
            kinds.get(&neo_hash.to_bytes().to_vec()),
            Some(&"Update".to_string())
        );
        assert_eq!(
            kinds.get(&crate::Notary::script_hash().to_bytes().to_vec()),
            Some(&"Deploy".to_string())
        );

        // Policy Echidna re-initialization (PolicyContract.cs:144-152).
        let policy_id = crate::PolicyContract::ID;
        assert_eq!(
            storage_int(
                &snapshot,
                policy_id,
                vec![
                    POLICY_PREFIX_ATTRIBUTE_FEE,
                    neo_primitives::TransactionAttributeType::NotaryAssisted.to_byte()
                ]
            ),
            Some(BigInt::from(1000_0000i64)),
            "DefaultNotaryAssistedAttributeFee"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![21]),
            Some(BigInt::from(settings.milliseconds_per_block)),
            "MillisecondsPerBlock migrates from ProtocolSettings"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![22]),
            Some(BigInt::from(settings.max_valid_until_block_increment)),
            "MaxValidUntilBlockIncrement migrates from ProtocolSettings"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![23]),
            Some(BigInt::from(settings.max_traceable_blocks)),
            "MaxTraceableBlocks migrates from ProtocolSettings"
        );

        // Notary's own ActiveIn seeding runs inside ContractManagement
        // OnPersist, matching C# InitializeAsync(HF_Echidna).
        let notary_initialize_seed = storage_int(
            &snapshot,
            crate::Notary::ID,
            vec![NOTARY_PREFIX_MAX_NOT_VALID_BEFORE_DELTA],
        );
        assert_eq!(notary_initialize_seed, Some(BigInt::from(140)));
    }

    /// The HF_Faun activation block: Policy's Faun re-initialization
    /// (PolicyContract.cs:154-168) converts the stored exec-fee factor to
    /// pico-GAS units and stamps blocked accounts with the persisting block's
    /// timestamp; Treasury (ActiveIn = Faun) deploys.
    #[test]
    fn faun_block_reinitializes_policy_and_deploys_treasury() {
        let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        // Genesis: Policy's ActiveIn seeds (the pipeline's initialize pass) +
        // the deployment records.
        NativeContract::initialize(&crate::PolicyContract::new(), &mut engine)
            .expect("policy init");
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");
        // A pre-Faun blocked account (empty-bytes record).
        let blocked = UInt160::from_bytes(&[0x77; 20]).unwrap();
        let blocked_key = StorageKey::create_with_uint160(
            crate::PolicyContract::ID,
            POLICY_PREFIX_BLOCKED_ACCOUNT,
            &blocked,
        );
        snapshot.add(blocked_key.clone(), StorageItem::from_bytes(Vec::new()));

        let timestamp: u64 = 1_700_000_000_123;
        let mut engine = on_persist_engine(&snapshot, &settings, 50, timestamp);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("faun block");

        // ExecFeeFactor: 30 datoshi -> 300000 pico-GAS units.
        assert_eq!(
            storage_int(
                &snapshot,
                crate::PolicyContract::ID,
                vec![POLICY_PREFIX_EXEC_FEE_FACTOR]
            ),
            Some(BigInt::from(30i64 * 10_000))
        );
        // The blocked account now carries the persisting block's timestamp.
        assert_eq!(
            storage_int(
                &snapshot,
                crate::PolicyContract::ID,
                blocked_key.key().to_vec()
            ),
            Some(BigInt::from(timestamp))
        );
        // Treasury deploys at Faun.
        let treasury = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::Treasury::script_hash(),
        )
        .unwrap()
        .expect("Treasury deploys at Faun");
        assert_eq!(treasury.update_counter, 0);
        let kinds: HashMap<Vec<u8>, String> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    n.state[0].as_bytes().unwrap().to_vec(),
                    n.event_name.clone(),
                )
            })
            .collect();
        assert_eq!(
            kinds.get(&crate::Treasury::script_hash().to_bytes().to_vec()),
            Some(&"Deploy".to_string())
        );
    }

    /// C# PolicyContract.cs:155-157: the Faun exec-fee-factor conversion
    /// requires Policy to have been initialized ("Policy was not initialized").
    #[test]
    fn faun_reinitialization_faults_when_policy_was_never_initialized() {
        let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 50, 1);
        let result =
            crate::PolicyContract::new().initialize_for_hardfork(&mut engine, Hardfork::HfFaun);
        assert!(result.is_err(), "missing exec-fee factor must fault");
    }
}
