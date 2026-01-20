use neo_core::constants::GENESIS_TIMESTAMP_MS;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::neo_io::{BinaryWriter, Serializable};
use neo_core::network::p2p::payloads::{Signer, Transaction, WitnessScope};
use neo_core::persistence::{DataCache, StorageItem, StorageKey};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::{LedgerContract, NativeContract, NativeHelpers};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, UInt256, Witness};
use neo_vm::{OpCode, ScriptBuilder, VMState};
use num_traits::ToPrimitive;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 1_000_000_000;

fn sample_account() -> UInt160 {
    UInt160::from_bytes(&[7u8; 20]).expect("valid account")
}

fn make_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![OpCode::RET as u8]);
    tx.set_signers(vec![Signer::new(sample_account(), WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn make_block(index: u32, transactions: Vec<Transaction>) -> Block {
    let header = BlockHeader {
        index,
        previous_hash: UInt256::zero(),
        merkle_root: UInt256::zero(),
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        next_consensus: UInt160::zero(),
        witnesses: vec![Witness::empty()],
        ..Default::default()
    };
    Block::new(header, transactions)
}

fn make_genesis_block(settings: &ProtocolSettings) -> Block {
    let validators = settings.standby_validators();
    let next_consensus = if validators.is_empty() {
        UInt160::zero()
    } else {
        NativeHelpers::get_bft_address(&validators)
    };

    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        GENESIS_TIMESTAMP_MS,
        2_083_236_893u64,
        0,
        0,
        next_consensus,
        vec![Witness::new_with_scripts(
            Vec::new(),
            vec![OpCode::PUSH1 as u8],
        )],
    );

    Block::new(header, Vec::new())
}

fn persist_block(snapshot: &Arc<DataCache>, block: &Block, settings: ProtocolSettings) {
    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(snapshot),
        Some(block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(snapshot),
        Some(block.clone()),
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");
}

fn serialize_transaction_state_record(
    block_index: u32,
    vm_state: VMState,
    tx: &Transaction,
) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0x01).expect("record kind");
    writer.write_u32(block_index).expect("block index");
    writer.write_u8(vm_state as u8).expect("vm state");

    let mut tx_writer = BinaryWriter::new();
    tx.serialize(&mut tx_writer).expect("serialize tx");
    writer
        .write_var_bytes(&tx_writer.into_bytes())
        .expect("transaction bytes");
    writer.into_bytes()
}

#[test]
fn ledger_current_index_and_hash_after_persist() {
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let tx = make_transaction(1);
    let block = make_block(0, vec![tx]);
    persist_block(&snapshot, &block, ProtocolSettings::default());

    let current_index = ledger
        .current_index(snapshot.as_ref())
        .expect("current index");
    assert_eq!(current_index, 0);

    let current_hash = ledger
        .current_hash(snapshot.as_ref())
        .expect("current hash");
    assert_eq!(current_hash, block.hash());
}

#[test]
fn ledger_get_block_by_hash_and_index() {
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let tx = make_transaction(2);
    let block = make_block(0, vec![tx.clone()]);
    persist_block(&snapshot, &block, ProtocolSettings::default());

    let by_hash = ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Hash(block.hash()))
        .expect("get block by hash")
        .expect("block should exist");
    assert_eq!(by_hash.hash(), block.hash());
    assert_eq!(by_hash.transactions.len(), 1);
    assert_eq!(by_hash.transactions[0].hash(), tx.hash());

    let by_index = ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Index(0))
        .expect("get block by index")
        .expect("block should exist");
    assert_eq!(by_index.hash(), block.hash());

    let missing_hash = UInt256::from_bytes(&[9u8; 32]).expect("hash");
    assert!(ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Hash(missing_hash))
        .expect("get missing block")
        .is_none());
    assert!(ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Index(1))
        .expect("get missing block")
        .is_none());
}

#[test]
fn ledger_get_block_reconstructs_from_trimmed_block_and_states() {
    const PREFIX_BLOCK: u8 = 5;
    const PREFIX_TRANSACTION: u8 = 11;

    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let mut tx1 = make_transaction(1);
    tx1.set_script(vec![
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        0x01,
    ]);
    let mut tx2 = make_transaction(2);
    tx2.set_script(vec![
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        0x02,
    ]);

    let tx1_key =
        StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_TRANSACTION, &tx1.hash());
    let tx2_key =
        StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_TRANSACTION, &tx2.hash());
    snapshot.add(
        tx1_key,
        StorageItem::from_bytes(serialize_transaction_state_record(1, VMState::NONE, &tx1)),
    );
    snapshot.add(
        tx2_key,
        StorageItem::from_bytes(serialize_transaction_state_record(1, VMState::NONE, &tx2)),
    );

    let header = BlockHeader::new(
        0,
        UInt256::from("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff01"),
        UInt256::from("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff02"),
        581990400,
        0,
        1,
        0,
        UInt160::from("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01"),
        vec![Witness::new_with_scripts(
            Vec::new(),
            vec![OpCode::PUSH1 as u8],
        )],
    );
    let trimmed = neo_core::smart_contract::native::trimmed_block::TrimmedBlock::create(
        header.clone(),
        vec![tx1.hash(), tx2.hash()],
    );

    let mut trimmed_writer = BinaryWriter::new();
    trimmed
        .serialize(&mut trimmed_writer)
        .expect("serialize trimmed");
    let trimmed_key =
        StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_BLOCK, &trimmed.hash());
    snapshot.add(
        trimmed_key,
        StorageItem::from_bytes(trimmed_writer.into_bytes()),
    );

    let block = ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Hash(trimmed.hash()))
        .expect("get block")
        .expect("block present");

    assert_eq!(block.index(), 1);
    assert_eq!(block.header.merkle_root, header.merkle_root);
    assert_eq!(block.transactions.len(), 2);
    assert_eq!(block.transactions[0].hash(), tx1.hash());
    assert_eq!(block.transactions[1].hash(), tx2.hash());
    assert_eq!(
        block.header.witnesses[0].invocation_script,
        header.witnesses[0].invocation_script
    );
    assert_eq!(
        block.header.witnesses[0].verification_script,
        header.witnesses[0].verification_script
    );
}

#[test]
fn ledger_contains_block_and_transaction() {
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let tx = make_transaction(3);
    let block = make_block(0, vec![tx.clone()]);
    persist_block(&snapshot, &block, ProtocolSettings::default());

    assert!(ledger.contains_block(snapshot.as_ref(), &block.hash()));
    assert!(ledger
        .contains_transaction(snapshot.as_ref(), &tx.hash())
        .expect("contains transaction"));
    let missing_hash = UInt256::from_bytes(&[8u8; 32]).expect("hash");
    assert!(!ledger
        .contains_transaction(snapshot.as_ref(), &missing_hash)
        .expect("contains transaction"));
}

#[test]
fn ledger_get_transaction_state_returns_height() {
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let tx = make_transaction(4);
    let block = make_block(0, vec![tx.clone()]);
    persist_block(&snapshot, &block, ProtocolSettings::default());

    let state = ledger
        .get_transaction_state(snapshot.as_ref(), &tx.hash())
        .expect("transaction state")
        .expect("state should exist");
    assert_eq!(state.block_index(), 0);
    assert_eq!(state.transaction().hash(), tx.hash());
}

#[test]
fn ledger_contract_call_get_transaction_height() {
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let tx = make_transaction(5);
    let block = make_block(0, vec![tx.clone()]);
    persist_block(&snapshot, &block, ProtocolSettings::default());

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        400_000_000,
        None,
    )
    .expect("application engine");

    let mut script = ScriptBuilder::new();
    script.emit_push(&tx.hash().to_bytes());
    script.emit_push_int(1);
    script.emit_opcode(OpCode::PACK);
    script.emit_push_int(i64::from(CallFlags::ALL.bits()));
    script.emit_push("getTransactionHeight".as_bytes());
    script.emit_push(&ledger.hash().to_bytes());
    script
        .emit_syscall("System.Contract.Call")
        .expect("contract call syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result stack");
    let height = result
        .as_int()
        .expect("int result")
        .to_u32()
        .expect("height fits u32");
    assert_eq!(height, 0);
}

#[test]
fn ledger_get_block_hash_matches_genesis() {
    let settings = ProtocolSettings::mainnet();
    let snapshot = Arc::new(DataCache::new(false));
    let ledger = LedgerContract::new();

    let genesis = make_genesis_block(&settings);
    persist_block(&snapshot, &genesis, settings);

    let expected =
        UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
            .expect("expected genesis hash");

    let hash = ledger
        .get_block_hash_by_index(snapshot.as_ref(), 0)
        .expect("get block hash")
        .expect("hash present");
    assert_eq!(hash, expected);

    let block = ledger
        .get_block(snapshot.as_ref(), HashOrIndex::Index(0))
        .expect("get block")
        .expect("block present");
    assert_eq!(block.hash(), expected);
    assert!(ledger.contains_block(snapshot.as_ref(), &expected));
}
