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
) -> ApplicationEngine<crate::StandardNativeProvider> {
    let container: Option<Arc<VerifiableContainer>> = signer.map(|hash| {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(hash, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        Arc::new(VerifiableContainer::from(tx))
    });
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        container,
        snapshot,
        None,
        ProtocolSettings::default(),
        10_000_000,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
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

fn seed_registration_payment_snapshot(
    sender: UInt160,
    candidate_account: UInt160,
    pubkey: &ECPoint,
) -> (
    Arc<DataCache>,
    Arc<VerifiableContainer>,
    ProtocolSettings,
    Vec<u8>,
) {
    let price = BigInt::from(DEFAULT_REGISTER_PRICE);
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
    let container = Arc::new(VerifiableContainer::from(tx));

    (
        snapshot,
        container,
        settings,
        gas_transfer_register_candidate_script(sender, pubkey),
    )
}

fn gas_transfer_register_candidate_script(sender: UInt160, pubkey: &ECPoint) -> Vec<u8> {
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
    b.to_array()
}

fn gas_transfer_register_then_get_candidates_script(sender: UInt160, pubkey: &ECPoint) -> Vec<u8> {
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
    b.emit_syscall("System.Contract.Call")
        .expect("transfer call");
    b.emit_opcode(neo_vm_rs::OpCode::DROP);

    b.emit_push_int(0);
    b.emit_pack();
    b.emit_push_int(i64::from(CallFlags::ALL.bits()));
    b.emit_push("getCandidates".as_bytes());
    b.emit_push(&NeoToken::script_hash().to_array());
    b.emit_syscall("System.Contract.Call")
        .expect("getCandidates call");
    b.to_array()
}

fn registration_payment_engine(
    snapshot: Arc<DataCache>,
    container: Arc<VerifiableContainer>,
    settings: ProtocolSettings,
    script: Vec<u8>,
    gas_limit: i64,
) -> ApplicationEngine<crate::StandardNativeProvider> {
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        settings,
        gas_limit,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("loads");
    engine
}

#[test]
fn on_nep17_payment_data_parser_uses_stack_value_projection() {
    let source = include_str!("../../../neo_token/invoke.rs");
    let start = source
        .find("fn invoke_on_nep17_payment(")
        .or_else(|| source.find("fn invoke_on_nep17_payment<"))
        .expect("onNEP17Payment handler exists");
    let end = source[start..]
        .find("fn invoke_unregister_candidate(")
        .or_else(|| source[start..].find("fn invoke_unregister_candidate<"))
        .map(|offset| start + offset)
        .expect("next handler exists");
    let handler = &source[start..end];

    assert!(handler.contains("decode_stack_value"));
    assert!(handler.contains("to_byte_string_bytes"));
    assert!(!handler.contains("BinarySerializer::deserialize("));
}

#[test]
fn neo_payment_callback_runs_before_deferred_gas_distribution_mint() {
    let source = include_str!("../../../neo_token/transfers.rs");

    let post_start = source
        .find("pub(super) fn neo_post_transfer")
        .expect("neo_post_transfer exists");
    let transfer_start = source
        .find("pub(super) fn neo_transfer_core")
        .expect("neo_transfer_core exists");
    let post_transfer = &source[post_start..transfer_start];
    assert!(post_transfer.contains("call_from_native_contract_void"));
    assert!(!post_transfer.contains("queue_contract_call_from_native"));

    let vote_start = source
        .find("pub(crate) fn vote_internal")
        .expect("vote_internal exists");
    let transfer_core = &source[transfer_start..vote_start];
    let callback = transfer_core
        .find("self.neo_post_transfer")
        .expect("transfer calls post transfer");
    let distribution_mint = transfer_core
        .find("for (account, datoshi) in distributions")
        .expect("transfer mints deferred GAS distributions");
    assert!(callback < distribution_mint);

    let mint_start = source
        .find("pub(super) fn neo_mint")
        .expect("neo_mint exists");
    let mint_body = &source[mint_start..];
    let callback = mint_body
        .find("call_from_native_contract_void")
        .expect("mint uses synchronous payment callback");
    let distribution_mint = mint_body
        .find("for (target, datoshi) in distributions")
        .expect("mint mints deferred GAS distributions");
    assert!(callback < distribution_mint);
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
    let (snapshot, container, settings, script) =
        seed_registration_payment_snapshot(sender, candidate_account, &pubkey);

    let mut engine = registration_payment_engine(
        Arc::clone(&snapshot),
        container,
        settings,
        script,
        2000_00000000,
    );
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

#[test]
fn gas_payment_registration_is_visible_before_next_contract_call() {
    let pubkey = candidate_pubkey();
    let candidate_account =
        UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let (snapshot, container, settings, _) =
        seed_registration_payment_snapshot(sender, candidate_account, &pubkey);
    let script = gas_transfer_register_then_get_candidates_script(sender, &pubkey);

    let mut engine = registration_payment_engine(
        Arc::clone(&snapshot),
        container,
        settings,
        script,
        2000_00000000,
    );

    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "GAS.transfer and immediate NEO.getCandidates must HALT: {:?}",
        engine.fault_exception(),
    );

    let candidates = engine
        .result_stack()
        .peek(0)
        .expect("getCandidates result")
        .as_array()
        .expect("candidate array");
    let registered_pubkeys = candidates
        .iter()
        .map(|entry| {
            let fields = entry.as_array().expect("Struct[pubkey, votes]");
            fields[0].as_bytes().expect("candidate pubkey")
        })
        .collect::<Vec<_>>();
    assert!(
        registered_pubkeys
            .iter()
            .any(|candidate| candidate.as_slice() == pubkey.to_bytes().as_slice()),
        "candidate registered by GAS payment must be visible to the next contract call"
    );
}

#[test]
fn gas_transfer_to_neo_faults_when_native_transfer_fee_exceeds_transaction_gas_limit() {
    let pubkey = candidate_pubkey();
    let candidate_account =
        UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let (snapshot, container, settings, script) =
        seed_registration_payment_snapshot(sender, candidate_account, &pubkey);

    let mut engine = registration_payment_engine(
        Arc::clone(&snapshot),
        container,
        settings,
        script,
        3_932_159,
    );

    assert_eq!(
        engine.execute_allow_fault(),
        VmState::FAULT,
        "GAS.transfer to NEO must fault when its native transfer fee exceeds the transaction gas limit",
    );
    let fault = engine
        .fault_exception()
        .expect("insufficient gas fault captured");
    assert!(
        fault.contains("Insufficient gas"),
        "unexpected fault: {fault}"
    );
    assert!(
        snapshot.get(&NeoToken::candidate_key(&pubkey)).is_none(),
        "candidate registration must not commit after an insufficient GAS fault"
    );
}

/// C# OnNEP17Payment faults unless the caller is the GAS contract, amount
/// equals register price, `data` decodes as a public key, and the candidate
/// account witnesses the transaction.
#[test]
fn on_nep17_payment_rejects_bad_caller_amount_pubkey_and_witness() {
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
