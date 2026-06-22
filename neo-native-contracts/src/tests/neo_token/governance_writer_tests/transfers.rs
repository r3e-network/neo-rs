use super::*;

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
        NeoToken::account_key(&from),
        StorageItem::from_bytes(NeoToken::encode_neo_account_state(&from_state).unwrap()),
    );
    let snapshot = Arc::new(cache);

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let mut b = ScriptBuilder::new();
    b.emit_push(&[]);
    b.emit_push_int(30);
    b.emit_push(&to.to_array());
    b.emit_push(&from.to_array());
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

    let (_, cand_votes) = NeoToken::decode_candidate_state(
        &snapshot
            .get(&NeoToken::candidate_key(&candidate))
            .unwrap()
            .value_bytes(),
    )
    .unwrap();
    assert_eq!(cand_votes, BigInt::from(70));
}
