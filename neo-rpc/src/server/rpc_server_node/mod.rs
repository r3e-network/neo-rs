//! Node-related RPC handlers (port of `RpcServer.Node.cs`).

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, internal_error, invalid_params};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
#[cfg(test)]
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_primitives::hardfork::Hardfork;
use neo_blockchain::BlockchainCommand;
use neo_io::{MemoryReader, Serializable};
// LocalNode is in neo-network p2p layer;
use neo_payloads::{block::Block, transaction::Transaction};
use serde_json::{json, Map, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};
use tokio::task::block_in_place;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub struct RpcServerNode;

impl RpcServerNode {
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getconnectioncount" => Self::get_connection_count,
            "getpeers" => Self::get_peers,
            "getversion" => Self::get_version,
            "sendrawtransaction" => Self::send_raw_transaction,
            "submitblock" => Self::submit_block,
        ]
   }

    fn get_connection_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Self::with_local_node(server, |node| json!(node.connected_peers_count()))
   }

    fn get_peers(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let system = server.system();
        let unconnected_endpoints = {
            let fut = system.unconnected_peers();
            if let Ok(handle) = Handle::try_current() {
                block_in_place(|| handle.block_on(fut))
           } else {
                let runtime = Runtime::new().map_err(|err| internal_error(err.to_string()))?;
                runtime.block_on(fut)
           }
       }
        .map_err(|err| internal_error(err.to_string()))?;

        Self::with_local_node(server, |node| {
            let unconnected: Vec<Value> = unconnected_endpoints
                .into_iter()
                .map(|endpoint| {
                    json!({
                        "address": endpoint.ip().to_string(),
                        "port": endpoint.port()})
               })
                .collect();

            let connected: Vec<Value> = node
                .remote_snapshots()
                .into_iter()
                .map(|snapshot| {
                    json!({
                        "address": snapshot.remote_address.ip().to_string(),
                        "port": snapshot.listen_tcp_port})
               })
                .collect();

            json!({
                "unconnected": unconnected,
                "bad": Vec::<Value>::new(),
                "connected": connected})
       })
   }

    fn get_version(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Self::with_local_node(server, |node| {
            let system = server.system();
            let protocol = system.settings();
            let rpc_settings = server.settings();
            let time_per_block_ms = system.time_per_block().as_millis() as u64;
            let max_traceable_blocks = system.max_traceable_blocks();
            let max_valid_until_block_increment = system.max_valid_until_block_increment();

            let mut rpc_info = Map::new();
            rpc_info.insert(
                "maxiteratorresultitems".to_string(),
                json!(rpc_settings.max_iterator_result_items),
            );
            rpc_info.insert(
                "sessionenabled".to_string(),
                Value::Bool(rpc_settings.session_enabled),
            );

            let mut protocol_info = Map::new();
            protocol_info.insert(
                "addressversion".to_string(),
                json!(protocol.address_version),
            );
            protocol_info.insert("network".to_string(), json!(protocol.network));
            protocol_info.insert(
                "validatorscount".to_string(),
                json!(protocol.validators_count),
            );
            protocol_info.insert("msperblock".to_string(), json!(time_per_block_ms));
            protocol_info.insert(
                "maxtraceableblocks".to_string(),
                json!(max_traceable_blocks),
            );
            protocol_info.insert(
                "maxvaliduntilblockincrement".to_string(),
                json!(max_valid_until_block_increment),
            );
            protocol_info.insert(
                "maxtransactionsperblock".to_string(),
                json!(protocol.max_transactions_per_block),
            );
            protocol_info.insert(
                "memorypoolmaxtransactions".to_string(),
                json!(protocol.memory_pool_max_transactions),
            );
            protocol_info.insert(
                "initialgasdistribution".to_string(),
                json!(protocol.initial_gas_distribution),
            );

            let hardforks = Hardfork::all()
                .iter()
                .filter_map(|fork| {
                    protocol.hardforks.get(fork).map(|height| {
                        json!({
                            "name": Self::format_hardfork(*fork),
                            "blockheight": height})
                   })
               })
                .collect();
            protocol_info.insert("hardforks".to_string(), Value::Array(hardforks));

            let committee: Vec<Value> = protocol
                .standby_committee
                .iter()
                .map(|point| Value::String(Self::format_public_key(point.as_bytes())))
                .collect();
            protocol_info.insert("standbycommittee".to_string(), Value::Array(committee));

            let seeds: Vec<Value> = protocol
                .seed_list
                .iter()
                .map(|seed| Value::String(seed.clone()))
                .collect();
            protocol_info.insert("seedlist".to_string(), Value::Array(seeds));

            let mut json = Map::new();
            json.insert("tcpport".to_string(), json!(node.port()));
            json.insert("nonce".to_string(), json!(node.nonce));
            json.insert(
                "useragent".to_string(),
                Value::String(node.user_agent.clone()),
            );
            json.insert("rpc".to_string(), Value::Object(rpc_info));
            json.insert("protocol".to_string(), Value::Object(protocol_info));
            Value::Object(json)
       })
   }

    fn send_raw_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "sendrawtransaction",
            "Invalid transaction payload",
        )?;
        let transaction = Transaction::from_bytes(&raw)
            .map_err(|err| invalid_params(format!("Invalid transaction: {err}")))?;
        let relay_result = rpc_relay::with_relay_responder(server, move |sender| {
            server
                .system()
                .tx_router_actor()
                .try_enqueue_preverify_from(transaction, true, Some(sender))
                .map_err(|err| internal_error(err.to_string()))
       })?;
        rpc_relay::map_relay_result(relay_result)
   }

    fn submit_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "submitblock",
            "Invalid block payload",
        )?;
        let mut reader = MemoryReader::new(&raw);
        let block = <Block as Serializable>::deserialize(&mut reader)
            .map_err(|err| invalid_params(format!("Invalid block: {err}")))?;
        let relay_result = rpc_relay::with_relay_responder(server, |sender| {
            server
                .system()
                .blockchain_actor()
                .tell_from(
                    BlockchainCommand::InventoryBlock {
                        block: Arc::new(block),
                        relay: true,
                        pre_verified: false},
                    Some(sender),
                )
                .map_err(|err| internal_error(err.to_string()))
       })?;
        rpc_relay::map_relay_result(relay_result)
   }

    fn with_local_node<F>(server: &RpcServer, func: F) -> Result<Value, RpcException>
    where
        F: FnOnce(&Arc<LocalNode>) -> Value,
    {
        let local = Self::fetch_local_node(server)?;
        Ok(func(&local))
   }

    fn fetch_local_node(server: &RpcServer) -> Result<Arc<LocalNode>, RpcException> {
        let system = server.system();
        if let Some(local) = system
            .context()
            .local_node_service()
            .map_err(|err| internal_error(err.to_string()))?
        {
            return Ok(local);
       }

        let fut = system.local_node_state();
        let result = if let Ok(handle) = Handle::try_current() {
            block_in_place(|| handle.block_on(fut))
       } else {
            let runtime = Runtime::new().map_err(|err| internal_error(err.to_string()))?;
            runtime.block_on(fut)
       };
        result.map_err(|err| internal_error(err.to_string()))
   }

    fn format_hardfork(fork: Hardfork) -> String {
        format!("{fork:?}").trim_start_matches("Hf").to_string()
   }

    fn format_public_key(bytes: &[u8]) -> String {
        hex::encode(bytes)
   }

    #[allow(dead_code)]
    fn format_endpoint(endpoint: &str) -> Option<Value> {
        if let Ok(addr) = endpoint.parse::<SocketAddr>() {
            return Some(json!({
                "address": addr.ip().to_string(),
                "port": addr.port()}));
       }

        if let Some((host, port)) = endpoint.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                return Some(json!({
                    "address": host.to_string(),
                    "port": port}));
           }
       }

        None
   }
}
