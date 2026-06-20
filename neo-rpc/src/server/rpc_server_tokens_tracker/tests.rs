use super::*;
use crate::plugins::tokens_tracker::{
    Nep11TransferKey, Nep17TransferKey, TokenTransfer, TokensTrackerService, TokensTrackerSettings,
    find_range,
};
use crate::server::rpc_server::RpcHandler;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_execution::ContractState;
use neo_io::{Serializable, SerializableExtensions};
use neo_manifest::NefFile;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_native_contracts::GasToken;
use neo_native_contracts::NativeContract;
use neo_primitives::ContractParameterType;
use neo_primitives::UInt256;
use neo_storage::persistence::Store;
use neo_storage::persistence::StoreProvider;
use neo_storage::persistence::providers::MemoryStoreProvider;
use neo_storage::{StorageItem, StorageKey};
use neo_system::Node;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("handler {} not found", name))
}

fn create_tracker_store() -> Arc<dyn Store> {
    let provider = MemoryStoreProvider::new();
    provider
        .get_store("tokens")
        .expect("memory store available")
}

fn write_tracker_entry<K, V>(store: &Arc<dyn Store>, prefix: u8, key: &K, value: &V)
where
    K: Serializable,
    V: Serializable,
{
    let mut key_bytes = Vec::with_capacity(1 + key.size());
    key_bytes.push(prefix);
    key_bytes.extend_from_slice(&key.to_array().expect("serialize key"));
    let value_bytes = value.to_array().expect("serialize value");

    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("unique snapshot");
    snapshot
        .put(key_bytes, value_bytes)
        .expect("storage put failed");
    snapshot.commit();
}

fn attach_tokens_tracker(
    system: &Arc<Node>,
    store: Arc<dyn Store>,
    enabled_trackers: Vec<String>,
    track_history: bool,
) {
    let mut settings = TokensTrackerSettings::default();
    settings.enabled_trackers = enabled_trackers;
    settings.track_history = track_history;
    settings.network = system.settings().network;

    let service = Arc::new(TokensTrackerService::new(settings, store));
    system.register_service(Arc::clone(&service));
}

fn store_contract_state(system: &Arc<Node>, contract: &ContractState) {
    const PREFIX_CONTRACT: u8 = 0x08;
    const PREFIX_CONTRACT_HASH: u8 = 0x0c;

    let contract_mgmt_id = crate::server::native_queries::NativeQueries::native_registry()
        .get_by_name("ContractManagement")
        .expect("contract management")
        .id();

    let record = contract
        .serialize_contract_record()
        .expect("serialize contract record");

    let mut store_cache = system.store_cache();
    let mut key_bytes = Vec::with_capacity(1 + 20);
    key_bytes.push(PREFIX_CONTRACT);
    key_bytes.extend_from_slice(&contract.hash.to_bytes());
    let key = StorageKey::new(contract_mgmt_id, key_bytes);
    store_cache.add(key, StorageItem::from_bytes(record));

    let mut id_bytes = Vec::with_capacity(1 + 4);
    id_bytes.push(PREFIX_CONTRACT_HASH);
    id_bytes.extend_from_slice(&contract.id.to_be_bytes());
    let id_key = StorageKey::new(contract_mgmt_id, id_bytes);
    store_cache.add(
        id_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );

    let mut legacy_bytes = Vec::with_capacity(1 + 4);
    legacy_bytes.push(PREFIX_CONTRACT_HASH);
    legacy_bytes.extend_from_slice(&contract.id.to_le_bytes());
    let legacy_key = StorageKey::new(contract_mgmt_id, legacy_bytes);
    store_cache.add(
        legacy_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );
    store_cache.commit();
}

fn emit_map_entry_string(builder: &mut ScriptBuilder, key: &str, value: &str) {
    builder.emit_opcode(OpCode::DUP);
    builder.emit_push_string(key);
    builder.emit_push_string(value);
    builder.emit_opcode(OpCode::SETITEM);
}

fn emit_map_entry_null(builder: &mut ScriptBuilder, key: &str) {
    builder.emit_opcode(OpCode::DUP);
    builder.emit_push_string(key);
    builder.emit_opcode(OpCode::PUSHNULL);
    builder.emit_opcode(OpCode::SETITEM);
}

fn emit_map_entry_bytes(builder: &mut ScriptBuilder, key: &str, value: &[u8]) {
    builder.emit_opcode(OpCode::DUP);
    builder.emit_push_string(key);
    builder.emit_push_byte_array(value);
    builder.emit_opcode(OpCode::SETITEM);
}

fn build_nep11_properties_contract() -> ContractState {
    let mut script = ScriptBuilder::new();
    script.emit_opcode(OpCode::DROP);
    script.emit_opcode(OpCode::NEWMAP);
    emit_map_entry_string(&mut script, "name", "Example NFT");
    emit_map_entry_string(&mut script, "image", "ipfs://example");
    emit_map_entry_null(&mut script, "tokenURI");
    emit_map_entry_bytes(&mut script, "extra", &[1u8, 2, 3]);
    script.emit_opcode(OpCode::RET);

    let nef = NefFile::new("nep11-properties".to_string(), script.to_array());
    let mut manifest = ContractManifest::new("Nep11Properties".to_string());
    manifest.supported_standards.push("NEP-11".to_string());

    let parameter =
        ContractParameterDefinition::new("tokenId".to_string(), ContractParameterType::ByteArray)
            .expect("parameter");
    let method = ContractMethodDescriptor::new(
        "properties".to_string(),
        vec![parameter],
        ContractParameterType::Map,
        0,
        true,
    )
    .expect("method");
    manifest.abi = ContractAbi::new(vec![method], Vec::new());

    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, &manifest.name);
    ContractState::new(9, hash, nef, manifest)
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep17_balances_reports_asset_metadata() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(
        &system,
        Arc::clone(&store),
        vec!["NEP-17".to_string()],
        true,
    );

    let gas_token = GasToken::new();
    let asset = gas_token.hash();
    let contract = gas_token
        .contract_state(&system.settings(), 0)
        .expect("gas contract");
    store_contract_state(&system, &contract);
    let store_cache = system.store_cache();
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let contract_lookup = ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &asset)
        .expect("contract lookup");
    assert!(contract_lookup.is_some());
    let mut script = ScriptBuilder::new();
    emit_contract_call(&mut script, &asset, "decimals").expect("emit decimals");
    emit_contract_call(&mut script, &asset, "symbol").expect("emit symbol");
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot.clone(),
        None,
        system.settings().as_ref().clone(),
        TEST_MODE_GAS,
        None,
    )
    .expect("engine");
    engine
        .load_script(script.to_array(), CallFlags::ALL, Some(asset))
        .expect("load script");
    engine.execute().expect("execute");
    assert_eq!(
        engine.state(),
        VMState::HALT,
        "fault: {:?}",
        engine.fault_exception()
    );
    let result_stack = engine.result_stack();
    let symbol_item = result_stack.peek(0).expect("symbol item");
    let decimals_item = result_stack.peek(1).expect("decimals item");
    let symbol_bytes = symbol_item.as_bytes().expect("symbol bytes");
    let symbol = String::from_utf8(symbol_bytes).expect("symbol utf8");
    let decimals = decimals_item
        .as_integer()
        .expect("decimals integer")
        .to_u32()
        .expect("decimals u32");
    assert_eq!(symbol, "GAS");
    assert_eq!(decimals, 8);
    assert!(query_asset_metadata(snapshot.as_ref(), &system.settings(), &asset).is_some());
    let user = UInt160::from_bytes(&[1u8; 20]).expect("user hash");
    let balance = TokenBalance {
        balance: BigInt::from(42),
        last_updated_block: 7,
    };
    let key = Nep17BalanceKey::new(user, asset);
    let (balance_prefix, _, _) = Nep17Tracker::rpc_prefixes();
    write_tracker_entry(&store, balance_prefix, &key, &balance);
    let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix.push(balance_prefix);
    prefix.extend_from_slice(&user.to_bytes());
    let entries =
        find_prefix::<Nep17BalanceKey, TokenBalance>(store.as_ref(), &prefix).expect("find prefix");
    assert_eq!(entries.len(), 1);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep17balances");

    let address = neo_wallets::wallet_helper::WalletAddress::to_address(
        &user,
        server.system().settings().address_version,
    );
    let params = [Value::String(address.clone())];
    let result = (handler.callback())(&server, &params).expect("getnep17balances");
    let obj = result.as_object().expect("result object");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(address.as_str())
    );

    let balances = obj
        .get("balance")
        .and_then(Value::as_array)
        .expect("balance array");
    assert_eq!(balances.len(), 1);
    let entry = balances[0].as_object().expect("balance entry");
    assert_eq!(
        entry.get("assethash").and_then(Value::as_str),
        Some(asset.to_string().as_str())
    );
    assert_eq!(entry.get("name").and_then(Value::as_str), Some("GasToken"));
    assert_eq!(entry.get("symbol").and_then(Value::as_str), Some("GAS"));
    assert_eq!(entry.get("decimals").and_then(Value::as_str), Some("8"));
    assert_eq!(entry.get("amount").and_then(Value::as_str), Some("42"));
    assert_eq!(
        entry.get("lastupdatedblock").and_then(Value::as_u64),
        Some(7)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep11_balances_groups_tokens_by_asset() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(
        &system,
        Arc::clone(&store),
        vec!["NEP-11".to_string()],
        true,
    );

    let gas_token = GasToken::new();
    let asset = gas_token.hash();
    let contract = gas_token
        .contract_state(&system.settings(), 0)
        .expect("gas contract");
    store_contract_state(&system, &contract);

    let user = UInt160::from_bytes(&[6u8; 20]).expect("user hash");
    let token_a = vec![0x01];
    let token_b = vec![0x02, 0x03];
    let key_a = Nep11BalanceKey::new(user, asset, token_a.clone());
    let key_b = Nep11BalanceKey::new(user, asset, token_b.clone());
    let (balance_prefix, _, _) = Nep11Tracker::rpc_prefixes();

    write_tracker_entry(
        &store,
        balance_prefix,
        &key_a,
        &TokenBalance {
            balance: BigInt::from(5),
            last_updated_block: 10,
        },
    );
    write_tracker_entry(
        &store,
        balance_prefix,
        &key_b,
        &TokenBalance {
            balance: BigInt::from(7),
            last_updated_block: 11,
        },
    );

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep11balances");

    let address = neo_wallets::wallet_helper::WalletAddress::to_address(
        &user,
        server.system().settings().address_version,
    );
    let params = [Value::String(address.clone())];
    let result = (handler.callback())(&server, &params).expect("getnep11balances");
    let obj = result.as_object().expect("result object");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(address.as_str())
    );

    let balances = obj
        .get("balance")
        .and_then(Value::as_array)
        .expect("balance array");
    assert_eq!(balances.len(), 1);
    let entry = balances[0].as_object().expect("balance entry");
    assert_eq!(
        entry.get("assethash").and_then(Value::as_str),
        Some(asset.to_string().as_str())
    );
    assert_eq!(entry.get("name").and_then(Value::as_str), Some("GasToken"));
    assert_eq!(entry.get("symbol").and_then(Value::as_str), Some("GAS"));
    assert_eq!(entry.get("decimals").and_then(Value::as_str), Some("8"));

    let tokens = entry
        .get("tokens")
        .and_then(Value::as_array)
        .expect("tokens array");
    assert_eq!(tokens.len(), 2);
    let mut token_map: HashMap<String, (String, u64)> = HashMap::new();
    for token in tokens {
        let token_obj = token.as_object().expect("token entry");
        let token_id = token_obj
            .get("tokenid")
            .and_then(Value::as_str)
            .expect("tokenid")
            .to_string();
        let amount = token_obj
            .get("amount")
            .and_then(Value::as_str)
            .expect("amount")
            .to_string();
        let last = token_obj
            .get("lastupdatedblock")
            .and_then(Value::as_u64)
            .expect("lastupdatedblock");
        token_map.insert(token_id, (amount, last));
    }

    let token_a_key = hex::encode(&token_a);
    let token_b_key = hex::encode(&token_b);
    assert_eq!(token_map.get(&token_a_key), Some(&(String::from("5"), 10)));
    assert_eq!(token_map.get(&token_b_key), Some(&(String::from("7"), 11)));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep11_transfers_orders_by_timestamp_descending() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(
        &system,
        Arc::clone(&store),
        vec!["NEP-11".to_string()],
        true,
    );

    let user = UInt160::from_bytes(&[8u8; 20]).expect("user hash");
    let other = UInt160::from_bytes(&[9u8; 20]).expect("other hash");
    let asset = UInt160::from_bytes(&[10u8; 20]).expect("asset hash");
    let tx1 = UInt256::from_bytes(&[11u8; 32]).expect("tx1");
    let tx2 = UInt256::from_bytes(&[12u8; 32]).expect("tx2");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis() as u64;
    let t1 = now_ms - 1_000;
    let t2 = now_ms - 500;

    let (_, sent_prefix, received_prefix) = Nep11Tracker::rpc_prefixes();
    let token_a = vec![0xAA];
    let token_b = vec![0xBB, 0xCC];

    let sent_key_1 = Nep11TransferKey::new(user, t1, asset, token_a.clone(), 0);
    let sent_key_2 = Nep11TransferKey::new(user, t2, asset, token_b.clone(), 1);
    write_tracker_entry(
        &store,
        sent_prefix,
        &sent_key_1,
        &TokenTransfer {
            user_script_hash: other,
            block_index: 1,
            tx_hash: tx1,
            amount: BigInt::from(5),
        },
    );
    write_tracker_entry(
        &store,
        sent_prefix,
        &sent_key_2,
        &TokenTransfer {
            user_script_hash: other,
            block_index: 2,
            tx_hash: tx2,
            amount: BigInt::from(7),
        },
    );

    let received_key = Nep11TransferKey::new(user, t1, asset, token_a.clone(), 0);
    write_tracker_entry(
        &store,
        received_prefix,
        &received_key,
        &TokenTransfer {
            user_script_hash: UInt160::zero(),
            block_index: 3,
            tx_hash: tx1,
            amount: BigInt::from(11),
        },
    );

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep11transfers");

    let address = neo_wallets::wallet_helper::WalletAddress::to_address(
        &user,
        server.system().settings().address_version,
    );
    let params = [
        Value::String(address.clone()),
        json!(t1 - 1),
        json!(now_ms + 1),
    ];
    let result = (handler.callback())(&server, &params).expect("getnep11transfers");
    let obj = result.as_object().expect("result object");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(address.as_str())
    );

    let sent = obj
        .get("sent")
        .and_then(Value::as_array)
        .expect("sent array");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].get("timestamp").and_then(Value::as_u64), Some(t2));
    assert_eq!(sent[1].get("timestamp").and_then(Value::as_u64), Some(t1));
    let token_b_hex = hex::encode(&token_b);
    assert_eq!(
        sent[0].get("tokenid").and_then(Value::as_str),
        Some(token_b_hex.as_str())
    );

    let received = obj
        .get("received")
        .and_then(Value::as_array)
        .expect("received array");
    assert_eq!(received.len(), 1);
    assert!(received[0].get("transferaddress").unwrap().is_null());
    let token_a_hex = hex::encode(&token_a);
    assert_eq!(
        received[0].get("tokenid").and_then(Value::as_str),
        Some(token_a_hex.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep11_properties_returns_expected_fields() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(&system, store, vec!["NEP-11".to_string()], true);

    let contract = build_nep11_properties_contract();
    let contract_hash = contract.hash;
    store_contract_state(&system, &contract);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep11properties");

    let address = neo_wallets::wallet_helper::WalletAddress::to_address(
        &contract_hash,
        server.system().settings().address_version,
    );
    let params = [Value::String(address), Value::String("0102".to_string())];
    let result = (handler.callback())(&server, &params).expect("getnep11properties");
    let obj = result.as_object().expect("properties object");
    assert_eq!(obj.get("name").and_then(Value::as_str), Some("Example NFT"));
    assert_eq!(
        obj.get("image").and_then(Value::as_str),
        Some("ipfs://example")
    );
    assert!(obj.get("tokenURI").unwrap().is_null());
    let extra_encoded = BASE64_STANDARD.encode([1u8, 2, 3]);
    assert_eq!(
        obj.get("extra").and_then(Value::as_str),
        Some(extra_encoded.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep17_transfers_orders_by_timestamp_descending() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(
        &system,
        Arc::clone(&store),
        vec!["NEP-17".to_string()],
        true,
    );

    let gas_token = GasToken::new();
    let asset = gas_token.hash();
    let contract = gas_token
        .contract_state(&system.settings(), 0)
        .expect("gas contract");
    store_contract_state(&system, &contract);
    let user = UInt160::from_bytes(&[2u8; 20]).expect("user hash");
    let other = UInt160::from_bytes(&[3u8; 20]).expect("other hash");
    let tx1 = UInt256::from_bytes(&[4u8; 32]).expect("tx1");
    let tx2 = UInt256::from_bytes(&[5u8; 32]).expect("tx2");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis() as u64;
    let t1 = now_ms - 1_000;
    let t2 = now_ms - 500;

    let (_, sent_prefix, received_prefix) = Nep17Tracker::rpc_prefixes();
    let sent_key_1 = Nep17TransferKey::new(user, t1, asset, 0);
    let sent_key_2 = Nep17TransferKey::new(user, t2, asset, 1);
    write_tracker_entry(
        &store,
        sent_prefix,
        &sent_key_1,
        &TokenTransfer {
            user_script_hash: other,
            block_index: 1,
            tx_hash: tx1,
            amount: BigInt::from(5),
        },
    );
    write_tracker_entry(
        &store,
        sent_prefix,
        &sent_key_2,
        &TokenTransfer {
            user_script_hash: other,
            block_index: 2,
            tx_hash: tx2,
            amount: BigInt::from(7),
        },
    );

    let received_key = Nep17TransferKey::new(user, t1, asset, 0);
    write_tracker_entry(
        &store,
        received_prefix,
        &received_key,
        &TokenTransfer {
            user_script_hash: UInt160::zero(),
            block_index: 3,
            tx_hash: tx1,
            amount: BigInt::from(11),
        },
    );
    let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix.push(sent_prefix);
    prefix.extend_from_slice(&user.to_bytes());
    let start_key = [prefix.as_slice(), &(t1 - 1).to_be_bytes()].concat();
    let end_key = [prefix.as_slice(), &(now_ms + 1).to_be_bytes()].concat();
    let sent_pairs =
        find_range::<Nep17TransferKey, TokenTransfer>(store.as_ref(), &start_key, &end_key)
            .expect("find sent range");
    assert_eq!(sent_pairs.len(), 2);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep17transfers");

    let address = neo_wallets::wallet_helper::WalletAddress::to_address(
        &user,
        server.system().settings().address_version,
    );
    let params = [
        Value::String(address.clone()),
        json!(t1 - 1),
        json!(now_ms + 1),
    ];
    let result = (handler.callback())(&server, &params).expect("getnep17transfers");
    let obj = result.as_object().expect("result object");
    assert_eq!(
        obj.get("address").and_then(Value::as_str),
        Some(address.as_str())
    );

    let sent = obj
        .get("sent")
        .and_then(Value::as_array)
        .expect("sent array");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].get("timestamp").and_then(Value::as_u64), Some(t2));
    assert_eq!(sent[1].get("timestamp").and_then(Value::as_u64), Some(t1));
    assert_eq!(
        sent[0].get("transferaddress").and_then(Value::as_str),
        Some(
            neo_wallets::wallet_helper::WalletAddress::to_address(
                &other,
                server.system().settings().address_version
            )
            .as_str()
        )
    );

    let received = obj
        .get("received")
        .and_then(Value::as_array)
        .expect("received array");
    assert_eq!(received.len(), 1);
    assert!(received[0].get("transferaddress").unwrap().is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_nep17_balances_requires_enabled_tracker() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = create_tracker_store();
    attach_tokens_tracker(&system, store, Vec::new(), true);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep17balances");

    let params = [Value::String(UInt160::zero().to_address())];
    let err = (handler.callback())(&server, &params).expect_err("method not found");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::method_not_found().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn token_tracker_methods_require_registered_service() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerTokensTracker::register_handlers();
    let handler = find_handler(&handlers, "getnep17balances");

    let params = [Value::String(UInt160::zero().to_address())];
    let err = (handler.callback())(&server, &params).expect_err("service should be required");
    let rpc_error: RpcError = err.into();

    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
    assert_eq!(
        rpc_error.data(),
        Some("TokensTracker service not available")
    );
}
