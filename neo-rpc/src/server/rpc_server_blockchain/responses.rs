//! Response construction helpers for blockchain RPC methods.
//!
//! Handlers own lookup and request flow; this module owns C#-compatible JSON
//! enrichment for blocks, headers, contracts, and transactions.

use crate::client::models::RpcContractState;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_execution::contract_state::ContractState;
use neo_io::Serializable;
use neo_native_contracts::LedgerContract;
use neo_payloads::{Header, TransactionState, block::Block, transaction::Transaction};
use neo_primitives::UInt256;
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
        .map(|tx| tx.to_json(&settings))
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
    let mut json = header.to_json(&settings);
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

pub(super) fn transaction_to_verbose_json(
    server: &RpcServer,
    tx: &Transaction,
    state: Option<&TransactionState>,
) -> Result<Value, RpcException> {
    let system = server.system();
    let settings = system.settings();
    let mut json = tx.to_json(&settings);

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
    let ledger = LedgerContract::new();
    let block_index = state.block_index();
    let current_index = ledger
        .current_index(store.data_cache())
        .map_err(internal_error)?;
    let confirmations = current_index.saturating_sub(block_index).saturating_add(1);
    obj.insert("confirmations".to_string(), json!(confirmations));

    // C# GetRawTransaction verbose adds only blockhash, confirmations and
    // blocktime to Transaction.ToJson (RpcServer.Blockchain.cs:373-381);
    // it does NOT add a vmstate field (that belongs to getapplicationlog).
    // Emitting it here surprises strict clients / response-diff tooling.
    if let Some(block_hash) = ledger
        .get_block_hash(store.data_cache(), block_index)
        .map_err(internal_error)?
    {
        obj.insert(
            "blockhash".to_string(),
            Value::String(block_hash.to_string()),
        );

        if let Some(block) = ledger
            .get_trimmed_block(store.data_cache(), &block_hash)
            .map_err(internal_error)?
        {
            obj.insert("blocktime".to_string(), json!(block.header.timestamp()));
        }
    }

    Ok(())
}
