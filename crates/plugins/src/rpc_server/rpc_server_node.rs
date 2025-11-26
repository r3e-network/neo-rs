//! Node-related RPC handlers (port of `RpcServer.Node.cs`).

use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};
use akka::{Actor, ActorContext, ActorRef, ActorResult, ActorSystem, Props};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_core::hardfork::Hardfork;
use neo_core::ledger::{BlockchainCommand, RelayResult, VerifyResult};
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::neo_system::TransactionRouterMessage;
use neo_core::network::p2p::local_node::LocalNode;
use neo_core::network::p2p::payloads::{block::Block, transaction::Transaction};
use serde_json::{json, Map, Value};
use std::any::Any;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::oneshot;
use tokio::task::block_in_place;

pub struct RpcServerNode;

impl RpcServerNode {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("getconnectioncount", Self::get_connection_count),
            Self::handler("getpeers", Self::get_peers),
            Self::handler("getplugins", Self::get_plugins),
            Self::handler("getversion", Self::get_version),
            Self::handler("sendrawtransaction", Self::send_raw_transaction),
            Self::handler("submitblock", Self::submit_block),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    fn get_connection_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Self::with_local_node(server, |node| json!(node.connected_peers_count()))
    }

    fn get_peers(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let system = server.system();
        // Populate the unconnected list from the underlying actor to match C# `GetPeers`.
        // If a consumer added peers via `add_unconnected_peers`, they will appear here.
        let unconnected_endpoints = {
            let fut = system.unconnected_peers();
            match Handle::try_current() {
                Ok(handle) => block_in_place(|| handle.block_on(fut)),
                Err(_) => {
                    let runtime =
                        Runtime::new().map_err(|err| Self::internal_error(err.to_string()))?;
                    runtime.block_on(fut)
                }
            }
        }
        .map_err(|err| Self::internal_error(err.to_string()))?;

        Self::with_local_node(server, |node| {
            let unconnected: Vec<Value> = unconnected_endpoints
                .into_iter()
                .map(|endpoint| {
                    json!({
                        "address": endpoint.ip().to_string(),
                        "port": endpoint.port(),
                    })
                })
                .collect();

            let connected: Vec<Value> = node
                .remote_snapshots()
                .into_iter()
                .map(|snapshot| {
                    json!({
                        "address": snapshot.remote_address.ip().to_string(),
                        "port": snapshot.listen_tcp_port,
                    })
                })
                .collect();

            json!({
                "unconnected": unconnected,
                "bad": Vec::<Value>::new(),
                "connected": connected,
            })
        })
    }

    fn get_plugins(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Ok(server.list_plugins())
    }

    fn get_version(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Self::with_local_node(server, |node| {
            let system = server.system();
            let protocol = system.settings();
            let rpc_settings = server.settings();

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
            protocol_info.insert(
                "msperblock".to_string(),
                json!(protocol.milliseconds_per_block),
            );
            protocol_info.insert(
                "maxtraceableblocks".to_string(),
                json!(protocol.max_traceable_blocks),
            );
            protocol_info.insert(
                "maxvaliduntilblockincrement".to_string(),
                json!(protocol.max_valid_until_block_increment),
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

            let hardforks = protocol
                .hardforks
                .iter()
                .map(|(fork, height)| {
                    json!({
                        "name": Self::format_hardfork(*fork),
                        "blockheight": height,
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
        let payload = Self::expect_string_param(params, 0, "sendrawtransaction")?;
        let raw = BASE64_STANDARD
            .decode(payload.trim())
            .map_err(|_| Self::invalid_params("Invalid transaction payload"))?;
        let transaction = Transaction::from_bytes(&raw)
            .map_err(|err| Self::invalid_params(format!("Invalid transaction: {}", err)))?;
        let relay_result = Self::with_relay_responder(server, |sender| {
            server
                .system()
                .tx_router_actor()
                .tell_from(
                    TransactionRouterMessage::Preverify {
                        transaction: transaction.clone(),
                        relay: true,
                    },
                    Some(sender),
                )
                .map_err(|err| Self::internal_error(err.to_string()))
        })?;
        Self::map_relay_result(relay_result)
    }

    fn submit_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let payload = Self::expect_string_param(params, 0, "submitblock")?;
        let raw = BASE64_STANDARD
            .decode(payload.trim())
            .map_err(|_| Self::invalid_params("Invalid block payload"))?;
        let mut reader = MemoryReader::new(&raw);
        let block = <Block as Serializable>::deserialize(&mut reader)
            .map_err(|err| Self::invalid_params(format!("Invalid block: {}", err)))?;
        let relay_result = Self::with_relay_responder(server, |sender| {
            server
                .system()
                .blockchain_actor()
                .tell_from(
                    BlockchainCommand::InventoryBlock { block, relay: true },
                    Some(sender),
                )
                .map_err(|err| Self::internal_error(err.to_string()))
        })?;
        Self::map_relay_result(relay_result)
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
        let fut = system.local_node_state();
        let result = match Handle::try_current() {
            Ok(handle) => block_in_place(|| handle.block_on(fut)),
            Err(_) => {
                let runtime =
                    Runtime::new().map_err(|err| Self::internal_error(err.to_string()))?;
                runtime.block_on(fut)
            }
        };
        result.map_err(|err| Self::internal_error(err.to_string()))
    }

    fn format_hardfork(fork: Hardfork) -> String {
        format!("{:?}", fork).trim_start_matches("Hf").to_string()
    }

    fn format_public_key(bytes: &[u8]) -> String {
        format!("0x{}", hex::encode(bytes))
    }

    fn internal_error(message: impl Into<String>) -> RpcException {
        RpcException::new(RpcError::internal_server_error().with_data(message.into()))
    }

    #[allow(dead_code)]
    fn format_endpoint(endpoint: &str) -> Option<Value> {
        if let Ok(addr) = endpoint.parse::<SocketAddr>() {
            return Some(json!({
                "address": addr.ip().to_string(),
                "port": addr.port(),
            }));
        }

        if let Some((host, port)) = endpoint.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                return Some(json!({
                    "address": host.to_string(),
                    "port": port,
                }));
            }
        }

        None
    }

    fn expect_string_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<String, RpcException> {
        params
            .get(index)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .ok_or_else(|| {
                RpcException::new(RpcError::invalid_params().with_data(format!(
                    "{} expects string parameter {}",
                    method,
                    index + 1
                )))
            })
    }

    fn invalid_params(message: impl Into<String>) -> RpcException {
        RpcException::new(RpcError::invalid_params().with_data(message.into()))
    }

    fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
        match result.result {
            VerifyResult::Succeed => Ok(json!({ "hash": result.hash.to_string() })),
            VerifyResult::AlreadyExists => Err(RpcException::new(RpcError::already_exists())),
            VerifyResult::AlreadyInPool => Err(RpcException::new(RpcError::already_in_pool())),
            VerifyResult::OutOfMemory => Err(RpcException::new(RpcError::mempool_cap_reached())),
            VerifyResult::InvalidScript => Err(RpcException::new(RpcError::invalid_script())),
            VerifyResult::InvalidAttribute => Err(RpcException::new(RpcError::invalid_attribute())),
            VerifyResult::InvalidSignature => Err(RpcException::new(RpcError::invalid_signature())),
            VerifyResult::OverSize => Err(RpcException::new(RpcError::invalid_size())),
            VerifyResult::Expired => Err(RpcException::new(RpcError::expired_transaction())),
            VerifyResult::InsufficientFunds => {
                Err(RpcException::new(RpcError::insufficient_funds()))
            }
            VerifyResult::PolicyFail => Err(RpcException::new(RpcError::policy_failed())),
            VerifyResult::UnableToVerify => Err(RpcException::new(
                RpcError::verification_failed().with_data("UnableToVerify"),
            )),
            VerifyResult::Invalid => Err(RpcException::new(
                RpcError::verification_failed().with_data("Invalid"),
            )),
            VerifyResult::HasConflicts => Err(RpcException::new(
                RpcError::verification_failed().with_data("HasConflicts"),
            )),
            VerifyResult::Unknown => Err(RpcException::new(
                RpcError::verification_failed().with_data("Unknown"),
            )),
        }
    }

    fn with_relay_responder<F>(server: &RpcServer, send: F) -> Result<RelayResult, RpcException>
    where
        F: FnOnce(ActorRef) -> Result<(), RpcException>,
    {
        let system = server.system();
        let actor_system = system.actor_system();
        let (responder, rx) = Self::spawn_relay_responder(actor_system)?;
        if let Err(err) = send(responder.clone()) {
            let _ = actor_system.stop(&responder);
            return Err(err);
        }
        let result = rx
            .blocking_recv()
            .map_err(|_| Self::internal_error("relay result channel closed"))?;
        let _ = actor_system.stop(&responder);
        Ok(result)
    }

    fn spawn_relay_responder(
        actor_system: &ActorSystem,
    ) -> Result<(ActorRef, oneshot::Receiver<RelayResult>), RpcException> {
        let (tx, rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(tx)));
        let props = {
            let completion = completion.clone();
            Props::new(move || RelayResultResponder {
                completion: completion.clone(),
            })
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let actor = actor_system
            .actor_of(props, format!("rpc-relay-{unique}"))
            .map_err(|err| Self::internal_error(err.to_string()))?;
        Ok((actor, rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc_server::rcp_server_settings::RpcServerConfig;
    use neo_core::{protocol_settings::ProtocolSettings, NeoSystem};

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .expect("handler present")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_peers_reports_unconnected_queue() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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

        let connected = result
            .get("connected")
            .and_then(|v| v.as_array())
            .expect("connected array");
        assert!(connected.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_peers_empty_when_no_queue() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system.clone(), RpcServerConfig::default());
        let handlers = RpcServerNode::register_handlers();
        let peers_handler = find_handler(&handlers, "getpeers");

        let result = (peers_handler.callback())(&server, &[]).expect("get peers");
        let unconnected = result
            .get("unconnected")
            .and_then(|v| v.as_array())
            .expect("unconnected array");
        assert!(unconnected.is_empty());

        let connected = result
            .get("connected")
            .and_then(|v| v.as_array())
            .expect("connected array");
        assert!(connected.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_connection_count_defaults_to_zero() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system.clone(), RpcServerConfig::default());
        let handlers = RpcServerNode::register_handlers();
        let handler = find_handler(&handlers, "getconnectioncount");

        let result = (handler.callback())(&server, &[]).expect("get connection count");
        assert_eq!(result.as_u64().unwrap_or_default(), 0);
    }
}

struct RelayResultResponder {
    completion: Arc<Mutex<Option<oneshot::Sender<RelayResult>>>>,
}

#[async_trait]
impl Actor for RelayResultResponder {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(result) = message.downcast::<RelayResult>() {
            if let Ok(mut guard) = self.completion.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(*result);
                }
            }
            let _ = ctx.stop_self();
        }
        Ok(())
    }
}
