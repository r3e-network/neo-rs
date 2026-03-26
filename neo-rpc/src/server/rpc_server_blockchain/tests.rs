use super::*;
use crate::client::models::RpcRawMemPool;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_core::extensions::io::serializable::SerializableExtensions;
use neo_core::ledger::block::Block as LedgerBlock;
use neo_core::ledger::block_header::BlockHeader as LedgerBlockHeader;
use neo_core::ledger::VerifyResult;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::block::Block;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;
use neo_core::smart_contract::native::GasToken;
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{BinarySerializer, ContractManifest, ContractState, NefFile};
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::wallets::KeyPair;
use neo_core::{IVerifiable, NeoSystem, UInt160, UInt256, Witness as LedgerWitness, WitnessScope};
use neo_json::JToken;
use neo_vm::op_code::OpCode;
use neo_vm::vm_state::VMState;
use num_bigint::BigInt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

fn parse_object(value: &Value) -> neo_json::JObject {
    let json = serde_json::to_string(value).expect("serialize");
    let token = JToken::parse(&json, 128).expect("parse");
    token.as_object().cloned().expect("expected JSON object")
}

fn build_signed_transaction(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(1_0000_0000);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn mint_gas(
    store: &mut neo_core::persistence::StoreCache,
    settings: &ProtocolSettings,
    account: UInt160,
    amount: BigInt,
) {
    let snapshot = Arc::new(store.data_cache().clone());
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        settings.clone(),
        400_000_000,
        None,
    )
    .expect("engine");

    let gas = GasToken::new();
    gas.mint(&mut engine, &account, &amount, false)
        .expect("mint");
}

fn make_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(1_0000_0000);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[7u8; 20]).expect("account"),
        WitnessScope::GLOBAL,
    )]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn make_ledger_block(
    store: &neo_core::persistence::StoreCache,
    index: u32,
    transactions: Vec<Transaction>,
) -> LedgerBlock {
    let ledger = LedgerContract::new();
    let prev_hash = ledger.current_hash(store).unwrap_or_default();

    let merkle_root = if transactions.is_empty() {
        UInt256::zero()
    } else {
        let hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();
        neo_core::cryptography::MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
    };

    let header = LedgerBlockHeader {
        index,
        previous_hash: prev_hash,
        merkle_root,
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        next_consensus: UInt160::zero(),
        witnesses: vec![LedgerWitness::empty()],
        ..Default::default()
    };

    LedgerBlock::new(header, transactions)
}

fn store_block(store: &mut neo_core::persistence::StoreCache, block: &LedgerBlock) {
    const PREFIX_BLOCK: u8 = 0x05;
    const PREFIX_BLOCK_HASH: u8 = 0x09;
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const PREFIX_CURRENT_BLOCK: u8 = 0x0c;
    const RECORD_KIND_TRANSACTION: u8 = 0x01;

    let hash = block.hash();
    let index = block.index();

    let mut hash_key_bytes = Vec::with_capacity(1 + 4);
    hash_key_bytes.push(PREFIX_BLOCK_HASH);
    hash_key_bytes.extend_from_slice(&index.to_le_bytes());
    let hash_key = StorageKey::new(LedgerContract::ID, hash_key_bytes);
    store.add(hash_key, StorageItem::from_bytes(hash.to_bytes().to_vec()));

    let trimmed = TrimmedBlock::from_block(block);
    let trimmed_bytes = trimmed.to_array().expect("serialize trimmed block");
    let mut block_key_bytes = Vec::with_capacity(1 + 32);
    block_key_bytes.push(PREFIX_BLOCK);
    block_key_bytes.extend_from_slice(&hash.to_bytes());
    let block_key = StorageKey::new(LedgerContract::ID, block_key_bytes);
    store.add(block_key, StorageItem::from_bytes(trimmed_bytes));

    for tx in &block.transactions {
        let mut writer = BinaryWriter::new();
        writer
            .write_u8(RECORD_KIND_TRANSACTION)
            .expect("record kind");
        writer.write_u32(index).expect("block index");
        writer.write_u8(VMState::NONE as u8).expect("vm state");
        writer.write_var_bytes(&tx.to_bytes()).expect("tx bytes");

        let mut tx_key_bytes = Vec::with_capacity(1 + 32);
        tx_key_bytes.push(PREFIX_TRANSACTION);
        tx_key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let tx_key = StorageKey::new(LedgerContract::ID, tx_key_bytes);
        store.add(tx_key, StorageItem::from_bytes(writer.into_bytes()));
    }

    let mut current_bytes = Vec::with_capacity(36);
    current_bytes.extend_from_slice(&hash.to_bytes());
    current_bytes.extend_from_slice(&index.to_le_bytes());
    let current_key = StorageKey::new(LedgerContract::ID, vec![PREFIX_CURRENT_BLOCK]);
    store.add(current_key, StorageItem::from_bytes(current_bytes));
    store.commit();
}

fn store_contract_state(store: &mut neo_core::persistence::StoreCache, contract: &ContractState) {
    const PREFIX_CONTRACT: u8 = 0x08;
    const PREFIX_CONTRACT_HASH: u8 = 0x0c;

    let contract_mgmt_id = NativeRegistry::new()
        .get_by_name("ContractManagement")
        .expect("contract management")
        .id();

    let mut writer = BinaryWriter::new();
    contract.serialize(&mut writer).expect("serialize contract");

    let mut key_bytes = Vec::with_capacity(1 + 20);
    key_bytes.push(PREFIX_CONTRACT);
    key_bytes.extend_from_slice(&contract.hash.to_bytes());
    let key = StorageKey::new(contract_mgmt_id, key_bytes);
    store.add(key, StorageItem::from_bytes(writer.into_bytes()));

    let mut id_bytes = Vec::with_capacity(1 + 4);
    id_bytes.push(PREFIX_CONTRACT_HASH);
    id_bytes.extend_from_slice(&contract.id.to_be_bytes());
    let id_key = StorageKey::new(contract_mgmt_id, id_bytes);
    store.add(
        id_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );

    let mut legacy_bytes = Vec::with_capacity(1 + 4);
    legacy_bytes.push(PREFIX_CONTRACT_HASH);
    legacy_bytes.extend_from_slice(&contract.id.to_le_bytes());
    let legacy_key = StorageKey::new(contract_mgmt_id, legacy_bytes);
    store.add(
        legacy_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );

    store.commit();
}

fn store_storage_item(
    store: &mut neo_core::persistence::StoreCache,
    contract_id: i32,
    key: &[u8],
    value: &[u8],
) {
    let storage_key = StorageKey::new(contract_id, key.to_vec());
    store.add(storage_key, StorageItem::from_bytes(value.to_vec()));
    store.commit();
}

fn store_committee(
    store: &mut neo_core::persistence::StoreCache,
    committee: &[neo_core::cryptography::ECPoint],
) {
    const PREFIX_COMMITTEE: u8 = 0x0e;
    let neo_token_id = NativeRegistry::new()
        .get_by_name("NeoToken")
        .expect("neo token")
        .id();

    let items: Vec<neo_vm::StackItem> = committee
        .iter()
        .map(|pk| {
            neo_vm::StackItem::from_struct(vec![
                neo_vm::StackItem::from_byte_string(pk.as_bytes().to_vec()),
                neo_vm::StackItem::from_int(BigInt::from(0)),
            ])
        })
        .collect();
    let array = neo_vm::StackItem::from_array(items);
    let bytes = BinarySerializer::serialize(&array, &neo_vm::ExecutionEngineLimits::default())
        .expect("serialize committee");
    let key = StorageKey::create(neo_token_id, PREFIX_COMMITTEE);
    store.add(key, StorageItem::from_bytes(bytes));
    store.commit();
}

fn store_candidate_state(
    store: &mut neo_core::persistence::StoreCache,
    candidate: &neo_core::cryptography::ECPoint,
    registered: bool,
    votes: BigInt,
) {
    let item = neo_vm::StackItem::from_struct(vec![
        neo_vm::StackItem::from_bool(registered),
        neo_vm::StackItem::from_int(votes),
    ]);
    let bytes = BinarySerializer::serialize(&item, &neo_vm::ExecutionEngineLimits::default())
        .expect("serialize candidate");
    store_candidate_state_raw(store, candidate, bytes);
}

fn store_candidate_state_raw(
    store: &mut neo_core::persistence::StoreCache,
    candidate: &neo_core::cryptography::ECPoint,
    bytes: Vec<u8>,
) {
    const PREFIX_CANDIDATE: u8 = 0x21;
    let neo_token_id = NativeRegistry::new()
        .get_by_name("NeoToken")
        .expect("neo token")
        .id();
    let mut key_bytes = Vec::with_capacity(1 + candidate.as_bytes().len());
    key_bytes.push(PREFIX_CANDIDATE);
    key_bytes.extend_from_slice(candidate.as_bytes());
    let key = StorageKey::new(neo_token_id, key_bytes);
    store.add(key, StorageItem::from_bytes(bytes));
    store.commit();
}

fn store_blocked_account(store: &mut neo_core::persistence::StoreCache, account: &UInt160) {
    const PREFIX_BLOCKED_ACCOUNT: u8 = 0x0f;
    let policy_id = NativeRegistry::new()
        .get_by_name("PolicyContract")
        .expect("policy")
        .id();
    let key = StorageKey::create_with_uint160(policy_id, PREFIX_BLOCKED_ACCOUNT, account);
    store.add(key, StorageItem::from_bytes(vec![1u8]));
    store.commit();
}

fn make_contract_state(id: i32, hash: UInt160, name: &str) -> ContractState {
    let nef = NefFile::new("test".to_string(), vec![OpCode::PUSH1 as u8]);
    let manifest = ContractManifest::new(name.to_string());
    ContractState::new(id, hash, nef, manifest)
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_defaults_to_verified_hashes() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let result = (handler.callback())(&server, &[]).expect("getrawmempool");
    let array = result.as_array().expect("array result");
    assert!(array.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_verbose_roundtrips_into_client_model() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let params = [Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("getrawmempool verbose");
    let parsed = RpcRawMemPool::from_json(&parse_object(&result)).expect("parse mempool");
    assert!(parsed.verified.is_empty());
    assert!(parsed.unverified.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_mixed_verified_and_unverified() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let keypair_a = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair a");
    let keypair_b = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair b");
    let keypair_c = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair c");

    let account_a = keypair_a.get_script_hash();
    let account_b = keypair_b.get_script_hash();
    let account_c = keypair_c.get_script_hash();

    let mut store = system.context().store_snapshot_cache();
    let funded = BigInt::from(50_0000_0000i64);
    mint_gas(&mut store, &settings, account_a, funded.clone());
    mint_gas(&mut store, &settings, account_b, funded.clone());
    mint_gas(&mut store, &settings, account_c, funded);
    store.commit();

    let tx1 = build_signed_transaction(&settings, &keypair_a, 1);
    let tx2 = build_signed_transaction(&settings, &keypair_b, 2);
    let tx3 = build_signed_transaction(&settings, &keypair_c, 3);

    let pool_arc = system.mempool();
    {
        let mut pool = pool_arc.lock();
        assert_eq!(
            pool.try_add(tx1.clone(), store.data_cache(), &settings),
            VerifyResult::Succeed
        );
        assert_eq!(
            pool.try_add(tx2.clone(), store.data_cache(), &settings),
            VerifyResult::Succeed
        );

        let mut block = Block::new();
        block.header.set_index(1);
        pool.update_pool_for_block_persisted(&block, store.data_cache(), &settings, true);

        assert_eq!(
            pool.try_add(tx3.clone(), store.data_cache(), &settings),
            VerifyResult::Succeed
        );
    }

    let params = [Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("getrawmempool verbose");
    let parsed = RpcRawMemPool::from_json(&parse_object(&result)).expect("parse mempool");

    let verified_hashes: HashSet<String> = parsed
        .verified
        .iter()
        .map(|hash| hash.to_string())
        .collect();
    let unverified_hashes: HashSet<String> = parsed
        .unverified
        .iter()
        .map(|hash| hash.to_string())
        .collect();

    assert!(verified_hashes.contains(&tx3.hash().to_string()));
    assert!(unverified_hashes.contains(&tx1.hash().to_string()));
    assert!(unverified_hashes.contains(&tx2.hash().to_string()));
    assert_eq!(verified_hashes.len(), 1);
    assert_eq!(unverified_hashes.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_best_block_hash_reflects_current_state() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getbestblockhash");

    let mut store = system.context().store_snapshot_cache();
    let hash = UInt256::zero();
    let index = 100u32;
    let mut current_bytes = Vec::with_capacity(36);
    current_bytes.extend_from_slice(&hash.to_bytes());
    current_bytes.extend_from_slice(&index.to_le_bytes());
    let key = StorageKey::new(LedgerContract::ID, vec![0x0c]);
    store.add(key, StorageItem::from_bytes(current_bytes));
    store.commit();

    let result = (handler.callback())(&server, &[]).expect("get best block hash");
    assert_eq!(result.as_str().expect("hash"), hash.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_count_defaults_to_one() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockcount");

    let result = (handler.callback())(&server, &[]).expect("get block count");
    assert_eq!(result.as_u64().unwrap_or_default(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_count_defaults_to_one() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheadercount");

    let result = (handler.callback())(&server, &[]).expect("get block header count");
    assert_eq!(result.as_u64().unwrap_or_default(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_sums_transaction_fees() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let mut tx1 = make_transaction(1);
    tx1.set_system_fee(100_000_000);
    let mut tx2 = make_transaction(2);
    tx2.set_system_fee(200_000_000);
    let block = make_ledger_block(&system.context().store_cache(), 100, vec![tx1, tx2]);
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(100u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block sys fee");
    assert_eq!(result.as_str().expect("sys fee"), "300000000");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_rejects_invalid_param() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let params = [Value::String("not-a-number".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid params");
    assert_eq!(err.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_reports_unknown_height() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let params = [Value::Number(1u32.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown height");
    assert_eq!(err.code(), RpcError::unknown_height().code());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Store isolation issue - snapshot cache commits not visible to store_cache reads"]
async fn get_block_hash_reports_hash_for_height() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockhash");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(1)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block hash");
    assert_eq!(result.as_str().expect("hash"), block.hash().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_roundtrips_by_hash_and_index() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(1)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let hash_params = [Value::String(block.hash().to_string())];
    let result = (handler.callback())(&server, &hash_params).expect("get block by hash");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let mut decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&mut decoded_clone), block.hash());

    let index_params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &index_params).expect("get block by index");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let mut decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&mut decoded_clone), block.hash());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_genesis_roundtrips_and_reports_empty_txs() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let genesis = system.genesis_block();
    let genesis_hash = genesis.hash().expect("genesis hash");

    let params = [Value::Number(0u32.into())];
    let result = (handler.callback())(&server, &params).expect("get genesis block");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let mut decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&mut decoded_clone), genesis_hash);
    assert!(decoded.transactions.is_empty());

    let params = [Value::Number(0u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get genesis verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        genesis_hash.to_string()
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert!(txs.is_empty());
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_no_transactions_reports_empty_txs() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(&system.context().store_cache(), 1, Vec::new());
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let mut decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&mut decoded_clone), block.hash());
    assert!(decoded.transactions.is_empty());

    let params = [Value::Number(1u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert!(txs.is_empty());
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_verbose_reports_confirmations() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(2)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert_eq!(txs.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_rejects_null_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_roundtrips() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheader");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(3)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::String(block.hash().to_string())];
    let result = (handler.callback())(&server, &params).expect("get block header");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Header as Serializable>::deserialize(&mut reader).expect("header");
    assert_eq!(decoded.index(), 1);

    let params = [Value::String(block.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block header verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_rejects_null_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheader");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_roundtrips_hash_and_id() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let hash = UInt160::from_bytes(&[0x01u8; 20]).expect("hash");
    let contract = make_contract_state(42, hash, "TestContract");
    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &contract);

    let params = [Value::String(hash.to_string())];
    let result = (handler.callback())(&server, &params).expect("get contract");
    assert_eq!(result, contract_state_to_json(&contract));

    let params = [Value::Number(42i64.into())];
    let result = (handler.callback())(&server, &params).expect("get contract by id");
    assert_eq!(result, contract_state_to_json(&contract));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_roundtrips_native_name_and_id() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let registry = NativeRegistry::new();
    let contract = registry
        .get_by_name("ContractManagement")
        .expect("contract management");
    let state = contract
        .contract_state(&settings, 0)
        .expect("contract state");

    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &state);

    let params = [Value::Number(state.id.into())];
    let result_by_id = (handler.callback())(&server, &params).expect("get by id");

    let params = [Value::String("ContractManagement".to_string())];
    let result_by_name = (handler.callback())(&server, &params).expect("get by name");
    assert_eq!(result_by_id, result_by_name);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_resolves_native_case_insensitive() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let registry = NativeRegistry::new();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(system.settings(), 0)
        .expect("gas state");
    let gas_hash = gas_state.hash;

    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &gas_state);

    for name in ["gastoken", "GASTOKEN", "GasToken"] {
        let params = [Value::String(name.to_string())];
        let result = (handler.callback())(&server, &params).expect("get gas state");
        let obj = result.as_object().expect("object");
        assert_eq!(
            obj.get("hash").and_then(Value::as_str).unwrap_or_default(),
            gas_hash.to_string()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_unknown_contract() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::String(
        UInt160::from_bytes(&[0x22u8; 20])
            .expect("hash")
            .to_string(),
    )];
    let err = (handler.callback())(&server, &params).expect_err("unknown contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_contract().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_invalid_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::String("0xInvalidHashString".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [Value::String("InvalidContractName".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid name");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_null_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_from_mempool() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x21u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction(&settings, &keypair, 1);
    let pool = system.mempool();
    {
        let mut pool = pool.lock();
        assert_eq!(
            pool.try_add(tx.clone(), store.data_cache(), &settings),
            VerifyResult::Succeed
        );
    }

    let params = [Value::String(tx.hash().to_string()), Value::Bool(false)];
    let result = (handler.callback())(&server, &params).expect("get raw tx");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap_or_default(),
        tx.hash().to_string()
    );
    assert!(obj.get("blockhash").is_none());
    assert!(obj.get("sysfee").is_some());
    assert!(obj.get("netfee").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_confirmed_in_block() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let tx = make_transaction(7);
    let block = make_ledger_block(&system.context().store_cache(), 1, vec![tx.clone()]);
    let block_hash = block.hash();
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::String(tx.hash().to_string()), Value::Bool(false)];
    let result = (handler.callback())(&server, &params).expect("get raw tx");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("blockhash")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        block_hash.to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        obj.get("blocktime")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    assert!(obj.get("sysfee").is_some());
    assert!(obj.get("netfee").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_rejects_unknown_hash() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let params = [Value::String(UInt256::from([0x99u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_rejects_null_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_roundtrips_value() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let hash = UInt160::from_bytes(&[0x10u8; 20]).expect("hash");
    let contract = make_contract_state(100, hash, "StorageTest");
    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &contract);
    store_storage_item(&mut store, contract.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [Value::String(hash.to_string()), Value::String(key_b64)];
    let result = (handler.callback())(&server, &params).expect("get storage");
    assert_eq!(
        result.as_str().unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_accepts_native_contract_name() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let registry = NativeRegistry::new();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(system.settings(), 0)
        .expect("gas state");

    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &gas_state);
    store_storage_item(&mut store, gas_state.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [
        Value::String("GasToken".to_string()),
        Value::String(key_b64),
    ];
    let result = (handler.callback())(&server, &params).expect("get storage");
    assert_eq!(
        result.as_str().unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_accepts_native_contract_name() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let registry = NativeRegistry::new();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(system.settings(), 0)
        .expect("gas state");

    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &gas_state);
    store_storage_item(&mut store, gas_state.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [
        Value::String("GasToken".to_string()),
        Value::String(key_b64),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage");
    let obj = result.as_object().expect("object");
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    let first = results.first().and_then(Value::as_object).expect("entry");
    assert_eq!(
        first
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_rejects_unknown_contract_or_key() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x11u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::String(BASE64_STANDARD.encode([0x01u8])),
    ];
    let err = (handler.callback())(&server, &params).expect_err("unknown contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_contract().code());

    let hash = UInt160::from_bytes(&[0x12u8; 20]).expect("hash");
    let contract = make_contract_state(101, hash, "StorageTest2");
    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &contract);

    let params = [
        Value::String(hash.to_string()),
        Value::String(BASE64_STANDARD.encode([0x01u8])),
    ];
    let err = (handler.callback())(&server, &params).expect_err("unknown storage item");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_rejects_null_params() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let params = [Value::Null, Value::String(BASE64_STANDARD.encode([0x01u8]))];
    let err = (handler.callback())(&server, &params).expect_err("null contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x13u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::Null,
    ];
    let err = (handler.callback())(&server, &params).expect_err("null key");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_paginates_results() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let config = RpcServerConfig {
        find_storage_page_size: 10,
        ..Default::default()
    };
    let server = RpcServer::new(system.clone(), config);
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let hash = UInt160::from_bytes(&[0x20u8; 20]).expect("hash");
    let contract = make_contract_state(200, hash, "FindStorage");
    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &contract);

    let page_size = server.settings().find_storage_page_size;
    let total_items = page_size + 5;
    for i in 0..total_items {
        let key = vec![0xAA, i as u8];
        let value = vec![i as u8];
        store.add(
            StorageKey::new(contract.id, key),
            StorageItem::from_bytes(value),
        );
    }
    store.commit();

    let prefix = BASE64_STANDARD.encode([0xAAu8]);
    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page1");
    let obj = result.as_object().expect("object");
    assert!(obj
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false));
    assert_eq!(
        obj.get("results")
            .and_then(Value::as_array)
            .map(|v| v.len())
            .unwrap_or_default(),
        page_size
    );
    let next = obj.get("next").and_then(Value::as_u64).unwrap_or_default() as usize;
    assert_eq!(next, page_size);

    let params = [
        Value::String(hash.to_string()),
        Value::String(BASE64_STANDARD.encode([0xAAu8])),
        Value::Number((next as u64).into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page2");
    let obj = result.as_object().expect("object");
    println!("page2 result: {}", result);
    assert!(!obj
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(true));
    assert_eq!(
        obj.get("results")
            .and_then(Value::as_array)
            .map(|v| v.len())
            .unwrap_or_default(),
        5
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_returns_empty_page_at_end() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let hash = UInt160::from_bytes(&[0x21u8; 20]).expect("hash");
    let contract = make_contract_state(201, hash, "FindStorageEnd");
    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &contract);

    let prefix = [0xBBu8];
    for i in 0..3u8 {
        let key = vec![prefix[0], i];
        let value = vec![i];
        store.add(
            StorageKey::new(contract.id, key),
            StorageItem::from_bytes(value),
        );
    }
    store.commit();

    let prefix_b64 = BASE64_STANDARD.encode(prefix);
    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix_b64.clone()),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page1");
    let obj = result.as_object().expect("object");
    assert!(!obj
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(true));
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results.len(), 3);
    let next = obj.get("next").and_then(Value::as_u64).unwrap_or_default();
    assert_eq!(next, 3);

    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix_b64),
        Value::Number(next.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page2");
    let obj = result.as_object().expect("object");
    assert!(!obj
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(true));
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert!(results.is_empty());
    let next_end = obj.get("next").and_then(Value::as_u64).unwrap_or_default();
    assert_eq!(next_end, next);
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_rejects_null_params() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let params = [
        Value::Null,
        Value::String(BASE64_STANDARD.encode([0x01u8])),
        Value::Number(0u64.into()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("null contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x30u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::Null,
    ];
    let err = (handler.callback())(&server, &params).expect_err("null prefix");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_reports_confirmed_height() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let tx = make_transaction(9);
    let block = make_ledger_block(&system.context().store_cache(), 2, vec![tx.clone()]);
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::String(tx.hash().to_string())];
    let result = (handler.callback())(&server, &params).expect("transaction height");
    assert_eq!(result.as_u64().unwrap_or_default(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_mempool_transaction() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let keypair = KeyPair::from_private_key(&[0x23u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction(&settings, &keypair, 1);
    let pool = system.mempool();
    {
        let mut pool = pool.lock();
        assert_eq!(
            pool.try_add(tx.clone(), store.data_cache(), &settings),
            VerifyResult::Succeed
        );
    }

    let params = [Value::String(tx.hash().to_string())];
    let err = (handler.callback())(&server, &params).expect_err("mempool tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_unknown_transaction() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let params = [Value::String(UInt256::from([0x44u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_null_identifier() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_returns_standby() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    assert_eq!(array.len(), settings.validators_count as usize);
    let expected: std::collections::HashSet<String> = settings
        .standby_validators()
        .into_iter()
        .map(|validator| hex::encode(validator.as_bytes()))
        .collect();
    let received: std::collections::HashSet<String> = array
        .iter()
        .filter_map(|item| {
            item.as_object()?
                .get("publickey")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect();
    assert_eq!(expected, received);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_reports_candidate_votes() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.context().store_snapshot_cache();
    store_candidate_state(&mut store, &candidate, true, BigInt::from(42));

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        42
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_reports_unregistered_as_negative_one() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.context().store_snapshot_cache();
    store_candidate_state(&mut store, &candidate, false, BigInt::from(11));

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        -1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_reports_registered_candidate() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.context().store_snapshot_cache();
    store_candidate_state(&mut store, &candidate, true, BigInt::from(10_000));

    let result = (handler.callback())(&server, &[]).expect("candidates");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "10000"
    );
    assert!(entry
        .get("active")
        .and_then(Value::as_bool)
        .unwrap_or(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_skips_blocked_and_unregistered() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate_active = settings
        .standby_committee
        .first()
        .expect("candidate")
        .clone();
    let candidate_blocked = settings
        .standby_committee
        .get(1)
        .expect("candidate")
        .clone();
    let candidate_unregistered = settings
        .standby_committee
        .get(2)
        .expect("candidate")
        .clone();

    let blocked_account =
        neo_core::smart_contract::Contract::create_signature_contract(candidate_blocked.clone())
            .script_hash();
    let mut store = system.context().store_snapshot_cache();
    store_candidate_state(&mut store, &candidate_active, true, BigInt::from(7));
    store_candidate_state(&mut store, &candidate_blocked, true, BigInt::from(9));
    store_candidate_state(&mut store, &candidate_unregistered, false, BigInt::from(11));
    store_blocked_account(&mut store, &blocked_account);

    let result = (handler.callback())(&server, &[]).expect("candidates");
    let array = result.as_array().expect("array");
    let keys: std::collections::HashSet<String> = array
        .iter()
        .filter_map(|item| {
            item.as_object()?
                .get("publickey")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect();

    assert!(keys.contains(&hex::encode(candidate_active.as_bytes())));
    assert!(!keys.contains(&hex::encode(candidate_blocked.as_bytes())));
    assert!(!keys.contains(&hex::encode(candidate_unregistered.as_bytes())));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_reports_internal_error_on_invalid_state() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate = settings
        .standby_committee
        .first()
        .expect("candidate")
        .clone();
    let invalid_item = neo_vm::StackItem::from_byte_string(vec![0x01]);
    let bytes =
        BinarySerializer::serialize(&invalid_item, &neo_vm::ExecutionEngineLimits::default())
            .expect("serialize invalid");
    let mut store = system.context().store_snapshot_cache();
    store_candidate_state_raw(&mut store, &candidate, bytes);

    let err = (handler.callback())(&server, &[]).expect_err("invalid state");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
    assert_eq!(rpc_error.data(), Some("Can't get candidates."));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_committee_returns_snapshot_members() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcommittee");

    let mut store = system.context().store_snapshot_cache();
    store_committee(&mut store, &settings.standby_committee);

    let result = (handler.callback())(&server, &[]).expect("committee");
    let array = result.as_array().expect("array");
    assert_eq!(array.len(), settings.standby_committee.len());
    let expected = hex::encode(settings.standby_committee[0].as_bytes());
    assert_eq!(array[0].as_str().unwrap_or_default(), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_native_contracts_includes_gas_token() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnativecontracts");

    let registry = NativeRegistry::new();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(&settings, 0)
        .expect("gas state");
    let gas_hash = gas_state.hash;

    let mut store = system.context().store_snapshot_cache();
    store_contract_state(&mut store, &gas_state);

    let result = (handler.callback())(&server, &[]).expect("native contracts");
    let array = result.as_array().expect("array");
    let has_gas = array.iter().any(|entry| {
        entry
            .as_object()
            .and_then(|obj| obj.get("hash").and_then(Value::as_str))
            .map(|hash| hash == gas_hash.to_string())
            .unwrap_or(false)
    });
    assert!(has_gas);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_native_contracts_returns_all_registered_states() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnativecontracts");

    let registry = NativeRegistry::new();
    let store = system.context().store_cache();
    let mut expected = Vec::new();
    for contract in registry.contracts() {
        if let Some(state) =
            ContractManagement::get_contract_from_store_cache(&store, &contract.hash())
                .expect("contract read")
        {
            expected.push(contract_state_to_json(&state));
        }
    }

    let result = (handler.callback())(&server, &[]).expect("native contracts");
    let result_array = result.as_array().expect("array");
    assert_eq!(result_array.len(), expected.len());

    let expected_by_hash: HashMap<String, Value> = expected
        .into_iter()
        .map(|value| {
            let hash = value
                .as_object()
                .and_then(|obj| obj.get("hash").and_then(Value::as_str))
                .expect("hash present")
                .to_string();
            (hash, value)
        })
        .collect();

    for value in result_array {
        let hash = value
            .as_object()
            .and_then(|obj| obj.get("hash").and_then(Value::as_str))
            .expect("hash present");
        let expected_value = expected_by_hash
            .get(hash)
            .unwrap_or_else(|| panic!("missing expected contract {}", hash));
        assert_eq!(value, expected_value);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_reports_unknown_block() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(5)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(999u64.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown index");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_block().code());

    let params = [Value::String(UInt256::from([0x55u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_block().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_hash_rejects_unknown_height() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockhash");

    let block = make_ledger_block(
        &system.context().store_cache(),
        1,
        vec![make_transaction(6)],
    );
    let mut store = system.context().store_snapshot_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(2u64.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown height");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_height().code());
}
