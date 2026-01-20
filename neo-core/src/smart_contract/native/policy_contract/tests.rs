use super::*;
use crate::hardfork::HardforkManager;
use crate::ledger::{create_genesis_block, Block, BlockHeader};
use crate::network::p2p::payloads::{Signer, Transaction, Witness, WitnessScope};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::iterators::i_iterator::IIterator;
use crate::smart_contract::iterators::IteratorInterop;
use crate::smart_contract::native::{
    AccountState, ContractManagement, GasToken, NativeContract, NativeHelpers, NeoToken,
    TreasuryContract,
};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::IInteroperable;
use crate::{IVerifiable, UInt160, UInt256};
use neo_primitives::TransactionAttributeType;
use neo_vm::{
    execution_engine_limits::ExecutionEngineLimits, OpCode, ScriptBuilder, StackItem, VMState,
};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::sync::Arc;

#[test]
fn test_whitelist_stack_item_roundtrip() {
    let bytes = hex::decode("1122334455667788990011223344556677889900").unwrap();
    let contract_hash = UInt160::from_bytes(&bytes).unwrap();
    let method = "testMethod";
    let arg_count = 3;
    let fixed_fee = 123456789;

    let wl = WhitelistedContract {
        contract_hash,
        method: method.to_string(),
        arg_count,
        fixed_fee,
    };

    let item = wl.to_stack_item();
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
    let decoded_item =
        BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();

    let mut decoded = WhitelistedContract::default();
    decoded.from_stack_item(decoded_item);

    assert_eq!(wl, decoded);
}

const TEST_GAS_LIMIT: i64 = 5_000_000_000;

fn settings_all_active() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = std::collections::HashMap::new();
    for hardfork in HardforkManager::all() {
        hardforks.insert(hardfork, 0);
    }
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = create_genesis_block(settings);

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

fn make_block(index: u32, timestamp: u64) -> Block {
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        timestamp,
        0,
        index,
        0,
        UInt160::zero(),
        vec![Witness::empty()],
    );
    Block::new(header, Vec::new())
}

fn make_engine(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    signers: Vec<Signer>,
    persisting_block: Option<Block>,
) -> ApplicationEngine {
    let container = if signers.is_empty() {
        None
    } else {
        let mut tx = Transaction::new();
        tx.set_signers(signers);
        tx.set_witnesses(vec![Witness::empty(); tx.signers().len()]);
        tx.set_script(vec![OpCode::NOP as u8]);
        Some(Arc::new(tx) as Arc<dyn IVerifiable>)
    };

    ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        persisting_block,
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine")
}

fn make_engine_with_script(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    signers: Vec<Signer>,
    persisting_block: Option<Block>,
    script: Vec<u8>,
) -> ApplicationEngine {
    let mut tx = Transaction::new();
    tx.set_signers(signers);
    tx.set_witnesses(vec![Witness::empty(); tx.signers().len()]);
    tx.set_script(script.clone());

    let container: Arc<dyn IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        persisting_block,
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load script");
    engine
}

fn committee_address(settings: &ProtocolSettings, snapshot: &DataCache) -> UInt160 {
    NativeHelpers::committee_address(settings, Some(snapshot))
}

fn almost_full_committee_address(settings: &ProtocolSettings, snapshot: &DataCache) -> UInt160 {
    let committee = NeoToken::new()
        .committee_from_snapshot(snapshot)
        .filter(|members| !members.is_empty())
        .unwrap_or_else(|| settings.standby_committee.clone());
    let len = committee.len();
    let min = std::cmp::max(1, len.saturating_sub((len.saturating_sub(1)) / 2));
    let m = std::cmp::max(min, len.saturating_sub(2));
    Contract::create_multi_sig_contract(m, &committee).script_hash()
}

fn int_arg_i64(value: i64) -> Vec<u8> {
    StackItem::from_int(value)
        .as_bytes()
        .expect("integer bytes")
}

fn int_arg_u32(value: u32) -> Vec<u8> {
    StackItem::from_int(value as i64)
        .as_bytes()
        .expect("integer bytes")
}

fn int_arg_u8(value: u8) -> Vec<u8> {
    StackItem::from_int(value as i64)
        .as_bytes()
        .expect("integer bytes")
}

fn bytes_to_i64(bytes: &[u8]) -> i64 {
    StackItem::from_byte_string(bytes.to_vec())
        .as_int()
        .expect("int")
        .to_i64()
        .expect("i64")
}

fn bytes_to_bool(bytes: &[u8]) -> bool {
    StackItem::from_byte_string(bytes.to_vec())
        .as_bool()
        .expect("bool")
}

fn emit_dynamic_call(
    builder: &mut ScriptBuilder,
    contract_hash: &UInt160,
    method: &str,
    args: &[Vec<u8>],
) {
    for arg in args {
        builder.emit_push_byte_array(arg);
    }
    builder.emit_push_int(args.len() as i64);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract_hash.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
}

#[test]
fn check_default() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    let policy = PolicyContract::new();

    let ret = engine
        .call_native_contract(policy.hash(), "getFeePerByte", &[])
        .expect("getFeePerByte");
    assert_eq!(
        bytes_to_i64(&ret),
        PolicyContract::DEFAULT_FEE_PER_BYTE as i64
    );

    let attr = TransactionAttributeType::Conflicts as u8;
    let ret = engine
        .call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(attr)])
        .expect("getAttributeFee");
    assert_eq!(
        bytes_to_i64(&ret),
        PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64
    );

    let invalid = u8::MAX;
    let result =
        engine.call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(invalid)]);
    assert!(result.is_err());
}

#[test]
fn check_set_attribute_fee() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();
    let attr = TransactionAttributeType::Conflicts as u8;

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setAttributeFee",
        &[int_arg_u8(attr), int_arg_u32(100_500)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(attr)])
        .expect("getAttributeFee");
    assert_eq!(bytes_to_i64(&ret), 0);

    // With signature, wrong value.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setAttributeFee",
        &[int_arg_u8(attr), int_arg_u32(1_100_000_000)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(attr)])
        .expect("getAttributeFee");
    assert_eq!(bytes_to_i64(&ret), 0);

    // Proper set.
    let ret = engine
        .call_native_contract(
            policy.hash(),
            "setAttributeFee",
            &[int_arg_u8(attr), int_arg_u32(300_300)],
        )
        .expect("setAttributeFee");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(attr)])
        .expect("getAttributeFee");
    assert_eq!(bytes_to_i64(&ret), 300_300);

    // Set to zero.
    let ret = engine
        .call_native_contract(
            policy.hash(),
            "setAttributeFee",
            &[int_arg_u8(attr), int_arg_u32(0)],
        )
        .expect("setAttributeFee");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getAttributeFee", &[int_arg_u8(attr)])
        .expect("getAttributeFee");
    assert_eq!(bytes_to_i64(&ret), 0);
}

#[test]
fn check_set_fee_per_byte() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(policy.hash(), "setFeePerByte", &[int_arg_i64(1)]);
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getFeePerByte", &[])
        .expect("getFeePerByte");
    assert_eq!(bytes_to_i64(&ret), 1000);

    // With signature.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "setFeePerByte", &[int_arg_i64(1)])
        .expect("setFeePerByte");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getFeePerByte", &[])
        .expect("getFeePerByte");
    assert_eq!(bytes_to_i64(&ret), 1);
}

#[test]
fn check_set_base_exec_fee() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(policy.hash(), "setExecFeeFactor", &[int_arg_u32(50)]);
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getExecFeeFactor", &[])
        .expect("getExecFeeFactor");
    assert_eq!(bytes_to_i64(&ret), 30);

    // With signature, wrong value.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setExecFeeFactor",
        &[int_arg_u32(1_005_000_000)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getExecFeeFactor", &[])
        .expect("getExecFeeFactor");
    assert_eq!(bytes_to_i64(&ret), 30);

    // Proper set (scaled by fee factor).
    let ret = engine
        .call_native_contract(policy.hash(), "setExecFeeFactor", &[int_arg_u32(500_000)])
        .expect("setExecFeeFactor");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getExecFeeFactor", &[])
        .expect("getExecFeeFactor");
    assert_eq!(bytes_to_i64(&ret), 50);
}

#[test]
fn check_recover_funds_complete_flow() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let gas = GasToken::new();

    let committee = committee_address(&settings, snapshot.as_ref());
    let almost_full = almost_full_committee_address(&settings, snapshot.as_ref());

    let blocked_account =
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("blocked account");
    let start_time = 1_000_000u64;
    let required_time = 365u64 * 24 * 60 * 60 * 1_000;
    let finish_time = start_time + required_time + 1_000;

    let block_start = make_block(1000, start_time);
    let block_finish = make_block(2000, finish_time);

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block_start.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "recoverFund",
        &[blocked_account.to_bytes(), gas.hash().to_bytes()],
    );
    assert!(result.is_err());

    // Block account.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block_start.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[blocked_account.to_bytes()])
        .expect("blockAccount");
    assert!(bytes_to_bool(&ret));

    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &blocked_account)
        .expect("is blocked"));

    // Set GAS balance for blocked account.
    let gas_balance = BigInt::from(50_000) * BigInt::from(10u8).pow(u32::from(gas.decimals()));
    let gas_key = StorageKey::create_with_uint160(
        gas.id(),
        crate::smart_contract::native::fungible_token::PREFIX_ACCOUNT,
        &blocked_account,
    );
    let gas_state = AccountState::with_balance(gas_balance.clone());
    let gas_bytes = BinarySerializer::serialize(
        &gas_state.to_stack_item(),
        &ExecutionEngineLimits::default(),
    )
    .expect("serialize account state");
    snapshot.add(
        gas_key,
        crate::persistence::StorageItem::from_bytes(gas_bytes),
    );
    assert_eq!(
        gas.balance_of_snapshot(snapshot.as_ref(), &blocked_account),
        gas_balance
    );

    // Recover funds with almost-full committee.
    let mut engine = make_engine_with_script(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(almost_full, WitnessScope::GLOBAL)],
        Some(block_finish.clone()),
        vec![OpCode::NOP as u8],
    );
    engine
        .call_native_contract(
            policy.hash(),
            "recoverFund",
            &[blocked_account.to_bytes(), gas.hash().to_bytes()],
        )
        .expect("recoverFund");

    assert!(gas
        .balance_of_snapshot(snapshot.as_ref(), &blocked_account)
        .is_zero());
    let treasury_balance =
        gas.balance_of_snapshot(snapshot.as_ref(), &TreasuryContract::new().hash());
    assert!(treasury_balance >= gas_balance);
}

#[test]
fn check_set_storage_price() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result =
        engine.call_native_contract(policy.hash(), "setStoragePrice", &[int_arg_u32(100_500)]);
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getStoragePrice", &[])
        .expect("getStoragePrice");
    assert_eq!(bytes_to_i64(&ret), 100_000);

    // With signature, wrong value.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setStoragePrice",
        &[int_arg_u32(100_000_000)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getStoragePrice", &[])
        .expect("getStoragePrice");
    assert_eq!(bytes_to_i64(&ret), 100_000);

    // Proper set.
    let ret = engine
        .call_native_contract(policy.hash(), "setStoragePrice", &[int_arg_u32(300_300)])
        .expect("setStoragePrice");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getStoragePrice", &[])
        .expect("getStoragePrice");
    assert_eq!(bytes_to_i64(&ret), 300_300);
}

#[test]
fn check_set_max_valid_until_block_increment() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setMaxValidUntilBlockIncrement",
        &[int_arg_u32(123)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getMaxValidUntilBlockIncrement", &[])
        .expect("getMaxValidUntilBlockIncrement");
    assert_eq!(bytes_to_i64(&ret), 5760);

    // With signature, wrong value.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setMaxValidUntilBlockIncrement",
        &[int_arg_u32(100_000_000)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getMaxValidUntilBlockIncrement", &[])
        .expect("getMaxValidUntilBlockIncrement");
    assert_eq!(bytes_to_i64(&ret), 5760);

    // Proper set.
    let ret = engine
        .call_native_contract(
            policy.hash(),
            "setMaxValidUntilBlockIncrement",
            &[int_arg_u32(123)],
        )
        .expect("setMaxValidUntilBlockIncrement");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getMaxValidUntilBlockIncrement", &[])
        .expect("getMaxValidUntilBlockIncrement");
    assert_eq!(bytes_to_i64(&ret), 123);

    // Update MaxTraceableBlocks for further test.
    let ret = engine
        .call_native_contract(policy.hash(), "setMaxTraceableBlocks", &[int_arg_u32(6000)])
        .expect("setMaxTraceableBlocks");
    assert!(ret.is_empty());

    // Set MaxValidUntilBlockIncrement >= MaxTraceableBlocks should fail.
    let result = engine.call_native_contract(
        policy.hash(),
        "setMaxValidUntilBlockIncrement",
        &[int_arg_u32(6000)],
    );
    assert!(result.is_err());
}

#[test]
fn check_set_milliseconds_per_block() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setMillisecondsPerBlock",
        &[int_arg_u32(123)],
    );
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getMillisecondsPerBlock", &[])
        .expect("getMillisecondsPerBlock");
    assert_eq!(bytes_to_i64(&ret), 15_000);

    // With signature, too big value.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setMillisecondsPerBlock",
        &[int_arg_u32(30_001)],
    );
    assert!(result.is_err());

    // With signature, too small value.
    let result =
        engine.call_native_contract(policy.hash(), "setMillisecondsPerBlock", &[int_arg_u32(0)]);
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getMillisecondsPerBlock", &[])
        .expect("getMillisecondsPerBlock");
    assert_eq!(bytes_to_i64(&ret), 15_000);

    // Proper set.
    let ret = engine
        .call_native_contract(
            policy.hash(),
            "setMillisecondsPerBlock",
            &[int_arg_u32(3_000)],
        )
        .expect("setMillisecondsPerBlock");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getMillisecondsPerBlock", &[])
        .expect("getMillisecondsPerBlock");
    assert_eq!(bytes_to_i64(&ret), 3_000);
}

#[test]
fn check_block_account() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let account_a =
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("account a");
    let result =
        engine.call_native_contract(policy.hash(), "blockAccount", &[account_a.to_bytes()]);
    assert!(result.is_err());

    // With signature.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[account_a.to_bytes()])
        .expect("blockAccount");
    assert!(bytes_to_bool(&ret));

    // Same account.
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[account_a.to_bytes()])
        .expect("blockAccount");
    assert!(!bytes_to_bool(&ret));

    // Account B.
    let account_b =
        UInt160::parse("0xb400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("account b");
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[account_b.to_bytes()])
        .expect("blockAccount");
    assert!(bytes_to_bool(&ret));

    assert!(!policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));
    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &account_a)
        .expect("is blocked"));
    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &account_b)
        .expect("is blocked"));
}

#[test]
fn check_block_unblock_account() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());

    // Block without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result =
        engine.call_native_contract(policy.hash(), "blockAccount", &[UInt160::zero().to_bytes()]);
    assert!(result.is_err());
    assert!(!policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));

    // Block with signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[UInt160::zero().to_bytes()])
        .expect("blockAccount");
    assert!(bytes_to_bool(&ret));
    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));

    // Unblock without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "unblockAccount",
        &[UInt160::zero().to_bytes()],
    );
    assert!(result.is_err());
    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));

    // Unblock with signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(
            policy.hash(),
            "unblockAccount",
            &[UInt160::zero().to_bytes()],
        )
        .expect("unblockAccount");
    assert!(bytes_to_bool(&ret));
    assert!(!policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));
}

#[test]
fn check_set_max_traceable_blocks() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();

    // Without signature.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
    );
    let result =
        engine.call_native_contract(policy.hash(), "setMaxTraceableBlocks", &[int_arg_u32(123)]);
    assert!(result.is_err());

    let ret = engine
        .call_native_contract(policy.hash(), "getMaxTraceableBlocks", &[])
        .expect("getMaxTraceableBlocks");
    assert_eq!(bytes_to_i64(&ret), 2_102_400);

    // Proper set.
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "setMaxTraceableBlocks", &[int_arg_u32(5761)])
        .expect("setMaxTraceableBlocks");
    assert!(ret.is_empty());

    let ret = engine
        .call_native_contract(policy.hash(), "getMaxTraceableBlocks", &[])
        .expect("getMaxTraceableBlocks");
    assert_eq!(bytes_to_i64(&ret), 5761);

    // Larger value should be prohibited.
    let result =
        engine.call_native_contract(policy.hash(), "setMaxTraceableBlocks", &[int_arg_u32(5762)]);
    assert!(result.is_err());
}

#[test]
fn test_list_blocked_accounts() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1000, 1_000);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());

    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        Some(block.clone()),
    );
    let ret = engine
        .call_native_contract(policy.hash(), "blockAccount", &[UInt160::zero().to_bytes()])
        .expect("blockAccount");
    assert!(bytes_to_bool(&ret));

    assert!(policy
        .is_blocked_snapshot(snapshot.as_ref(), &UInt160::zero())
        .expect("is blocked"));

    let mut script = ScriptBuilder::new();
    emit_dynamic_call(&mut script, &policy.hash(), "getBlockedAccounts", &[]);
    script.emit_opcode(OpCode::RET);

    let mut engine = make_engine_with_script(
        Arc::clone(&snapshot),
        settings.clone(),
        Vec::new(),
        Some(block.clone()),
        script.to_array(),
    );

    engine.execute().expect("execute");
    assert_eq!(engine.state(), VMState::HALT);

    let item = engine.result_stack().peek(0).expect("result item");
    let iterator = item
        .as_interface::<IteratorInterop>()
        .expect("iterator interface");
    let iter_id = iterator.id();
    let storage_iter = engine
        .get_storage_iterator_mut(iter_id)
        .expect("storage iterator");
    assert!(storage_iter.next());
    let value = storage_iter.value();
    let bytes = value.as_bytes().expect("bytes");
    let account = UInt160::from_bytes(&bytes).expect("account");
    assert_eq!(account, UInt160::zero());
}

#[test]
fn test_white_list_fee() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let neo = NeoToken::new();
    let committee = committee_address(&settings, snapshot.as_ref());

    let mut script = ScriptBuilder::new();
    emit_dynamic_call(
        &mut script,
        &neo.hash(),
        "balanceOf",
        &[committee.to_bytes()],
    );
    script.emit_opcode(OpCode::RET);
    let script = script.to_array();

    // Not whitelisted.
    let mut engine = make_engine_with_script(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
        script.clone(),
    );
    engine.execute().expect("execute");
    assert_eq!(engine.state(), VMState::HALT);
    let result = engine
        .result_stack()
        .peek(0)
        .expect("result")
        .as_int()
        .expect("int");
    assert!(result.is_zero());
    assert_eq!(engine.fee_consumed(), 2_028_330);
    assert_eq!(
        policy
            .clean_whitelist(
                &mut engine,
                &ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &neo.hash())
                    .expect("contract")
                    .expect("state"),
            )
            .expect("clean whitelist"),
        0
    );
    assert!(engine.notifications().is_empty());

    // Whitelist.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    engine
        .call_native_contract(
            policy.hash(),
            "setWhitelistFeeContract",
            &[
                neo.hash().to_bytes(),
                b"balanceOf".to_vec(),
                int_arg_u32(1),
                int_arg_i64(0),
            ],
        )
        .expect("setWhitelistFeeContract");

    assert_eq!(engine.notifications().len(), 1);

    // Whitelisted.
    let mut engine = make_engine_with_script(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
        script.clone(),
    );
    engine.execute().expect("execute");
    assert_eq!(engine.state(), VMState::HALT);
    let result = engine
        .result_stack()
        .peek(0)
        .expect("result")
        .as_int()
        .expect("int");
    assert!(result.is_zero());
    assert_eq!(engine.fee_consumed(), 1_045_260);

    // Clean whitelist.
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    let count = policy
        .clean_whitelist(
            &mut engine,
            &ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &neo.hash())
                .expect("contract")
                .expect("state"),
        )
        .expect("clean whitelist");
    assert_eq!(count, 1);
    assert_eq!(engine.notifications().len(), 1);
}

#[test]
fn test_set_whitelist_fee_contract_negative_fixed_fee() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();

    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setWhitelistFeeContract",
        &[
            NeoToken::new().hash().to_bytes(),
            b"balanceOf".to_vec(),
            int_arg_u32(1),
            int_arg_i64(-1),
        ],
    );
    assert!(result.is_err());
}

#[test]
fn test_set_whitelist_fee_contract_when_contract_not_found() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    let random_hash = UInt160::from_bytes(&[1u8; 20]).expect("hash");
    let result = engine.call_native_contract(
        policy.hash(),
        "setWhitelistFeeContract",
        &[
            random_hash.to_bytes(),
            b"transfer".to_vec(),
            int_arg_u32(3),
            int_arg_i64(10),
        ],
    );
    assert!(result.is_err());
}

#[test]
fn test_set_whitelist_fee_contract_when_contract_not_in_abi() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setWhitelistFeeContract",
        &[
            NeoToken::new().hash().to_bytes(),
            b"noexists".to_vec(),
            int_arg_u32(0),
            int_arg_i64(10),
        ],
    );
    assert!(result.is_err());
}

#[test]
fn test_set_whitelist_fee_contract_when_arg_count_mismatch() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setWhitelistFeeContract",
        &[
            NeoToken::new().hash().to_bytes(),
            b"transfer".to_vec(),
            int_arg_u32(0),
            int_arg_i64(10),
        ],
    );
    assert!(result.is_err());
}

#[test]
fn test_set_whitelist_fee_contract_when_not_committee() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)],
        None,
    );
    let result = engine.call_native_contract(
        policy.hash(),
        "setWhitelistFeeContract",
        &[
            NeoToken::new().hash().to_bytes(),
            b"transfer".to_vec(),
            int_arg_u32(4),
            int_arg_i64(10),
        ],
    );
    assert!(result.is_err());
}

#[test]
fn test_set_whitelist_fee_contract_set_contract() {
    let settings = settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let policy = PolicyContract::new();
    let committee = committee_address(&settings, snapshot.as_ref());
    let mut engine = make_engine(
        Arc::clone(&snapshot),
        settings.clone(),
        vec![Signer::new(committee, WitnessScope::GLOBAL)],
        None,
    );

    engine
        .call_native_contract(
            policy.hash(),
            "setWhitelistFeeContract",
            &[
                NeoToken::new().hash().to_bytes(),
                b"balanceOf".to_vec(),
                int_arg_u32(1),
                int_arg_i64(123_456),
            ],
        )
        .expect("setWhitelistFeeContract");

    let fixed_fee = policy
        .get_whitelisted_fee(snapshot.as_ref(), &NeoToken::new().hash(), "balanceOf", 1)
        .expect("get whitelisted fee");
    assert_eq!(fixed_fee, Some(123_456));
}
