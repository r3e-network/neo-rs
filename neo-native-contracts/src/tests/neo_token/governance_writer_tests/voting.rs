use super::*;

#[test]
fn vote_assigns_weight_distributes_gas_and_records_target() {
    use neo_payloads::{Block, BlockHeader};

    let candidate = candidate_pubkey();
    let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let cache = DataCache::new(false);
    deploy_native(
        &cache,
        &build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0),
    );
    cache.update(
        NeoToken::candidate_key(&candidate),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(0)).unwrap()),
    );
    let voter_state = NeoAccountStateView {
        balance: BigInt::from(100),
        balance_height: 0,
        vote_to: None,
        last_gas_per_vote: BigInt::from(0),
    };
    cache.update(
        NeoToken::account_key(&voter),
        StorageItem::from_bytes(NeoToken::encode_neo_account_state(&voter_state).unwrap()),
    );
    NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
    let snapshot = Arc::new(cache);

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(voter, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container = Arc::new(VerifiableContainer::from(tx));
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&candidate.to_bytes());
    builder.emit_push(&voter.to_array());
    builder.emit_push_int(2);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push("vote".as_bytes());
    builder.emit_push(&NeoToken::script_hash().to_array());
    builder.emit_syscall("System.Contract.Call").expect("call");

    let mut header = BlockHeader::default();
    header.set_index(100);
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        Some(Block::from_parts(header, vec![])),
        ProtocolSettings::default(),
        2000_00000000,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
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

    let (_, cand_votes) = NeoToken::decode_candidate_state(
        &snapshot
            .get(&NeoToken::candidate_key(&candidate))
            .unwrap()
            .value_bytes(),
    )
    .unwrap();
    assert_eq!(cand_votes, BigInt::from(100));

    let acct = NeoToken::decode_neo_account_state(
        &NeoToken::new()
            .read_account_state(&snapshot, &voter)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(acct.vote_to, Some(candidate));

    let gas_key = crate::GasToken::account_key(&voter);
    let gas_item = snapshot.get(&gas_key).expect("voter GAS account written");
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
