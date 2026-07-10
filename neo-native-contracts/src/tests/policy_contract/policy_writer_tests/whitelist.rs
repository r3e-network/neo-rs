use super::*;
/// setWhitelistFeeContract / removeWhitelistFeeContract round trip (HF_Faun):
/// the committee whitelists NEO.balanceOf (mirroring C# TestWhiteListFee),
/// the entry lands under [16] ++ hash ++ offset(BE) with the
/// WhitelistedContract struct value, the `whitelisted_fee` seam reads it
/// back, and the remove writer deletes it again.
#[test]
fn whitelist_fee_contract_e2e_set_then_remove() {
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
        |b| {
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
        NativeContract::<neo_execution::native_contract_provider::NoNativeContractProvider>::whitelisted_fee(
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
        NativeContract::<neo_execution::native_contract_provider::NoNativeContractProvider>::whitelisted_fee(
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
        NativeContract::<neo_execution::native_contract_provider::NoNativeContractProvider>::whitelisted_fee(
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
        |b| {
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
        |b| {
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
        |b| {
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
        |b| {
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
