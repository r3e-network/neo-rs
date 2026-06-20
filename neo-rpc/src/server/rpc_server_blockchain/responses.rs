use crate::client::models::RpcContractState;
use crate::server::rpc_server::RpcServer;
use neo_execution::contract_state::ContractState;
use neo_io::Serializable;
use neo_payloads::{Header, block::Block};
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
