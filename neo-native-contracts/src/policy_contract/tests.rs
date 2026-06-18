//! Tests for the PolicyContract native contract.
//!
//! Extracted from `policy_contract.rs` to keep the production module
//! focused. The `use super::*;` below re-exports the production items so
//! the inner test modules' own `use super::*;` resolves to them.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt256;
    use neo_storage::StorageItem;

    fn seed_current_block(cache: &DataCache, index: u32) {
        let value = crate::LedgerContract::new()
            .serialize_hash_index_state(&UInt256::default(), index)
            .expect("current block pointer");
        cache.add(
            StorageKey::new(crate::LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(value),
        );
    }

    fn seed_policy_setting(cache: &DataCache, prefix: u8, value: i64) {
        cache.add(
            PolicyContract::setting_key(prefix),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    #[test]
    fn native_contract_surface() {
        let c = PolicyContract::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getFeePerByte",
                "getStoragePrice",
                "setFeePerByte",
                "setStoragePrice",
                "getExecFeeFactor",
                "getExecPicoFeeFactor",
                "setExecFeeFactor",
                "getAttributeFee",
                "getAttributeFee",
                "setAttributeFee",
                "setAttributeFee",
                "getBlockedAccounts",
                "setMillisecondsPerBlock",
                "setMaxValidUntilBlockIncrement",
                "setMaxTraceableBlocks",
                "isBlocked",
                "unblockAccount",
                "getMillisecondsPerBlock",
                "getMaxValidUntilBlockIncrement",
                "getMaxTraceableBlocks",
                "blockAccount",
                "blockAccount",
                "setWhitelistFeeContract",
                "removeWhitelistFeeContract",
                "getWhitelistFeeContracts",
                "recoverFund"
            ]
        );
        // The Echidna-era chain-parameter getters are hardfork-gated.
        let mtb = c
            .methods()
            .iter()
            .find(|m| m.name == "getMaxTraceableBlocks")
            .unwrap();
        assert_eq!(mtb.active_in, Some(Hardfork::HfEchidna));
        // unblockAccount is a non-safe, write-flagged (States), Boolean writer.
        let unblock = c
            .methods()
            .iter()
            .find(|m| m.name == "unblockAccount")
            .unwrap();
        assert!(!unblock.safe);
        assert_eq!(unblock.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(unblock.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(unblock.return_type, ContractParameterType::Boolean);
        // The fee/price setters are non-safe, write-flagged (States), Void methods.
        for name in ["setFeePerByte", "setStoragePrice"] {
            let setter = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!setter.safe, "{name} must not be safe");
            assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(setter.return_type, ContractParameterType::Void);
        }
        // The Echidna setter additionally emits a notification (States|AllowNotify).
        let ms = c
            .methods()
            .iter()
            .find(|m| m.name == "setMillisecondsPerBlock")
            .unwrap();
        assert!(!ms.safe);
        assert_eq!(
            ms.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(ms.return_type, ContractParameterType::Void);
        assert_eq!(ms.active_in, Some(Hardfork::HfEchidna));
        // The cross-validated Echidna setters are non-safe, States, Void, gated.
        for name in ["setMaxValidUntilBlockIncrement", "setMaxTraceableBlocks"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!m.safe, "{name} must not be safe");
            assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(m.return_type, ContractParameterType::Void);
            assert_eq!(m.active_in, Some(Hardfork::HfEchidna));
        }
        // getExecFeeFactor is always present; getExecPicoFeeFactor is HF_Faun-gated;
        // both are safe Integer reads.
        let exec = c
            .methods()
            .iter()
            .find(|m| m.name == "getExecFeeFactor")
            .unwrap();
        assert!(exec.safe && exec.active_in.is_none());
        assert_eq!(exec.return_type, ContractParameterType::Integer);
        assert_eq!(exec.cpu_fee, 1 << 15);
        let pico = c
            .methods()
            .iter()
            .find(|m| m.name == "getExecPicoFeeFactor")
            .unwrap();
        assert!(pico.safe);
        assert_eq!(pico.active_in, Some(Hardfork::HfFaun));
        assert_eq!(pico.return_type, ContractParameterType::Integer);
        // setExecFeeFactor is a non-safe, States, Integer -> Void writer.
        let set_exec = c
            .methods()
            .iter()
            .find(|m| m.name == "setExecFeeFactor")
            .unwrap();
        assert!(!set_exec.safe);
        assert_eq!(set_exec.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(set_exec.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(set_exec.return_type, ContractParameterType::Void);
        assert!(set_exec.active_in.is_none());
        // getAttributeFee/setAttributeFee are dual C# registrations around
        // HF_Echidna. The ABI shape is unchanged, but exactly one descriptor is
        // active at a given height.
        let get_af_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "getAttributeFee")
            .collect();
        assert_eq!(get_af_versions.len(), 2);
        for m in &get_af_versions {
            assert!(m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.required_call_flags, CallFlags::READ_STATES.bits());
            assert_eq!(m.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
        }
        assert_eq!(get_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
        assert_eq!(get_af_versions[1].active_in, Some(Hardfork::HfEchidna));

        let set_af_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "setAttributeFee")
            .collect();
        assert_eq!(set_af_versions.len(), 2);
        for m in &set_af_versions {
            assert!(!m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(
                m.parameters,
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer
                ]
            );
            assert_eq!(m.return_type, ContractParameterType::Void);
        }
        assert_eq!(set_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
        assert_eq!(set_af_versions[1].active_in, Some(Hardfork::HfEchidna));
        // getBlockedAccounts is an HF_Faun-gated, safe, no-arg iterator reader.
        let blocked = c
            .methods()
            .iter()
            .find(|m| m.name == "getBlockedAccounts")
            .unwrap();
        assert_eq!(blocked.active_in, Some(Hardfork::HfFaun));
        assert!(blocked.safe && blocked.parameters.is_empty());
        assert_eq!(blocked.return_type, ContractParameterType::InteropInterface);
        assert_eq!(blocked.required_call_flags, CallFlags::READ_STATES.bits());
        // blockAccount is registered twice (C# V0/V1): V0 genesis-active and
        // DeprecatedIn HF_Faun with States; V1 ActiveIn HF_Faun with
        // States|AllowNotify. Both Hash160 -> Boolean, not safe, CpuFee 1<<15.
        let block_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "blockAccount")
            .collect();
        assert_eq!(block_versions.len(), 2);
        for m in &block_versions {
            assert!(!m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
            assert_eq!(m.return_type, ContractParameterType::Boolean);
        }
        let v0 = block_versions
            .iter()
            .find(|m| m.deprecated_in == Some(Hardfork::HfFaun))
            .expect("blockAccount V0");
        assert_eq!(v0.active_in, None);
        assert_eq!(v0.required_call_flags, CallFlags::STATES.bits());
        let v1 = block_versions
            .iter()
            .find(|m| m.active_in == Some(Hardfork::HfFaun))
            .expect("blockAccount V1");
        assert_eq!(v1.deprecated_in, None);
        assert_eq!(
            v1.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        // Whitelist writers: HF_Faun, not safe, States|AllowNotify, Void.
        let set_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "setWhitelistFeeContract")
            .unwrap();
        assert!(!set_wl.safe);
        assert_eq!(set_wl.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            set_wl.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            set_wl.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(set_wl.return_type, ContractParameterType::Void);
        let rm_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "removeWhitelistFeeContract")
            .unwrap();
        assert!(!rm_wl.safe);
        assert_eq!(rm_wl.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            rm_wl.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            rm_wl.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(rm_wl.return_type, ContractParameterType::Void);
        // getWhitelistFeeContracts: HF_Faun, safe, no-arg iterator reader.
        let get_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "getWhitelistFeeContracts")
            .unwrap();
        assert_eq!(get_wl.active_in, Some(Hardfork::HfFaun));
        assert!(get_wl.safe && get_wl.parameters.is_empty());
        assert_eq!(get_wl.return_type, ContractParameterType::InteropInterface);
        assert_eq!(get_wl.required_call_flags, CallFlags::READ_STATES.bits());
        // recoverFund: HF_Faun, not safe, States|AllowNotify, two Hash160 args.
        let recover = c
            .methods()
            .iter()
            .find(|m| m.name == "recoverFund")
            .unwrap();
        assert!(!recover.safe);
        assert_eq!(recover.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            recover.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            recover.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160
            ]
        );
        assert_eq!(recover.return_type, ContractParameterType::Boolean);
        assert_eq!(recover.cpu_fee, 1 << 15);
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
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, 1234); // Prefix_FeePerByte, must be excluded

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

        seed_policy_setting(
            &cache,
            PREFIX_EXEC_FEE_FACTOR,
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

        seed_policy_setting(
            &cache,
            PREFIX_EXEC_FEE_FACTOR,
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
        seed_policy_setting(&cache, PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, 42);
        seed_current_block(&cache, 0);

        assert_eq!(
            PolicyContract::new()
                .get_max_valid_until_block_increment_snapshot(&cache, &settings)
                .unwrap(),
            settings.max_valid_until_block_increment
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
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, i64::from(DEFAULT_FEE_PER_BYTE));
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
        seed_policy_setting(&cache, PREFIX_STORAGE_PRICE, DEFAULT_STORAGE_PRICE);
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
        assert!(
            PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK).is_ok()
        );
        assert!(PolicyContract::validate_milliseconds_per_block(0).is_err());
        assert!(
            PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK + 1)
                .is_err()
        );
    }

    #[test]
    fn milliseconds_per_block_write_persists_to_storage() {
        let cache = DataCache::new(false);
        seed_policy_setting(&cache, PREFIX_MILLISECONDS_PER_BLOCK, 15_000);
        PolicyContract::new()
            .put_milliseconds_per_block(&cache, 7_000)
            .unwrap();
        // Read back the raw storage value (the engine-aware getter adds the
        // ProtocolSettings default, which isn't needed once a value is stored).
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MILLISECONDS_PER_BLOCK, 0)
                .unwrap(),
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
        assert!(
            PolicyContract::validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS + 1).is_err()
        );
    }

    #[test]
    fn max_chain_param_writes_persist_to_storage() {
        let cache = DataCache::new(false);
        seed_policy_setting(&cache, PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, 5_760);
        PolicyContract::new()
            .put_max_valid_until_block_increment(&cache, 5_000)
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                PolicyContract::ID,
                PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
                0
            )
            .unwrap(),
            5_000
        );
        seed_policy_setting(&cache, PREFIX_MAX_TRACEABLE_BLOCKS, 2_102_400);
        PolicyContract::new()
            .put_max_traceable_blocks(&cache, 1_000_000)
            .unwrap();
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MAX_TRACEABLE_BLOCKS, 0)
                .unwrap(),
            1_000_000
        );
    }

    #[test]
    fn is_blocked_checks_storage_existence() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();
        let key = StorageKey::create_with_uint160(PolicyContract::ID, PREFIX_BLOCKED_ACCOUNT, &account);
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
            !<PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash)
                .unwrap()
        );
        cache.add(
            PolicyContract::blocked_account_key(&hash),
            StorageItem::from_bytes(vec![]),
        );
        assert!(
            <PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash)
                .unwrap()
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
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, 4242);
        assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 4242);
    }

    #[test]
    fn storage_price_reads_storage_with_default() {
        let cache = DataCache::new(false);
        let err = PolicyContract::new()
            .storage_price(&cache)
            .expect_err("missing StoragePrice storage should fault");
        assert!(err.to_string().contains("StoragePrice"), "{err}");

        seed_policy_setting(&cache, PREFIX_STORAGE_PRICE, 250_000);
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
        let engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            std::sync::Arc::new(cache),
            None,
            settings.clone(),
            0,
            None,
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
        assert_eq!(
            Interoperable::to_stack_value(&view).unwrap(),
            StackValue::try_from(expected_item.clone()).unwrap()
        );

        let mut trait_decoded = WhitelistedContractView {
            contract_hash: UInt160::from_bytes(&[0x00; 20]).unwrap(),
            method: String::new(),
            arg_count: 0,
            fixed_fee: 0,
        };
        Interoperable::from_stack_value(
            &mut trait_decoded,
            StackValue::try_from(expected_item).unwrap(),
        )
        .unwrap();
        assert_eq!(trait_decoded, view);
    }

    #[test]
    fn whitelisted_contract_storage_uses_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("mod.rs");
        let decoder = slice_between(
            source,
            "fn decode_whitelisted_contract",
            "fn encode_whitelisted_contract",
        );
        assert!(decoder.contains("deserialize_stack_value_with_limits"));
        assert!(decoder.contains("WhitelistedContractView::from_stack_value"));
        assert!(!decoder.contains("BinarySerializer::deserialize("));

        let encoder = slice_between(
            source,
            "fn encode_whitelisted_contract",
            "fn whitelist_fee_entries",
        );
        assert!(encoder.contains("to_stack_value"));
        assert!(encoder.contains("serialize_stack_value_default"));
        assert!(!encoder.contains("StackItem::from_struct"));
        assert!(!encoder.contains("BinarySerializer::serialize("));
    }

    #[test]
    fn committee_cache_reader_uses_stack_value_projection() {
        let source = include_str!("mod.rs");
        let start = source
            .find("fn read_neo_committee_sorted")
            .expect("committee reader exists");
        let end = source[start..]
            .find("fn assert_almost_full_committee")
            .map(|offset| start + offset)
            .expect("assert_almost_full_committee follows committee reader");
        let helper = &source[start..end];

        assert!(helper.contains("deserialize_stack_value_with_limits"));
        assert!(helper.contains("CachedCommittee::from_stack_value"));
        assert!(!helper.contains("StackValue::Array"));
        assert!(!helper.contains("StackValue::Struct"));
        assert!(!helper.contains("BinarySerializer::deserialize("));
        assert!(!helper.contains("StackItem::Array"));
        assert!(!helper.contains("StackItem::Struct"));
    }

    #[test]
    fn whitelist_fee_key_is_prefix_hash_and_big_endian_offset() {
        // C# CreateStorageKey(Prefix_WhitelistedFeeContracts, contractHash,
        // methodDescriptor.Offset): [16] ++ hash(20) ++ offset as big-endian i32.
        let hash = UInt160::from_bytes(&[0xAB; 20]).unwrap();
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
                PolicyContract::is_native_contract_hash(&spec.hash),
                "{} ({}) is native",
                spec.name,
                spec.hash
            );
        }
        assert!(!PolicyContract::is_native_contract_hash(
            &UInt160::from_bytes(&[0x42; 20]).unwrap()
        ));
    }

    #[test]
    fn remaining_time_message_matches_csharp_format() {
        // C# RecoverFund's ternary chain: days -> "{d}d {h}h {m}m",
        // hours -> "{h}h {m}m {s}s", minutes -> "{m}m {s}s", else "{s}s".
        let ms = |d: i64, h: i64, m: i64, s: i64| {
            d * 86_400_000 + h * 3_600_000 + m * 60_000 + s * 1_000
        };
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
}

/// End-to-end verification of the committee-gated PolicyContract writers
/// through the VM (the witness-gated script-execution path proven by
/// `neo_token::witness_harness_tests`): a script `System.Contract.Call`s
/// PolicyContract with the committee multisig address as signer, and the
/// resulting storage transitions are asserted against the shared snapshot.
#[cfg(test)]
mod policy_writer_tests {
    use super::*;
    use crate::test_support::{
        CM_PREFIX_CONTRACT, NEO_PREFIX_COMMITTEE, POLICY_PREFIX_ATTRIBUTE_FEE, committee_address,
        deploy_native, hex, sample_committee, seed_committee,
    };
    use neo_config::ProtocolSettings;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_manifest::{ContractManifest, ContractMethodDescriptor, NefFile};
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// ProtocolSettings with HF_Faun scheduled from genesis.
    fn faun_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        settings
    }

    /// Runs `method(args...)` on PolicyContract via System.Contract.Call,
    /// signed (Global) by `signer`, against the shared `snapshot`. The closure
    /// must push the call arguments in REVERSE order (deepest first). Returns
    /// the final VM state and the finished engine (for result-stack and
    /// notification assertions).
    fn call_policy_engine(
        snapshot: Arc<DataCache>,
        signer: UInt160,
        settings: ProtocolSettings,
        block: Option<Block>,
        method: &str,
        argc: i64,
        push_args_reversed: &dyn Fn(&mut ScriptBuilder),
    ) -> (VmState, ApplicationEngine) {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        push_args_reversed(&mut builder);
        builder.emit_push_int(argc);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(method.as_bytes());
        builder.emit_push(&PolicyContract::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            block,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    /// [`call_policy_engine`] reduced to the final VM state and the boolean on
    /// top of the result stack (if any).
    fn call_policy(
        snapshot: Arc<DataCache>,
        signer: UInt160,
        settings: ProtocolSettings,
        block: Option<Block>,
        method: &str,
        argc: i64,
        push_args_reversed: &dyn Fn(&mut ScriptBuilder),
    ) -> (VmState, Option<bool>) {
        let (state, engine) = call_policy_engine(
            snapshot,
            signer,
            settings,
            block,
            method,
            argc,
            push_args_reversed,
        );
        let top = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok());
        (state, top)
    }

    fn returning_user_contract(hash: UInt160) -> ContractState {
        let nef = NefFile::new(
            "policy-blocked-call-test".to_string(),
            vec![
                neo_vm_rs::OpCode::PUSH1.byte(),
                neo_vm_rs::OpCode::RET.byte(),
            ],
        );
        let mut manifest = ContractManifest::new("BlockedCallFixture".to_string());
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "answer".to_string(),
                Vec::new(),
                ContractParameterType::Integer,
                0,
                true,
            )
            .expect("method descriptor"),
        );
        ContractState::new(7, hash, nef, manifest)
    }

    #[test]
    fn real_policy_blocked_storage_rejects_system_contract_call_target() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let target_hash = UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030405").unwrap();
        deploy_native(&cache, &returning_user_contract(target_hash));
        cache.add(
            PolicyContract::blocked_account_key(&target_hash),
            StorageItem::from_bytes(Vec::new()),
        );

        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("answer".as_bytes());
        builder.emit_push(&target_hash.to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::new(cache),
            None,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");

        let state = engine.execute_allow_fault();
        assert_eq!(
            state,
            VmState::FAULT,
            "C# ApplicationEngine.CallContractInternal rejects Policy-blocked contracts before invocation"
        );
        assert_eq!(
            engine.invocation_stack().len(),
            1,
            "blocked contract target must not be loaded as an invocation context"
        );
    }

    /// Pre-Faun blockAccount (the V0 registration): committee-gated, writes an
    /// empty `Prefix_BlockedAccount` record, and double-blocking returns false
    /// (C# UT_PolicyContract.Check_BlockAccount).
    #[test]
    fn block_account_e2e_pre_faun_blocks_then_double_block_returns_false() {
        crate::install();
        // Default MainNet schedules Faun at 8,800,000, so block 0 is pre-Faun.
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

        // blockAccount's pre-Faun path records the persisting block timestamp
        // (Faun onwards stores GetTime()), so the engine needs a persisting
        // block fixture. Height 0 is pre-Faun on MainNet defaults.
        let mut persisting_header = BlockHeader::default();
        persisting_header.set_index(0);
        persisting_header.set_timestamp(1_700_000_000_000);
        let persisting_block = Some(Block::from_parts(persisting_header, vec![]));

        let (state, result) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            persisting_block,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::HALT, "blockAccount must HALT");
        assert_eq!(result, Some(true), "first block returns true");
        let item = snapshot
            .get(&PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written");
        assert!(
            item.value_bytes().is_empty(),
            "pre-Faun blocked value is empty"
        );

        // Blocking the same account again returns false (no fault).
        let (state2, result2) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state2, VmState::HALT, "double block must still HALT");
        assert_eq!(result2, Some(false), "double block returns false");
    }

    /// blockAccount without the committee witness faults (C# AssertCommittee
    /// throws) and writes nothing.
    #[test]
    fn block_account_e2e_requires_committee_witness() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            stranger,
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state,
            VmState::FAULT,
            "non-committee blockAccount must FAULT"
        );
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&account))
                .is_none()
        );
    }

    /// blockAccount on a native contract hash faults ("Cannot block a native
    /// contract.") even with the committee witness.
    #[test]
    fn block_account_e2e_rejects_native_contract_hash() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&gas_hash.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "blocking a native hash must FAULT");
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&gas_hash))
                .is_none()
        );
    }

    /// Faun-path blockAccount (the V1 registration): clears the account's vote
    /// via NEO.VoteInternal (candidate weight drops, VoteTo cleared,
    /// _votersCount reduced) and stamps the blocked entry with the persisting
    /// block's millisecond timestamp (`engine.GetTime()`).
    #[test]
    fn block_account_e2e_faun_clears_vote_and_stamps_time() {
        const BLOCK_TIME_MS: u64 = 1_234_567_890;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );

        // A registered candidate with 100 votes, all from `voter` (balance 100,
        // voting since height 0), and the matching _votersCount.
        let candidate = committee[0].clone();
        let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let candidate_state =
            StackItem::from_struct(vec![StackItem::from_bool(true), StackItem::from_int(100)]);
        // NeoToken Prefix_Candidate (0x21 = 33); the suffix is the 33-byte ECPoint.
        let candidate_key =
            StorageKey::create_with_bytes(crate::NeoToken::ID, 33u8, &candidate.to_bytes());
        cache.add(
            candidate_key.clone(),
            StorageItem::from_bytes(
                BinarySerializer::serialize(&candidate_state, &ExecutionEngineLimits::default())
                    .unwrap(),
            ),
        );
        let voter_state = StackItem::from_struct(vec![
            StackItem::from_int(100),                          // Balance
            StackItem::from_int(0),                            // BalanceHeight
            StackItem::from_byte_string(candidate.to_bytes()), // VoteTo
            StackItem::from_int(0),                            // LastGasPerVote
        ]);
        // NEP-17 Prefix_Account (0x14 = 20).
        let voter_key = StorageKey::create_with_uint160(crate::NeoToken::ID, 20u8, &voter);
        cache.add(
            voter_key.clone(),
            StorageItem::from_bytes(
                BinarySerializer::serialize(&voter_state, &ExecutionEngineLimits::default())
                    .unwrap(),
            ),
        );
        // NeoToken Prefix_VotersCount (0x01).
        let voters_count_key = StorageKey::create(crate::NeoToken::ID, 1u8);
        cache.add(
            voters_count_key.clone(),
            StorageItem::from_bytes(BigInt::from(100).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        // Persisting block at index 100 with a known timestamp (GetTime source).
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(BLOCK_TIME_MS);
        let block = Block::from_parts(header, vec![]);

        let (state, result) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(block),
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&voter.to_array());
            },
        );
        assert_eq!(state, VmState::HALT, "Faun blockAccount must HALT");
        assert_eq!(result, Some(true));

        // The blocked entry carries the block timestamp (the recoverFund clock).
        let blocked = snapshot
            .get(&PolicyContract::blocked_account_key(&voter))
            .expect("blocked entry written");
        assert_eq!(
            blocked.value_bytes().into_owned(),
            BigInt::from(BLOCK_TIME_MS).to_signed_bytes_le()
        );

        // The candidate lost the voter's 100-NEO weight (still registered).
        let cand = snapshot.get(&candidate_key).expect("candidate entry kept");
        let decoded = BinarySerializer::deserialize(
            &cand.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("candidate state is not a struct");
        };
        assert!(
            fields.items()[0].as_bool().unwrap(),
            "candidate stays registered"
        );
        assert_eq!(
            fields.items()[1].as_int().unwrap(),
            BigInt::from(0),
            "votes cleared"
        );

        // The voter's VoteTo is now null and the reward markers advanced.
        let acct = snapshot.get(&voter_key).expect("voter account kept");
        let decoded = BinarySerializer::deserialize(
            &acct.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("voter account state is not a struct");
        };
        assert_eq!(
            fields.items()[0].as_int().unwrap(),
            BigInt::from(100),
            "balance kept"
        );
        assert!(
            matches!(fields.items()[2], StackItem::Null),
            "VoteTo cleared"
        );

        // _votersCount dropped by the voter's balance (100 -> 0).
        let voters = snapshot.get(&voters_count_key).expect("voters count kept");
        assert_eq!(
            BigInt::from_signed_bytes_le(&voters.value_bytes()),
            BigInt::from(0)
        );
    }

    /// setWhitelistFeeContract / removeWhitelistFeeContract round trip (HF_Faun):
    /// the committee whitelists NEO.balanceOf (mirroring C# TestWhiteListFee),
    /// the entry lands under [16] ++ hash ++ offset(BE) with the
    /// WhitelistedContract struct value, the `whitelisted_fee` seam reads it
    /// back, and the remove writer deletes it again.
    #[test]
    fn whitelist_fee_contract_e2e_set_then_remove() {
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        // The whitelist target: NEO's deployed state (its manifest carries the
        // balanceOf(1) descriptor whose offset keys the whitelist entry).
        let neo_state = build_native_contract_state(&crate::NeoToken, &settings, 0);
        let balance_of_offset = neo_state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == "balanceOf" && m.parameters.len() == 1)
            .expect("NEO balanceOf in manifest")
            .offset;
        deploy_native(&cache, &neo_state);
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let neo_hash = crate::NeoToken::script_hash();

        // setWhitelistFeeContract(NEO, "balanceOf", 1, 12345).
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(12345); // fixedFee (arg 3, deepest)
                b.emit_push_int(1); // argCount (arg 2)
                b.emit_push("balanceOf".as_bytes()); // method (arg 1)
                b.emit_push(&neo_hash.to_array()); // contractHash (arg 0, top)
            },
        );
        assert_eq!(state, VmState::HALT, "setWhitelistFeeContract must HALT");
        let key = PolicyContract::whitelist_fee_key(&neo_hash, balance_of_offset);
        let item = snapshot.get(&key).expect("whitelist entry written");
        let view = PolicyContract::decode_whitelisted_contract(&item.value_bytes()).unwrap();
        assert_eq!(view.contract_hash, neo_hash);
        assert_eq!(view.method, "balanceOf");
        assert_eq!(view.arg_count, 1);
        assert_eq!(view.fixed_fee, 12345);

        // The engine-facing seam (C# IsWhitelistFeeContract) resolves the fee.
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &neo_hash,
                "balanceOf",
                1
            )
            .unwrap(),
            Some(12345)
        );
        // A different method / a missing contract resolve to no whitelist.
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &neo_hash,
                "transfer",
                4
            )
            .unwrap(),
            None
        );
        let unknown = UInt160::from_bytes(&[0x55; 20]).unwrap();
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &unknown,
                "balanceOf",
                1
            )
            .unwrap(),
            None
        );

        // removeWhitelistFeeContract(NEO, "balanceOf", 1) deletes the entry.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "removeWhitelistFeeContract",
            3,
            &|b| {
                b.emit_push_int(1); // argCount (arg 2, deepest)
                b.emit_push("balanceOf".as_bytes()); // method (arg 1)
                b.emit_push(&neo_hash.to_array()); // contractHash (arg 0, top)
            },
        );
        assert_eq!(
            state2,
            VmState::HALT,
            "removeWhitelistFeeContract must HALT"
        );
        assert!(snapshot.get(&key).is_none(), "whitelist entry deleted");

        // Removing again faults: C# throws "Whitelist not found".
        let (state3, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "removeWhitelistFeeContract",
            3,
            &|b| {
                b.emit_push_int(1);
                b.emit_push("balanceOf".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(
            state3,
            VmState::FAULT,
            "removing a missing whitelist must FAULT"
        );
    }

    /// setWhitelistFeeContract rejects a negative fixedFee before the committee
    /// check (C# ArgumentOutOfRangeException.ThrowIfNegative) and faults for an
    /// unknown method (C# "Method ... was not found").
    #[test]
    fn whitelist_fee_contract_e2e_validation_faults() {
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::NeoToken, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let neo_hash = crate::NeoToken::script_hash();

        // Negative fixedFee -> FAULT.
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(-1);
                b.emit_push_int(1);
                b.emit_push("balanceOf".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "negative fixedFee must FAULT");

        // Unknown method name -> FAULT, nothing stored.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(5);
                b.emit_push_int(0);
                b.emit_push("noexists".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(state2, VmState::FAULT, "unknown method must FAULT");
        assert!(
            PolicyContract::new()
                .whitelist_fee_entries(&snapshot)
                .is_empty()
        );
    }

    /// recoverFund's verifiable prefix: the almost-full-committee gate (2-of-3
    /// here, max(max(1, n-(n-1)/2), n-2) = 2 for n = 3) plus the
    /// "Request not found." fault for an account that was never blocked.
    #[test]
    fn recover_fund_e2e_requires_request_and_committee() {
        const BLOCK_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        let snapshot = Arc::new(cache);
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(BLOCK_TIME_MS);

        // Without the almost-full-committee witness -> FAULT.
        let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            stranger,
            settings.clone(),
            Some(Block::from_parts(header.clone(), vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
                b.emit_push(&account.to_array()); // account (arg 0, top)
            },
        );
        assert_eq!(
            state,
            VmState::FAULT,
            "non-committee recoverFund must FAULT"
        );

        // With the witness but no blocked entry -> FAULT ("Request not found.").
        // For the 3-member sample committee the almost-full threshold equals the
        // regular committee threshold (both 2-of-3), so the same address signs.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state2,
            VmState::FAULT,
            "recoverFund without a request must FAULT"
        );
    }

    /// Seeds a GAS `AccountState` (`Struct[Balance]`) for `account`.
    fn seed_gas_balance(cache: &DataCache, account: &UInt160, balance: i64) {
        let state = StackItem::from_struct(vec![StackItem::from_int(balance)]);
        let key = StorageKey::create_with_uint160(crate::GasToken::ID, crate::NEP17_PREFIX_ACCOUNT, account);
        cache.add(
            key,
            StorageItem::from_bytes(
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap(),
            ),
        );
    }

    /// recoverFund happy path (C# `PolicyContract.RecoverFund`, lines 663-680):
    /// exactly one year after the blocked-account request, an almost-full
    /// committee signer sweeps the account's full GAS balance to Treasury
    /// through the VM — `balanceOf` then `transfer` issued from the native
    /// frame with `account` as the native calling script hash (authorizing the
    /// transfer via the `from == CallingScriptHash` bypass), Treasury's
    /// `onNEP17Payment` callback included — and emits `Transfer` followed by
    /// `RecoveredFund(account)`.
    #[test]
    fn recover_fund_e2e_sweeps_balance_to_treasury_and_notifies() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        const SWEPT: i64 = 123_456_789;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );
        // Treasury must be a deployed contract so the GAS transfer's
        // onNEP17Payment callback runs (C# PostTransferAsync calls it whenever
        // the recipient is a contract).
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::Treasury, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let treasury = *crate::hashes::TREASURY_HASH;
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        // The blocked-account entry carries the request's millisecond timestamp.
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        seed_gas_balance(&cache, &account, SWEPT);
        let snapshot = Arc::new(cache);

        // Exactly one year elapsed: C# faults only when `elapsed < required`,
        // so the boundary block must pass.
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, engine) = call_policy_engine(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
                b.emit_push(&account.to_array()); // account (arg 0, top)
            },
        );
        assert_eq!(
            state,
            VmState::HALT,
            "recoverFund sweep must HALT: {:?}",
            engine.fault_exception()
        );
        assert!(
            engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
            "recoverFund returns true after a sweep"
        );

        // The full balance moved to Treasury; the account's entry was deleted
        // (an exact-balance NEP-17 transfer removes the from-record).
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &treasury).unwrap(),
            BigInt::from(SWEPT)
        );
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
        // recoverFund does not unblock the account.
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&account))
                .is_some()
        );

        // Notification order matches C#: the GAS Transfer (emitted inside the
        // nested transfer call) first, then Policy's RecoveredFund(account).
        let notifications = engine.notifications();
        assert_eq!(notifications.len(), 2, "expected Transfer + RecoveredFund");
        assert_eq!(notifications[0].script_hash, gas_hash);
        assert_eq!(notifications[0].event_name, "Transfer");
        assert_eq!(
            notifications[0].state[0].as_bytes().unwrap(),
            account.to_bytes()
        );
        assert_eq!(
            notifications[0].state[1].as_bytes().unwrap(),
            treasury.to_bytes()
        );
        assert_eq!(
            notifications[0].state[2].as_int().unwrap(),
            BigInt::from(SWEPT)
        );
        assert_eq!(notifications[1].script_hash, PolicyContract::script_hash());
        assert_eq!(notifications[1].event_name, "RecoveredFund");
        assert_eq!(
            notifications[1].state[0].as_bytes().unwrap(),
            account.to_bytes()
        );
    }

    /// recoverFund with a zero balance: C# `return false` — HALT, nothing
    /// moves, and neither Transfer nor RecoveredFund is emitted.
    #[test]
    fn recover_fund_e2e_zero_balance_returns_false() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, engine) = call_policy_engine(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state,
            VmState::HALT,
            "zero-balance recoverFund must HALT: {:?}",
            engine.fault_exception()
        );
        assert!(
            !engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
            "recoverFund returns false when there is nothing to sweep"
        );
        assert!(
            engine.notifications().is_empty(),
            "no Transfer/RecoveredFund for an empty sweep"
        );
        assert_eq!(
            crate::read_nep17_balance(
                &snapshot,
                crate::GasToken::ID,
                &crate::hashes::TREASURY_HASH
            )
            .unwrap(),
            BigInt::from(0)
        );
    }

    /// One millisecond short of the one-year window faults (C# "Request must
    /// be signed at least 1 year ago. Remaining time: …") and moves no funds.
    #[test]
    fn recover_fund_e2e_rejects_recent_request() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        const BALANCE: i64 = 777;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        seed_gas_balance(&cache, &account, BALANCE);
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND - 1);

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "a too-recent request must FAULT");
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &account).unwrap(),
            BigInt::from(BALANCE),
            "the balance must be untouched"
        );
    }

    /// A deployed token that does not declare the NEP-17 standard faults (C#
    /// "Contract {token} does not implement NEP-17 standard."). Treasury is a
    /// deployed non-NEP-17 contract, so it doubles as the token here.
    #[test]
    fn recover_fund_e2e_requires_nep17_standard() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::Treasury, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let treasury = *crate::hashes::TREASURY_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&treasury.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "a non-NEP-17 token must FAULT");
    }
}
