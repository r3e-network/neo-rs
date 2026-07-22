//! Response construction helpers for blockchain RPC methods.
//!
//! Handlers own lookup and request flow; this module owns C#-compatible JSON
//! enrichment for blocks, headers, contracts, governance/native results,
//! mempool entries, storage, and transactions.

use crate::server::ledger_queries;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use crate::types::RpcContractState;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_execution::contract_state::ContractState;
use neo_io::Serializable;
use neo_mempool::PoolItem;
use neo_payloads::{Header, TransactionState, block::Block, transaction::Transaction};
use neo_primitives::{UInt256, hex_util};
use num_bigint::BigInt;
use serde_json::{Map, Value, json};

pub(super) fn block_to_json(
    server: &RpcServer,
    block: &Block,
    current_index: u32,
    next_hash: Option<UInt256>,
) -> Value {
    let mut json = header_fields_to_map(server, &block.header, current_index, next_hash);
    json.insert("size".to_string(), json!(block.size()));
    let system = server.system();
    let settings = system.settings();
    let transactions: Vec<Value> = block
        .transactions
        .iter()
        .map(|tx| tx.to_json(settings.address_version))
        .collect();
    json.insert("tx".to_string(), Value::Array(transactions));
    Value::Object(json)
}

pub(super) fn hash_to_json(hash: &UInt256) -> Value {
    Value::String(hash.to_string())
}

pub(super) fn count_to_json(count: u32) -> Value {
    json!(count)
}

pub(super) fn base64_payload_to_json(payload: String) -> Value {
    Value::String(payload)
}

pub(super) fn system_fee_to_json(system_fee: i64) -> Value {
    Value::String(system_fee.to_string())
}

pub(super) fn transaction_height_to_json(height: u32) -> Value {
    json!(height)
}

pub(super) fn storage_value_to_json(value: &[u8]) -> Value {
    base64_bytes_to_json(value)
}

pub(super) fn find_storage_result_to_json(key_suffix: &[u8], value: &[u8]) -> Value {
    json!({
        "key": BASE64_STANDARD.encode(key_suffix),
        "value": BASE64_STANDARD.encode(value),
    })
}

pub(super) fn find_storage_page_to_json(
    truncated: bool,
    next: usize,
    results: Vec<Value>,
) -> Value {
    json!({
        "truncated": truncated,
        "next": next,
        "results": results,
    })
}

pub(super) fn raw_mempool_hashes_to_json(items: &[PoolItem]) -> Value {
    Value::Array(mempool_hash_values(items))
}

pub(super) fn raw_mempool_with_unverified_to_json(
    height: u32,
    verified: &[PoolItem],
    unverified: &[PoolItem],
) -> Value {
    json!({
        "height": height,
        "verified": mempool_hash_values(verified),
        "unverified": mempool_hash_values(unverified),
    })
}

fn mempool_hash_values(items: &[PoolItem]) -> Vec<Value> {
    items
        .iter()
        .map(|item| Value::String(item.hash().to_string()))
        .collect()
}

fn base64_bytes_to_json(bytes: &[u8]) -> Value {
    Value::String(BASE64_STANDARD.encode(bytes))
}

pub(super) fn header_to_json(
    server: &RpcServer,
    header: &Header,
    current_index: u32,
    next_hash: Option<UInt256>,
) -> Value {
    Value::Object(header_fields_to_map(
        server,
        header,
        current_index,
        next_hash,
    ))
}

fn header_fields_to_map(
    server: &RpcServer,
    header: &Header,
    current_index: u32,
    next_hash: Option<UInt256>,
) -> Map<String, Value> {
    // Canonical header wire-JSON is owned by `neo_payloads::Header::to_json`
    // (single source of truth shared with the RPC client); the server adds
    // only the contextual confirmations / nextblockhash on top.
    let system = server.system();
    let settings = system.settings();
    let mut json = header.to_json(settings.address_version);
    let confirmations = current_index.saturating_sub(header.index()) + 1;
    json.insert("confirmations".to_string(), json!(confirmations));
    if let Some(hash) = next_hash {
        json.insert("nextblockhash".to_string(), Value::String(hash.to_string()));
    }
    json
}

pub(super) fn contract_state_to_json(contract: &ContractState) -> Value {
    let rpc_contract = RpcContractState {
        contract_state: contract.clone(),
    };

    match rpc_contract.to_json() {
        Ok(jobj) => serde_json::from_str(&jobj.to_string())
            .unwrap_or_else(|err| json!({"error": err.to_string()})),
        Err(err) => json!({"error": err.to_string()}),
    }
}

pub(super) fn native_contracts_to_json(contract_states: &[ContractState]) -> Value {
    Value::Array(contract_states.iter().map(contract_state_to_json).collect())
}

pub(super) fn next_block_validator_to_json(public_key: &[u8], votes: i64) -> Value {
    json!({
        "publickey": hex_util::encode_hex(public_key),
        "votes": votes,
    })
}

pub(super) fn next_block_validators_to_json(validators: Vec<Value>) -> Value {
    Value::Array(validators)
}

pub(super) fn candidate_to_json(public_key: &[u8], votes: &BigInt, active: bool) -> Value {
    json!({
        "publickey": hex_util::encode_hex(public_key),
        "votes": votes.to_string(),
        "active": active,
    })
}

pub(super) fn candidates_to_json(candidates: Vec<Value>) -> Value {
    Value::Array(candidates)
}

pub(super) fn committee_to_json(public_keys: &[Vec<u8>]) -> Value {
    Value::Array(
        public_keys
            .iter()
            .map(|point| Value::String(hex_util::encode_hex(point)))
            .collect(),
    )
}

pub(super) fn transaction_to_verbose_json(
    server: &RpcServer,
    tx: &Transaction,
    state: Option<&TransactionState>,
) -> Result<Value, RpcException> {
    let system = server.system();
    let settings = system.settings();
    let mut json = tx.to_json(settings.address_version);

    if let (Value::Object(obj), Some(state)) = (&mut json, state) {
        append_ledger_transaction_context(server, obj, state)?;
    }

    Ok(json)
}

fn append_ledger_transaction_context(
    server: &RpcServer,
    obj: &mut Map<String, Value>,
    state: &TransactionState,
) -> Result<(), RpcException> {
    let system = server.system();
    let store = system.store_cache();
    let block_index = state.block_index();
    let context =
        ledger_queries::transaction_context(system.as_ref(), store.data_cache(), block_index)
            .map_err(internal_error)?;
    obj.insert("confirmations".to_string(), json!(context.confirmations));

    // C# GetRawTransaction verbose adds only blockhash, confirmations and
    // blocktime to Transaction.ToJson (RpcServer.Blockchain.cs:373-381);
    // it does NOT add a vmstate field (that belongs to getapplicationlog).
    // Emitting it here surprises strict clients / response-diff tooling.
    if let Some(block_hash) = context.block_hash {
        obj.insert(
            "blockhash".to_string(),
            Value::String(block_hash.to_string()),
        );

        if let Some(block_time) = context.block_time {
            obj.insert("blocktime".to_string(), json!(block_time));
        }
    }

    Ok(())
}
