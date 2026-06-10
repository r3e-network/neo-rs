//! Node-related RPC handlers (port of `RpcServer.Node.cs`).

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, invalid_params};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
#[cfg(test)]
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_primitives::hardfork::Hardfork;
use neo_io::{MemoryReader, Serializable};
use neo_network::handle::LocalNodeInfo;
use neo_payloads::{block::Block, transaction::Transaction};
use serde_json::{json, Map, Value};
use std::net::SocketAddr;

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
        // The reth-style network service tracks neither an unconnected
        // address book nor per-peer socket addresses at the handle seam
        // (its lifecycle events carry opaque peer ids only), so the
        // unconnected and connected lists are served empty. The
        // connected-peer COUNT is available via `getconnectioncount`.
        Self::with_local_node(server, |_node| {
            json!({
                "unconnected": Vec::<Value>::new(),
                "bad": Vec::<Value>::new(),
                "connected": Vec::<Value>::new()})
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
        let relay_result = rpc_relay::relay_transaction(server, transaction)?;
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
        let relay_result = rpc_relay::relay_block(server, block)?;
        rpc_relay::map_relay_result(relay_result)
   }

    fn with_local_node<F>(server: &RpcServer, func: F) -> Result<Value, RpcException>
    where
        F: FnOnce(&LocalNodeInfo) -> Value,
    {
        let local = Self::fetch_local_node(server);
        Ok(func(&local))
   }

    fn fetch_local_node(server: &RpcServer) -> LocalNodeInfo {
        server.system().network().local_node_info()
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
