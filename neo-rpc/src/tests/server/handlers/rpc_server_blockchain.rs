use super::*;
use crate::client::models::RpcRawMemPool;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_execution::ContractState;
use neo_io::{MemoryReader, Serializable};
use neo_manifest::{ContractManifest, NefFile};
use neo_native_contracts::LedgerContract;
use neo_payloads::Block as LedgerBlock;
use neo_payloads::VerifyResult;
use neo_payloads::block::Block;
use neo_payloads::get_sign_data_vec;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_serialization::json::JToken;
use neo_storage::{StorageItem, StorageKey};
use neo_test_fixtures::TestTransactionBuilder;
use neo_vm_rs::{ExecutionEngineLimits, VmState as VMState};
use neo_vm_rs::{OpCode, StackValue};
use neo_wallets::KeyPair;
use num_bigint::BigInt;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

fn parse_object(value: &Value) -> neo_serialization::json::JObject {
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
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(
        keypair.script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn mint_gas(
    store: &mut neo_storage::persistence::StoreCache,
    _settings: &ProtocolSettings,
    account: UInt160,
    amount: BigInt,
) {
    crate::server::test_support::seed_gas_balance(store, &account, amount);
}

fn make_transaction(nonce: u32) -> Transaction {
    TestTransactionBuilder::new()
        .nonce(nonce)
        .network_fee(1_0000_0000)
        .system_fee(0)
        .valid_until_block(1)
        .signer(
            UInt160::from_bytes(&[7u8; 20]).expect("account"),
            WitnessScope::GLOBAL,
        )
        .build()
}

fn make_ledger_block(
    store: &neo_storage::persistence::StoreCache,
    index: u32,
    transactions: Vec<Transaction>,
) -> LedgerBlock {
    neo_test_fixtures::make_ledger_block(store, index, transactions)
}

fn store_block(store: &mut neo_storage::persistence::StoreCache, block: &LedgerBlock) {
    neo_test_fixtures::store_block_with_vmstate(store, block, VMState::NONE);
}

fn store_contract_state(
    store: &mut neo_storage::persistence::StoreCache,
    contract: &ContractState,
) {
    const PREFIX_CONTRACT: u8 = 0x08;
    const PREFIX_CONTRACT_HASH: u8 = 0x0c;

    let contract_mgmt_id = crate::server::native_queries::NativeQueries::native_registry()
        .get_by_name("ContractManagement")
        .expect("contract management")
        .id();

    let record = contract
        .serialize_contract_record()
        .expect("serialize contract record");

    let mut key_bytes = Vec::with_capacity(1 + 20);
    key_bytes.push(PREFIX_CONTRACT);
    key_bytes.extend_from_slice(&contract.hash.to_bytes());
    let key = StorageKey::new(contract_mgmt_id, key_bytes);
    store.add(key, StorageItem::from_bytes(record));

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
    store: &mut neo_storage::persistence::StoreCache,
    contract_id: i32,
    key: &[u8],
    value: &[u8],
) {
    let storage_key = StorageKey::new(contract_id, key.to_vec());
    store.add(storage_key, StorageItem::from_bytes(value.to_vec()));
    store.commit();
}

fn serialize_test_stack_value(value: &StackValue) -> Vec<u8> {
    BinarySerializer::serialize_stack_value(value, &ExecutionEngineLimits::default())
        .expect("serialize stack value")
}

fn store_committee(
    store: &mut neo_storage::persistence::StoreCache,
    committee: &[neo_crypto::ECPoint],
) {
    const PREFIX_COMMITTEE: u8 = 0x0e;
    let neo_token_id = crate::server::native_queries::NativeQueries::native_registry()
        .get_by_name("NeoToken")
        .expect("neo token")
        .id();

    let items: Vec<StackValue> = committee
        .iter()
        .map(|pk| {
            StackValue::Struct(vec![
                StackValue::ByteString(pk.as_bytes().to_vec()),
                StackValue::Integer(0),
            ])
        })
        .collect();
    let bytes = serialize_test_stack_value(&StackValue::Array(items));
    let key = StorageKey::create(neo_token_id, PREFIX_COMMITTEE);
    store.add(key, StorageItem::from_bytes(bytes));
    store.commit();
}

fn store_candidate_state(
    store: &mut neo_storage::persistence::StoreCache,
    candidate: &neo_crypto::ECPoint,
    registered: bool,
    votes: BigInt,
) {
    let item = StackValue::Struct(vec![
        StackValue::Boolean(registered),
        StackValue::BigInteger(votes.to_signed_bytes_le()),
    ]);
    let bytes = serialize_test_stack_value(&item);
    store_candidate_state_raw(store, candidate, bytes);
}

fn store_candidate_state_raw(
    store: &mut neo_storage::persistence::StoreCache,
    candidate: &neo_crypto::ECPoint,
    bytes: Vec<u8>,
) {
    const PREFIX_CANDIDATE: u8 = 0x21;
    let neo_token_id = crate::server::native_queries::NativeQueries::native_registry()
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

fn store_blocked_account(store: &mut neo_storage::persistence::StoreCache, account: &UInt160) {
    const PREFIX_BLOCKED_ACCOUNT: u8 = 0x0f;
    let policy_id = crate::server::native_queries::NativeQueries::native_registry()
        .get_by_name("PolicyContract")
        .expect("policy")
        .id();
    let key = StorageKey::create_with_uint160(policy_id, PREFIX_BLOCKED_ACCOUNT, account);
    store.add(key, StorageItem::from_bytes(vec![1u8]));
    store.commit();
}

fn make_contract_state(id: i32, hash: UInt160, name: &str) -> ContractState {
    let nef = NefFile::new("test".to_string(), vec![OpCode::PUSH1.byte()]);
    let manifest = ContractManifest::new(name.to_string());
    ContractState::new(id, hash, nef, manifest)
}

#[path = "../rpc_server_blockchain/blocks.rs"]
mod blocks;
#[path = "../rpc_server_blockchain/contracts.rs"]
mod contracts;
#[path = "../rpc_server_blockchain/mempool.rs"]
mod mempool;
#[path = "../rpc_server_blockchain/native_and_errors.rs"]
mod native_and_errors;
#[path = "../rpc_server_blockchain/storage.rs"]
mod storage;
#[path = "../rpc_server_blockchain/transactions.rs"]
mod transactions;
#[path = "../rpc_server_blockchain/validators.rs"]
mod validators;
