use super::*;

/// Seeds a GAS balance entry (`Struct[Balance]`) and a matching total supply.
fn seed_gas(cache: &DataCache, account: &UInt160, balance: &BigInt) {
    let state = StackItem::from_struct(vec![StackItem::from_int(balance.clone())]);
    let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
    cache.add(
        crate::GasToken::account_key(account),
        StorageItem::from_bytes(bytes),
    );
    cache.add(
        crate::GasToken::total_supply_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(balance)),
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

#[test]
fn on_nep17_payment_data_parser_uses_stack_value_projection() {
    let source = include_str!("../../../neo_token/invoke.rs");
    let start = source
        .find("crate::NEP17_PAYMENT_METHOD =>")
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

/// Full Echidna flow (C# NeoToken.OnNEP17Payment, NeoToken.cs:374-389):
/// `GAS.transfer(sender -> NEO, registerPrice, data = pubkey)` registers
/// the candidate and burns the GAS from NEO's balance.
#[test]
fn on_nep17_payment_registers_candidate_and_burns_gas() {
    let pubkey = candidate_pubkey();
    let candidate_account =
        UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let price = BigInt::from(DEFAULT_REGISTER_PRICE);

    crate::install();
    let cache = DataCache::new(false);
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

    let mut tx = Transaction::new();
    tx.set_signers(vec![
        Signer::new(sender, WitnessScope::GLOBAL),
        Signer::new(candidate_account, WitnessScope::GLOBAL),
    ]);
    tx.set_witnesses(vec![Witness::empty(), Witness::empty()]);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    let mut b = ScriptBuilder::new();
    b.emit_push(&pubkey.to_bytes());
    b.emit_push_int(DEFAULT_REGISTER_PRICE);
    b.emit_push(&NeoToken::script_hash().to_array());
    b.emit_push(&sender.to_array());
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

    let item = snapshot
        .get(&NeoToken::candidate_key(&pubkey))
        .expect("candidate entry written");
    let (registered, votes) = NeoToken::decode_candidate_state(&item.value_bytes()).unwrap();
    assert!(registered, "candidate is Registered");
    assert_eq!(votes, BigInt::from(0));
    assert!(
        snapshot
            .get(&crate::GasToken::account_key(&sender))
            .is_none(),
        "sender spent all GAS"
    );
    assert!(
        snapshot
            .get(&crate::GasToken::account_key(&NeoToken::script_hash()))
            .is_none(),
        "NEO's received GAS is burned"
    );
    let supply = snapshot
        .get(&crate::GasToken::total_supply_key())
        .expect("supply entry");
    assert_eq!(
        BigInt::from_signed_bytes_le(&supply.value_bytes()),
        BigInt::from(0)
    );
}

/// C# OnNEP17Payment faults unless the caller is the GAS contract, amount
/// equals register price, `data` decodes as a public key, and the candidate
/// account witnesses the transaction.
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

    let mut engine = payment_engine(Arc::clone(&snapshot), None, Some(candidate_account));
    let err = NativeContract::invoke(
        &neo,
        &mut engine,
        "onNEP17Payment",
        &payment_args(&sender, DEFAULT_REGISTER_PRICE, &pubkey_item),
    )
    .unwrap_err();
    assert!(err.to_string().contains("only the GAS contract"), "{err}");

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
