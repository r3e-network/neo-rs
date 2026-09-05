use super::*;
use crate::client::models::RpcPeers;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_core::ledger::{TransactionVerificationContext, VerifyResult};
use neo_core::neo_io::BinaryWriter;
use neo_core::neo_io::SerializableExt;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::TransactionAttributeType;
use neo_core::network::p2p::payloads::notary_assisted::NotaryAssisted;
use neo_core::network::p2p::payloads::oracle_response::{MAX_RESULT_SIZE, OracleResponse};
use neo_core::network::p2p::payloads::oracle_response_code::OracleResponseCode;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::network::p2p::payloads::{Block, Header, TransactionAttribute};
use neo_core::persistence::StoreCache;
use neo_core::persistence::apply_tracked_items;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::Contract;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::GasToken;
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::native::NativeContract;
use neo_core::smart_contract::native::Notary;
use neo_core::smart_contract::native::PolicyContract;
use neo_core::smart_contract::native::helpers::NativeHelpers;
use neo_core::smart_contract::native::{Role, RoleManagement};
use neo_core::smart_contract::{CallFlags, StorageItem, StorageKey};
use neo_core::wallets::KeyPair;
use neo_core::{
    NeoSystem, ScriptBuilder, UInt160, UInt256, Verifiable, VerifiableExt, WitnessScope,
};
use neo_json::JToken;
use neo_vm::OpCode;
use neo_vm::VmState as VMState;
use num_bigint::BigInt;
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

fn build_signed_transaction_custom(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    script: Vec<u8>,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(1);
    tx.set_script(script);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn build_signed_transaction(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
    system_fee: i64,
) -> Transaction {
    build_signed_transaction_custom(
        settings,
        keypair,
        nonce,
        system_fee,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    )
}

#[allow(clippy::too_many_arguments)]
fn build_signed_transaction_with(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    valid_until_block: u32,
    script: Vec<u8>,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(valid_until_block);
    tx.set_script(script);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);
    tx.set_attributes(attributes);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn single_validator_settings(keypair: &KeyPair) -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let validator = keypair
        .get_public_key_point()
        .expect("validator public key");
    settings.standby_committee = vec![validator];
    settings.validators_count = 1;
    settings
}

fn build_signed_block(
    settings: &ProtocolSettings,
    store: &StoreCache,
    validator: &KeyPair,
    transactions: Vec<Transaction>,
) -> Block {
    let snapshot = store.data_cache();
    let ledger = LedgerContract::new();
    let prev_hash = ledger.current_hash(snapshot).expect("current hash");
    let prev_trimmed = ledger
        .get_trimmed_block(snapshot, &prev_hash)
        .expect("prev trimmed query")
        .expect("prev trimmed block");
    let prev_index = prev_trimmed.header.index();
    let prev_timestamp = prev_trimmed.header.timestamp;

    let validators = settings.standby_validators();
    let next_consensus = NativeHelpers::get_bft_address(&validators);

    let mut header = Header::new();
    header.set_prev_hash(prev_hash);
    header.set_index(prev_index + 1);
    header.set_timestamp(prev_timestamp + settings.milliseconds_per_block as u64);
    header.set_primary_index(0);
    header.set_next_consensus(next_consensus);
    header.set_nonce(0);

    let mut block = Block::new();
    block.header = header;
    block.transactions = transactions;
    block.rebuild_merkle_root();

    let sign_data = get_sign_data_vec(&block.header, settings.network).expect("sign data");
    let signature = validator.sign(&sign_data).expect("sign header");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = Contract::create_multi_sig_redeem_script(1, &validators);
    block.header.witness = Witness::new_with_scripts(invocation, verification_script);

    block
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
    let script_container: Arc<dyn Verifiable> = Arc::new(container);
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
    let tracked = engine.snapshot_cache().tracked_items();
    apply_tracked_items(store, tracked);
}

fn persist_transaction_record(store: &mut StoreCache, tx: &Transaction, block_index: u32) {
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const RECORD_KIND_TRANSACTION: u8 = 0x01;

    let mut writer = BinaryWriter::new();
    writer
        .write_u8(RECORD_KIND_TRANSACTION)
        .expect("record kind");
    writer.write_u32(block_index).expect("block index");
    writer.write_u8(VMState::NONE.to_byte()).expect("vm state");
    let tx_bytes = tx.to_bytes();
    writer.write_var_bytes(&tx_bytes).expect("tx bytes");

    let mut key_bytes = Vec::with_capacity(1 + 32);
    key_bytes.push(PREFIX_TRANSACTION);
    key_bytes.extend_from_slice(&tx.hash().to_bytes());
    let key = StorageKey::new(LedgerContract::ID, key_bytes);
    store.add(key, StorageItem::from_bytes(writer.to_bytes()));
    store.commit();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_reports_unconnected_queue() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let endpoint: SocketAddr = "127.0.0.1:25000".parse().unwrap();

    system
        .add_unconnected_peers(vec![endpoint])
        .expect("enqueue peers");

    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let unconnected = result
        .get("unconnected")
        .and_then(|v| v.as_array())
        .expect("unconnected array");
    assert_eq!(unconnected.len(), 1);

    let bad = result
        .get("bad")
        .and_then(|v| v.as_array())
        .expect("bad array");
    assert!(bad.is_empty());

    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_empty_when_no_queue() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let unconnected = result
        .get("unconnected")
        .and_then(|v| v.as_array())
        .expect("unconnected array");
    assert!(unconnected.is_empty());

    let bad = result
        .get("bad")
        .and_then(|v| v.as_array())
        .expect("bad array");
    assert!(bad.is_empty());

    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_roundtrips_into_client_model() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let endpoint: SocketAddr = "127.0.0.1:25001".parse().unwrap();
    system
        .add_unconnected_peers(vec![endpoint])
        .expect("enqueue peers");

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let parsed = RpcPeers::from_json(&parse_object(&result)).expect("parse peers");
    assert_eq!(parsed.unconnected.len(), 1);
    assert!(parsed.connected.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_contains_expected_fields() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");

    assert!(json.get("tcpport").is_some());
    assert!(json.get("nonce").is_some());
    assert!(json.get("useragent").is_some());

    let rpc = json
        .get("rpc")
        .and_then(Value::as_object)
        .expect("rpc object");
    assert!(rpc.get("maxiteratorresultitems").is_some());
    assert!(rpc.get("sessionenabled").is_some());

    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    for key in [
        "addressversion",
        "network",
        "validatorscount",
        "msperblock",
        "maxtraceableblocks",
        "maxvaliduntilblockincrement",
        "maxtransactionsperblock",
        "memorypoolmaxtransactions",
        "initialgasdistribution",
        "hardforks",
    ] {
        assert!(
            protocol.get(key).is_some(),
            "missing protocol field {}",
            key
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_hardforks_structure() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");
    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    let hardforks = protocol
        .get("hardforks")
        .and_then(Value::as_array)
        .expect("hardforks array");

    for fork in hardforks {
        let fork_obj = fork.as_object().expect("hardfork object");
        let name = fork_obj
            .get("name")
            .and_then(Value::as_str)
            .expect("hardfork name");
        let blockheight = fork_obj
            .get("blockheight")
            .and_then(Value::as_u64)
            .expect("hardfork blockheight");
        assert!(!name.starts_with("HF_"));
        let _ = blockheight;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_includes_zero_height_hardforks() {
    let mut settings = ProtocolSettings::default();
    for height in settings.hardforks.values_mut() {
        *height = 0;
    }
    let expected = settings.hardforks.len();
    let system = NeoSystem::new(settings, None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");
    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    let hardforks = protocol
        .get("hardforks")
        .and_then(Value::as_array)
        .expect("hardforks array");
    assert_eq!(hardforks.len(), expected);
    assert!(hardforks.iter().all(|fork| {
        fork.as_object()
            .and_then(|obj| obj.get("blockheight"))
            .and_then(Value::as_u64)
            == Some(0)
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_null_input() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_empty_input() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::String(String::new())];
    let err = (handler.callback())(&server, &params).expect_err("empty input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_invalid_base64() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::String("not_base64".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid base64");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_invalid_transaction_bytes() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let payload = BASE64_STANDARD.encode([0u8; 4]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid transaction")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_accepts_valid_transaction() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction(&settings, &keypair, 1, 0);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let result =
        tokio::task::block_in_place(|| (handler.callback())(&server, &params)).expect("send raw");
    let hash = result.get("hash").and_then(Value::as_str).expect("hash");
    assert_eq!(hash, tx.hash().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_notary_sponsored_transaction_without_deposit() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let payer_key = KeyPair::from_private_key(&[0x65u8; 32]).expect("payer keypair");
    let payer = payer_key.get_script_hash();
    let notary = Notary::new().hash();
    let mut tx = Transaction::new();
    tx.set_nonce(2);
    tx.set_network_fee(1_0000_0000);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(1),
    )]);
    tx.set_signers(vec![
        Signer::new(notary, WitnessScope::NONE),
        Signer::new(payer, WitnessScope::GLOBAL),
    ]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = payer_key.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    tx.set_witnesses(vec![
        Witness::empty(),
        Witness::new_with_scripts(invocation, payer_key.get_verification_script()),
    ]);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("missing Notary deposit");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::insufficient_funds().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_insufficient_funds() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x66u8; 32]).expect("keypair");
    let tx = build_signed_transaction(&settings, &keypair, 3, 0);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];

    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("insufficient funds");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::insufficient_funds().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_signature() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x77u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let mut tx = build_signed_transaction(&settings, &keypair, 4, 0);
    if let Some(witness) = tx.witnesses_mut().get_mut(0) {
        if let Some(last) = witness.invocation_script.last_mut() {
            *last ^= 0x01;
        }
    }

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid signature");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_signature().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_size() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x88u8; 32]).expect("keypair");
    let mut tx = Transaction::new();
    tx.set_nonce(13);
    tx.set_network_fee(0);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(1, OracleResponseCode::Success, vec![0u8; MAX_RESULT_SIZE]),
    )]);
    tx.set_script(vec![OpCode::PUSH0.byte(); u16::MAX as usize]);
    tx.set_witnesses(vec![Witness::empty()]);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid size");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_size().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_script() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair");
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        8,
        0,
        1_0000_0000,
        1,
        vec![0xff],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid script");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_script().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_attribute() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let attributes = vec![TransactionAttribute::not_valid_before(5)];
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        9,
        0,
        1_0000_0000,
        1,
        vec![OpCode::PUSH1.byte()],
        attributes,
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid attribute");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_attribute().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_expired_transaction() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair");
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        10,
        0,
        1_0000_0000,
        0,
        vec![OpCode::PUSH1.byte()],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("expired transaction");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::expired_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_policy_failed() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let policy = PolicyContract::new();
    let mut store = system.context().store_snapshot_cache();
    let key = StorageKey::create_with_uint160(policy.id(), 15, &account);
    store.add(key, StorageItem::from_bytes(Vec::new()));
    store.commit();

    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        11,
        0,
        1_0000_0000,
        1,
        vec![OpCode::PUSH1.byte()],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("policy failed");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::policy_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_already_in_pool() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let account = keypair.get_script_hash();
    let mut store = system.context().store_snapshot_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction(&settings, &keypair, 12, 0);
    let mempool = system.mempool();
    let mut pool = mempool.lock();
    let result = pool.try_add(tx.clone(), store.data_cache(), &settings);
    assert_eq!(result, VerifyResult::Succeed);
    drop(pool);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already in pool");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_in_pool().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_already_exists() {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let tx = build_signed_transaction(&settings, &keypair, 2, 0);
    let mut store = system.context().store_snapshot_cache();
    persist_transaction_record(&mut store, &tx, 1);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already exists");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());
    assert_eq!(rpc_error.message(), RpcError::already_exists().message());
    assert!(rpc_error.data().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_invalid_base64() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::String("not_base64".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid base64");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_invalid_block_bytes() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let payload = BASE64_STANDARD.encode([0u8; 4]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid block bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid block")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_accepts_valid_block() {
    let validator = KeyPair::from_private_key(&[0x10u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.context().store_snapshot_cache();
    let account = validator.get_script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        1,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let snapshot = store.data_cache();
    let verification = tx.verify(
        &settings,
        snapshot,
        Some(&TransactionVerificationContext::new()),
        &[],
    );
    assert_eq!(verification, VerifyResult::Succeed);
    let store = system.context().store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    let expected_hash = Block::hash(&mut block);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("submit block");
    let hash = result.get("hash").and_then(Value::as_str).expect("hash");
    assert_eq!(hash, expected_hash.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_already_exists() {
    let validator = KeyPair::from_private_key(&[0x11u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.context().store_snapshot_cache();
    let account = validator.get_script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        2,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.context().store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_index(0);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already exists");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_block() {
    let validator = KeyPair::from_private_key(&[0x12u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.context().store_snapshot_cache();
    let account = validator.get_script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        3,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.context().store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.witness = Witness::new();

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid block");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_prev_hash() {
    let validator = KeyPair::from_private_key(&[0x13u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.context().store_snapshot_cache();
    let account = validator.get_script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        4,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.context().store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_prev_hash(UInt256::from([0xABu8; 32]));

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid prev hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_index() {
    let validator = KeyPair::from_private_key(&[0x14u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.context().store_snapshot_cache();
    let account = validator.get_script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        5,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.context().store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_index(block.header.index() + 10);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid index");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_null_input() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_empty_input() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::String(String::new())];
    let err = (handler.callback())(&server, &params).expect_err("empty input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_connection_count_defaults_to_zero() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getconnectioncount");

    let result = (handler.callback())(&server, &[]).expect("get connection count");
    assert_eq!(result.as_u64().unwrap_or_default(), 0);
}

/// Node-level Notary roundtrip driven purely through the RPC handlers:
///
/// 1. block 1: committee designates a P2PNotary node and the payer deposits
///    10 GAS into the Notary contract via a real `GAS.transfer` invocation;
/// 2. `sendrawtransaction` admits a `NotaryAssisted` transaction whose first
///    signer is the Notary contract with an empty witness — the witness is
///    verified by invoking the native `Notary.verify` through the deployed
///    contract state (deposit sufficiency + designated notary signature);
/// 3. block 2: `submitblock` persists the transaction, so `Notary::on_persist`
///    deducts `system_fee + network_fee` from the payer deposit;
/// 4. the deposit balance/expiration and the notary reward are asserted
///    against the persisted store.
#[tokio::test(flavor = "multi_thread")]
async fn notary_node_level_deposit_deduction_roundtrip() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
        .try_init();
    let validator = KeyPair::from_private_key(&[0x10u8; 32]).expect("validator key");
    let payer_key = KeyPair::from_private_key(&[0x65u8; 32]).expect("payer key");
    let notary_node_key = KeyPair::from_private_key(&[0x66u8; 32]).expect("notary node key");
    let payer = payer_key.get_script_hash();
    let notary = Notary::new().hash();
    let role_management_hash = RoleManagement::new().hash();
    let gas = GasToken::new();
    let notary_pubkey = notary_node_key
        .get_public_key_point()
        .expect("notary pubkey");

    let settings = single_validator_settings(&validator);
    let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let send_handler = find_handler(&handlers, "sendrawtransaction");
    let submit_handler = find_handler(&handlers, "submitblock");

    // Fund the payer and the committee account directly in the store (same
    // fixture approach as the other tests). The Notary deposit itself is
    // created on-chain by the GAS transfer transaction inside block 1.
    let mut store = system.context().store_snapshot_cache();
    let committee_address = {
        let snapshot = store.data_cache();
        NativeHelpers::committee_address(&settings, Some(snapshot))
    };
    mint_gas(&mut store, &settings, payer, BigInt::from(100_0000_0000i64));
    mint_gas(
        &mut store,
        &settings,
        committee_address,
        BigInt::from(100_0000_0000i64),
    );
    store.commit();

    // ---- block 1 tx A: committee designates the notary node as P2PNotary ----
    let designate_script = {
        let mut sb = ScriptBuilder::new();
        // NeoVM PACK pops the stack top into items[0], so arguments are pushed
        // in reverse logical order. The syscall pops hash -> method -> flags ->
        // args, and `call_contract_internal` re-pushes the array items so the
        // native handler observes args[i] == args_array.items[i].
        sb.emit_push(&notary_pubkey.to_bytes()); // keys[0]: public key
        sb.emit_push_int(1);
        sb.emit_opcode(OpCode::PACK); // keys = [public key]
        sb.emit_push_int(Role::P2PNotary as i64); // pushed last -> items[0] = role
        sb.emit_push_int(2);
        sb.emit_opcode(OpCode::PACK); // args = [role, keys]
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push("designateAsRole".as_bytes());
        sb.emit_push(&role_management_hash.to_bytes());
        sb.emit_syscall("System.Contract.Call")
            .expect("emit designate syscall");
        let script = sb.to_array();
        println!(
            "DESIGNATE_SCRIPT_HEX: {}",
            script
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        script
    };

    let mut designate_tx = Transaction::new();
    designate_tx.set_nonce(1);
    designate_tx.set_system_fee(1_0000_0000);
    designate_tx.set_network_fee(0_1000_0000);
    designate_tx.set_valid_until_block(1000);
    designate_tx.set_script(designate_script);
    designate_tx.set_signers(vec![Signer::new(committee_address, WitnessScope::GLOBAL)]);

    let validators = settings.standby_validators();
    let redeem_script = Contract::create_multi_sig_redeem_script(1, &validators);
    let sign_data =
        get_sign_data_vec(&designate_tx, settings.network).expect("designate sign data");
    let validator_signature = validator.sign(&sign_data).expect("validator sign");
    let mut validator_invocation = Vec::with_capacity(validator_signature.len() + 2);
    validator_invocation.push(OpCode::PUSHDATA1.byte());
    validator_invocation.push(validator_signature.len() as u8);
    validator_invocation.extend_from_slice(&validator_signature);
    designate_tx.set_witnesses(vec![Witness::new_with_scripts(
        validator_invocation,
        redeem_script,
    )]);

    // ---- block 1 tx B: payer deposits 10 GAS into the Notary contract ------
    let deposit_script = {
        let mut sb = ScriptBuilder::new();
        // PACK places the stack top at items[0]: push in reverse logical order.
        // transfer signature: transfer(from, to, amount, data) where the Notary
        // contract expects data = [owner, till].
        sb.emit_push_int(2000); // data.till (pushed first -> items[1])
        sb.emit_push(&payer.to_bytes()); // data.owner (pushed last -> items[0])
        sb.emit_push_int(2);
        sb.emit_opcode(OpCode::PACK); // data = [owner, till]; the array stays on
        // the stack and is consumed as args[3] by
        // the outer PACK below — nothing else may
        // be pushed between this PACK and it.
        sb.emit_push_int(10_0000_0000); // args[2]: amount
        sb.emit_push(&notary.to_bytes()); // args[1]: to
        sb.emit_push(&payer.to_bytes()); // args[0]: from (stack top)
        sb.emit_push_int(4);
        sb.emit_opcode(OpCode::PACK); // args = [from, to, amount, data]
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push("transfer".as_bytes());
        sb.emit_push(&gas.hash().to_bytes());
        sb.emit_syscall("System.Contract.Call")
            .expect("emit transfer syscall");
        sb.to_array()
    };

    let mut deposit_tx = Transaction::new();
    deposit_tx.set_nonce(2);
    deposit_tx.set_system_fee(1_0000_0000);
    deposit_tx.set_network_fee(0_1000_0000);
    deposit_tx.set_valid_until_block(1000);
    deposit_tx.set_script(deposit_script);
    deposit_tx.set_signers(vec![Signer::new(payer, WitnessScope::GLOBAL)]);
    let sign_data = get_sign_data_vec(&deposit_tx, settings.network).expect("deposit sign data");
    let payer_signature = payer_key.sign(&sign_data).expect("payer sign");
    let mut payer_invocation = Vec::with_capacity(payer_signature.len() + 2);
    payer_invocation.push(OpCode::PUSHDATA1.byte());
    payer_invocation.push(payer_signature.len() as u8);
    payer_invocation.extend_from_slice(&payer_signature);
    deposit_tx.set_witnesses(vec![Witness::new_with_scripts(
        payer_invocation,
        payer_key.get_verification_script(),
    )]);

    // Admit both transactions through the real sendrawtransaction handler.
    for (tx, label) in [(&designate_tx, "designate"), (&deposit_tx, "deposit")] {
        let payload = BASE64_STANDARD.encode(tx.to_bytes());
        let params = [Value::String(payload)];
        let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
            .unwrap_or_else(|error| panic!("{label} transaction rejected: {error:?}"));
        let hash = result.get("hash").and_then(Value::as_str).expect("hash");
        assert_eq!(hash, tx.hash().to_string(), "{label} hash mismatch");
    }

    // ---- block 1: submitblock persists designation + deposit ---------------
    let store = system.context().store_cache();
    let block = build_signed_block(
        &settings,
        &store,
        &validator,
        vec![designate_tx, deposit_tx],
    );
    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block 1"));
    let params = [Value::String(payload)];
    let result = tokio::task::block_in_place(|| (submit_handler.callback())(&server, &params))
        .expect("submit block 1");
    assert!(result.get("hash").is_some());

    let store = system.context().store_snapshot_cache();
    assert_eq!(
        LedgerContract::new()
            .current_index(&store)
            .expect("current index after block 1"),
        1,
        "block 1 should be persisted (genesis + 1)"
    );

    let snapshot = store.data_cache();
    // DEBUG: inspect role designation storage after block 1
    {
        use neo_core::persistence::SeekDirection;
        let prefix = StorageKey::create(RoleManagement::new().id(), Role::P2PNotary as u8);
        let found: Vec<_> = snapshot
            .find(Some(&prefix), SeekDirection::Backward)
            .collect();
        println!("ROLE_DESIGNATION_ENTRIES: {}", found.len());
        for (k, v) in &found {
            println!(
                "  id={} suffix={:02x?} value_len={}",
                k.id(),
                k.suffix(),
                v.to_value().len()
            );
        }
        // Dump every store key to find where the designation landed.
        let all: Vec<_> = snapshot.find(None, SeekDirection::Forward).collect();
        println!("TOTAL_STORE_KEYS: {}", all.len());
        for (k, v) in &all {
            let sfx = k.suffix();
            println!(
                "  KEY id={} suffix={:02x?} value_len={}",
                k.id(),
                &sfx[..sfx.len().min(9)],
                v.to_value().len()
            );
        }
    }
    let notary_contract = Notary::new();
    let deposit_before = notary_contract.balance_of(snapshot, &payer);
    assert_eq!(
        deposit_before,
        BigInt::from(10_0000_0000i64),
        "deposit must exist after the on-chain GAS transfer"
    );

    // ---- sendrawtransaction: NotaryAssisted tx with a notary-node witness --
    let mut notary_tx = Transaction::new();
    notary_tx.set_nonce(3);
    notary_tx.set_system_fee(0);
    notary_tx.set_network_fee(2_0000_0000);
    notary_tx.set_valid_until_block(1000);
    notary_tx.set_script(vec![OpCode::PUSH1.byte()]);
    notary_tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(1),
    )]);
    notary_tx.set_signers(vec![
        Signer::new(notary, WitnessScope::NONE),
        Signer::new(payer, WitnessScope::GLOBAL),
    ]);

    let sign_data = get_sign_data_vec(&notary_tx, settings.network).expect("notary tx sign data");
    let notary_signature = notary_node_key.sign(&sign_data).expect("notary node sign");
    let mut notary_invocation = Vec::with_capacity(notary_signature.len() + 2);
    notary_invocation.push(OpCode::PUSHDATA1.byte());
    notary_invocation.push(notary_signature.len() as u8);
    notary_invocation.extend_from_slice(&notary_signature);
    let payer_signature = payer_key.sign(&sign_data).expect("payer sign");
    let mut payer_invocation = Vec::with_capacity(payer_signature.len() + 2);
    payer_invocation.push(OpCode::PUSHDATA1.byte());
    payer_invocation.push(payer_signature.len() as u8);
    payer_invocation.extend_from_slice(&payer_signature);
    notary_tx.set_witnesses(vec![
        // The Notary signer has an empty verification script; verification
        // invokes the native `verify` method from the deployed contract state.
        Witness::new_with_scripts(notary_invocation, Vec::new()),
        Witness::new_with_scripts(payer_invocation, payer_key.get_verification_script()),
    ]);

    let notary_tx_hash = notary_tx.hash().to_string();
    let payload = BASE64_STANDARD.encode(notary_tx.to_bytes());
    let params = [Value::String(payload)];
    let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
        .expect("notary-assisted transaction should be accepted into the pool");
    assert_eq!(
        result.get("hash").and_then(Value::as_str),
        Some(notary_tx_hash.as_str())
    );

    // ---- block 2: submitblock persists the NotaryAssisted transaction ------
    let store = system.context().store_cache();
    let block = build_signed_block(&settings, &store, &validator, vec![notary_tx]);
    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block 2"));
    let params = [Value::String(payload)];
    let result = tokio::task::block_in_place(|| (submit_handler.callback())(&server, &params))
        .expect("submit block 2");
    assert!(result.get("hash").is_some());

    let store = system.context().store_snapshot_cache();
    assert_eq!(
        LedgerContract::new()
            .current_index(&store)
            .expect("current index after block 2"),
        2,
        "block 2 should be persisted (genesis + 2)"
    );

    let snapshot = store.data_cache();
    let notary_contract = Notary::new();
    let deposit_after = notary_contract.balance_of(snapshot, &payer);
    assert_eq!(
        deposit_after,
        deposit_before - BigInt::from(2_0000_0000i64),
        "Notary::on_persist must deduct system_fee + network_fee (0 + 2 GAS) from the payer deposit"
    );
    let expiration = notary_contract.expiration_of(snapshot, &payer);
    assert_eq!(expiration, 2000, "deposit expiration must stay unchanged");

    // Notary node reward: (nkeys + 1) * NotaryServiceFeePerKey / #notaries.
    // The fee is policy-configurable; with default policy it is 0, so assert
    // against the policy value instead of a hardcoded constant.
    let notary_account = Contract::create_signature_contract(notary_pubkey).script_hash();
    let reward = gas.balance_of_snapshot(snapshot, &notary_account);
    let policy = PolicyContract::new();
    let fee_per_key = policy
        .get_attribute_fee_for_type(snapshot, TransactionAttributeType::NotaryAssisted as u8)
        .expect("notary attribute fee");
    let expected_reward = BigInt::from((1i64 + 1) * fee_per_key);
    assert_eq!(reward, expected_reward, "notary node reward");
}
