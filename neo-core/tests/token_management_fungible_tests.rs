use neo_core::hardfork::Hardfork;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::neo_vm::StackItem;
use neo_core::network::p2p::payloads::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::ScriptBuilder;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::native::{NativeContract, TokenManagement};
use neo_core::smart_contract::TriggerType;
use neo_core::{UInt160, UInt256};
use neo_vm_rs::OpCode;
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
        let value = neo_vm_rs::StackValue::try_from(arg).expect("convert arg");
        sb.emit_push_stack_value(&value).expect("emit arg");
    }
    sb.emit_push_int(arg_count as i64);
    sb.emit_opcode(OpCode::PACK);
    sb.emit_push_int(CallFlags::ALL.bits() as i64);
    sb.emit_push_string(method);
    sb.emit_push_byte_array(&contract_hash.to_bytes());
    sb.emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
}

struct FungibleFixture {
    token_mgmt: TokenManagement,
    owner: UInt160,
    engine: ApplicationEngine,
}

impl FungibleFixture {
    fn new() -> Self {
        let settings = protocol_settings_with_faun();
        let snapshot = make_snapshot_with_genesis(&settings);
        let token_mgmt = TokenManagement::new();
        let owner = sample_account(0x01);
        let block = make_block(1);
        let engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            Some(block),
            settings,
            TEST_GAS_LIMIT,
            None,
        )
        .expect("engine");

        Self {
            token_mgmt,
            owner,
            engine,
        }
    }

    fn token_hash(&self) -> UInt160 {
        self.token_mgmt.hash()
    }

    fn call(&mut self, method: &str, args: &[Vec<u8>]) -> Vec<u8> {
        let token_hash = self.token_hash();
        self.engine
            .call_native_contract(token_hash, method, args)
            .unwrap_or_else(|err| panic!("{method} call: {err}"))
    }

    fn create(&mut self, name: &[u8], symbol: &[u8]) -> UInt160 {
        let create_args = vec![
            vec![0],
            self.owner.to_bytes(),
            name.to_vec(),
            symbol.to_vec(),
            vec![0],
            Vec::new(),
            vec![1],
        ];
        let asset_result = self.call("create", &create_args);
        UInt160::from_bytes(&asset_result).expect("asset id")
    }
}

#[test]
fn get_assets_of_owner_vm_call_returns_iterator_interface() {
    let mut fixture = FungibleFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x04);

    let asset_id = fixture.create(b"IterableToken", b"ITR");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = fixture.call("mint", &mint_args);
    assert_eq!(mint_result, vec![1]);

    let mut sb = ScriptBuilder::new();
    emit_contract_call(
        &mut sb,
        fixture.token_hash(),
        "getAssetsOfOwner",
        vec![StackItem::from_byte_string(holder.to_bytes())],
    );
    sb.emit_opcode(OpCode::RET);

    fixture
        .engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    fixture.engine.execute().expect("execute getAssetsOfOwner");

    match fixture.engine.result_stack().peek(0).unwrap() {
        StackItem::InteropInterface(_) => {}
        item => panic!(
            "expected iterator interop result, got {:?}",
            item.stack_item_type()
        ),
    }
}

#[test]
fn direct_invoke_transfer_requires_data_argument() {
    let mut fixture = FungibleFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x05);
    let recipient = sample_account(0x06);

    let asset_id = fixture.create(b"StrictTransferToken", b"STT");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = fixture.call("mint", &mint_args);
    assert_eq!(mint_result, vec![1]);

    fixture.engine.set_calling_script_hash(Some(holder));

    let err = fixture
        .token_mgmt
        .invoke(
            &mut fixture.engine,
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
    let mut fixture = FungibleFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x05);

    let asset_id = fixture.create(b"BurnableToken", b"BRN");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let mint_result = fixture.call("mint", &mint_args);
    assert_eq!(mint_result, vec![1]);

    fixture
        .engine
        .load_script(vec![neo_vm_rs::OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load overlay script");
    fixture.engine.set_calling_script_hash(Some(holder));

    let burn_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let burn_result = fixture.call("burn", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_assets_args = vec![holder.to_bytes()];
    let iterator_bytes = fixture.call("getAssetsOfOwner", &get_assets_args);
    let iterator_id = u32::from_le_bytes(iterator_bytes.try_into().expect("iterator id length"));

    assert!(
        !fixture
            .engine
            .iterator_next_internal(iterator_id)
            .expect("iterator next"),
        "getAssetsOfOwner should be empty after full burn in the overlay snapshot"
    );
}

#[test]
fn mint_and_burn_support_explicit_amount_argument() {
    let mut fixture = FungibleFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x06);

    let asset_id = fixture.create(b"AmountToken", b"AMT");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes(), vec![5]];
    let mint_result = fixture.call("mint", &mint_args);
    assert_eq!(mint_result, vec![1]);

    let balance_result = fixture.call("balanceOf", &[asset_id.to_bytes(), holder.to_bytes()]);
    assert_eq!(
        BigInt::from_signed_bytes_le(&balance_result),
        BigInt::from(5)
    );

    fixture.engine.set_calling_script_hash(Some(holder));
    let burn_args = vec![asset_id.to_bytes(), holder.to_bytes(), vec![3]];
    let burn_result = fixture.call("burn", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let balance_after_burn = fixture.call("balanceOf", &[asset_id.to_bytes(), holder.to_bytes()]);
    assert_eq!(
        BigInt::from_signed_bytes_le(&balance_after_burn),
        BigInt::from(2)
    );
}
