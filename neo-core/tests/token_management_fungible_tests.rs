use neo_core::hardfork::Hardfork;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::network::p2p::payloads::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::{NativeContract, TokenManagement};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, UInt256};
use neo_vm::{OpCode, ScriptBuilder, StackItem};
use std::collections::HashMap;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 3_000_000_000;

use num_bigint::BigInt;

fn protocol_settings_with_faun() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = neo_core::ledger::create_genesis_block(settings);

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");

    snapshot
}

fn make_block(index: u32) -> Block {
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
    Block::new(header, Vec::new())
}

fn sample_account(tag: u8) -> UInt160 {
    let bytes = [tag; 20];
    UInt160::from_bytes(&bytes).unwrap()
}

fn emit_contract_call(
    sb: &mut ScriptBuilder,
    contract_hash: UInt160,
    method: &str,
    mut args: Vec<StackItem>,
) {
    let arg_count = args.len();
    for arg in args.drain(..).rev() {
        sb.emit_push_stack_item(arg).expect("emit arg");
    }
    sb.emit_push_int(arg_count as i64);
    sb.emit_opcode(OpCode::PACK);
    sb.emit_push_int(CallFlags::ALL.bits() as i64);
    sb.emit_push_string(method);
    sb.emit_push_byte_array(&contract_hash.to_bytes());
    sb.emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
}

#[test]
fn get_assets_of_owner_vm_call_returns_iterator_interface() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);
    let holder = sample_account(0x04);

    let block = make_block(1);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let create_args = vec![
        vec![0],
        owner.to_bytes(),
        b"IterableToken".to_vec(),
        b"ITR".to_vec(),
        vec![0],
        Vec::new(),
        vec![1],
    ];
    let asset_result = engine
        .call_native_contract(token_mgmt.hash(), "create", &create_args)
        .expect("create call");
    let asset_id = UInt160::from_bytes(&asset_result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = engine
        .call_native_contract(token_mgmt.hash(), "mint", &mint_args)
        .expect("mint call");
    assert_eq!(mint_result, vec![1]);

    let mut sb = ScriptBuilder::new();
    emit_contract_call(
        &mut sb,
        token_mgmt.hash(),
        "getAssetsOfOwner",
        vec![StackItem::from_byte_string(holder.to_bytes())],
    );
    sb.emit_opcode(OpCode::RET);

    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute getAssetsOfOwner");

    match engine.result_stack().peek(0).unwrap() {
        StackItem::InteropInterface(_) => {}
        item => panic!(
            "expected iterator interop result, got {:?}",
            item.stack_item_type()
        ),
    }
}

#[test]
fn direct_invoke_transfer_requires_data_argument() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);
    let holder = sample_account(0x05);
    let recipient = sample_account(0x06);

    let block = make_block(1);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let create_args = vec![
        vec![0],
        owner.to_bytes(),
        b"StrictTransferToken".to_vec(),
        b"STT".to_vec(),
        vec![0],
        Vec::new(),
        vec![1],
    ];
    let asset_result = engine
        .call_native_contract(token_mgmt.hash(), "create", &create_args)
        .expect("create call");
    let asset_id = UInt160::from_bytes(&asset_result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = engine
        .call_native_contract(token_mgmt.hash(), "mint", &mint_args)
        .expect("mint call");
    assert_eq!(mint_result, vec![1]);

    engine.set_calling_script_hash(Some(holder));

    let err = token_mgmt
        .invoke(
            &mut engine,
            "transfer",
            &[
                asset_id.to_bytes(),
                holder.to_bytes(),
                recipient.to_bytes(),
                vec![1],
            ],
        )
        .expect_err("direct invoke without data should fail arity validation");
    assert!(err
        .to_string()
        .contains("TokenManagement.transfer: invalid arguments"));
}

#[test]

fn get_assets_of_owner_excludes_fully_burned_asset_in_same_overlay() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);
    let holder = sample_account(0x05);

    let block = make_block(1);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let create_args = vec![
        vec![0],
        owner.to_bytes(),
        b"BurnableToken".to_vec(),
        b"BRN".to_vec(),
        vec![0],
        Vec::new(),
        vec![1],
    ];
    let asset_result = engine
        .call_native_contract(token_mgmt.hash(), "create", &create_args)
        .expect("create call");
    let asset_id = UInt160::from_bytes(&asset_result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = engine
        .call_native_contract(token_mgmt.hash(), "mint", &mint_args)
        .expect("mint call");
    assert_eq!(mint_result, vec![1]);

    engine
        .load_script(vec![neo_vm::OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load overlay script");
    engine.set_calling_script_hash(Some(holder));

    let burn_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let burn_result = engine
        .call_native_contract(token_mgmt.hash(), "burn", &burn_args)
        .expect("burn call");
    assert_eq!(burn_result, vec![1]);

    let get_assets_args = vec![holder.to_bytes()];
    let iterator_bytes = engine
        .call_native_contract(token_mgmt.hash(), "getAssetsOfOwner", &get_assets_args)
        .expect("getAssetsOfOwner call");
    let iterator_id = u32::from_le_bytes(iterator_bytes.try_into().expect("iterator id length"));

    assert!(
        !engine
            .iterator_next_internal(iterator_id)
            .expect("iterator next"),
        "getAssetsOfOwner should be empty after full burn in the overlay snapshot"
    );
}

#[test]
fn mint_and_burn_support_explicit_amount_argument() {
    let settings = protocol_settings_with_faun();
    let snapshot = make_snapshot_with_genesis(&settings);
    let token_mgmt = TokenManagement::new();
    let owner = sample_account(0x01);
    let holder = sample_account(0x06);

    let block = make_block(1);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        Some(block),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let create_args = vec![
        vec![0],
        owner.to_bytes(),
        b"AmountToken".to_vec(),
        b"AMT".to_vec(),
        vec![0],
        Vec::new(),
        vec![1],
    ];
    let asset_result = engine
        .call_native_contract(token_mgmt.hash(), "create", &create_args)
        .expect("create call");
    let asset_id = UInt160::from_bytes(&asset_result).expect("asset id");

    engine.set_current_script_hash(Some(owner));
    engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes(), vec![5]];
    let mint_result = engine
        .call_native_contract(token_mgmt.hash(), "mint", &mint_args)
        .expect("mint with explicit amount");
    assert_eq!(mint_result, vec![1]);

    let balance_result = engine
        .call_native_contract(
            token_mgmt.hash(),
            "balanceOf",
            &[asset_id.to_bytes(), holder.to_bytes()],
        )
        .expect("balanceOf after mint");
    assert_eq!(
        BigInt::from_signed_bytes_le(&balance_result),
        BigInt::from(5)
    );

    engine.set_calling_script_hash(Some(holder));
    let burn_args = vec![asset_id.to_bytes(), holder.to_bytes(), vec![3]];
    let burn_result = engine
        .call_native_contract(token_mgmt.hash(), "burn", &burn_args)
        .expect("burn with explicit amount");
    assert_eq!(burn_result, vec![1]);

    let balance_after_burn = engine
        .call_native_contract(
            token_mgmt.hash(),
            "balanceOf",
            &[asset_id.to_bytes(), holder.to_bytes()],
        )
        .expect("balanceOf after burn");
    assert_eq!(
        BigInt::from_signed_bytes_le(&balance_after_burn),
        BigInt::from(2)
    );
}
