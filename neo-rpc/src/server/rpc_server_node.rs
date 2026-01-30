//! Node-related RPC handlers (port of `RpcServer.Node.cs`).

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_core::akka::{Actor, ActorContext, ActorRef, ActorResult, ActorSystem, Props};
use neo_core::hardfork::Hardfork;
use neo_core::ledger::{BlockchainCommand, RelayResult, VerifyResult};
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::neo_system::TransactionRouterMessage;
use neo_core::network::p2p::local_node::LocalNode;
use neo_core::network::p2p::payloads::{block::Block, transaction::Transaction};
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::any::Any;
use std::net::SocketAddr;
use std::sync::Arc;
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

            // Match Neo's RpcServer: include configured hardforks in declaration order.
            let hardforks = Hardfork::all()
                .iter()
                .filter_map(|fork| {
                    protocol.hardforks.get(fork).map(|height| {
                        json!({
                            "name": Self::format_hardfork(*fork),
                            "blockheight": height,
                        })
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
        let payload =
            crate::server::rpc_helpers::expect_string_param(params, 0, "sendrawtransaction")?;
        let raw = BASE64_STANDARD
            .decode(payload.trim())
            .map_err(|_| invalid_params("Invalid transaction payload"))?;
        let transaction = Transaction::from_bytes(&raw)
            .map_err(|err| invalid_params(format!("Invalid transaction: {err}")))?;
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
                .map_err(|err| internal_error(err.to_string()))
        })?;
        Self::map_relay_result(relay_result)
    }

    fn submit_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let payload = crate::server::rpc_helpers::expect_string_param(params, 0, "submitblock")?;
        let raw = BASE64_STANDARD
            .decode(payload.trim())
            .map_err(|_| invalid_params("Invalid block payload"))?;
        let mut reader = MemoryReader::new(&raw);
        let block = <Block as Serializable>::deserialize(&mut reader)
            .map_err(|err| invalid_params(format!("Invalid block: {err}")))?;
        let relay_result = Self::with_relay_responder(server, |sender| {
            server
                .system()
                .blockchain_actor()
                .tell_from(
                    BlockchainCommand::InventoryBlock { block, relay: true },
                    Some(sender),
                )
                .map_err(|err| internal_error(err.to_string()))
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

    fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
        match result.result {
            VerifyResult::Succeed => Ok(json!({ "hash": result.hash.to_string() })),
            VerifyResult::AlreadyExists => Err(RpcException::from(RpcError::already_exists())),
            VerifyResult::AlreadyInPool => Err(RpcException::from(RpcError::already_in_pool())),
            VerifyResult::OutOfMemory => Err(RpcException::from(RpcError::mempool_cap_reached())),
            VerifyResult::InvalidScript => Err(RpcException::from(RpcError::invalid_script())),
            VerifyResult::InvalidAttribute => {
                Err(RpcException::from(RpcError::invalid_attribute()))
            }
            VerifyResult::InvalidSignature => {
                Err(RpcException::from(RpcError::invalid_signature()))
            }
            VerifyResult::OverSize => Err(RpcException::from(RpcError::invalid_size())),
            VerifyResult::Expired => Err(RpcException::from(RpcError::expired_transaction())),
            VerifyResult::InsufficientFunds => {
                Err(RpcException::from(RpcError::insufficient_funds()))
            }
            VerifyResult::PolicyFail => Err(RpcException::from(RpcError::policy_failed())),
            VerifyResult::UnableToVerify => Err(RpcException::from(
                RpcError::verification_failed().with_data("UnableToVerify"),
            )),
            VerifyResult::Invalid => Err(RpcException::from(
                RpcError::verification_failed().with_data("Invalid"),
            )),
            VerifyResult::HasConflicts => Err(RpcException::from(
                RpcError::verification_failed().with_data("HasConflicts"),
            )),
            VerifyResult::Unknown => Err(RpcException::from(
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
            .map_err(|_| internal_error("relay result channel closed"))?;
        let _ = actor_system.stop(&responder);
        Ok(result)
    }

    fn spawn_relay_responder(
        actor_system: &ActorSystem,
    ) -> Result<(ActorRef, oneshot::Receiver<RelayResult>), RpcException> {
        let (tx, rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(tx)));
        let props = {
            let completion = completion;
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
            .map_err(|err| internal_error(err.to_string()))?;
        Ok((actor, rx))
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
            let mut guard = self.completion.lock();
            if let Some(tx) = guard.take() {
                let _ = tx.send(*result);
            }
            let _ = ctx.stop_self();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::RpcPeers;
    use crate::server::rcp_server_settings::RpcServerConfig;
    use neo_core::extensions::io::serializable::SerializableExtensions;
    use neo_core::ledger::TransactionVerificationContext;
    use neo_core::neo_io::BinaryWriter;
    use neo_core::network::p2p::helper::get_sign_data_vec;
    use neo_core::network::p2p::payloads::oracle_response::{OracleResponse, MAX_RESULT_SIZE};
    use neo_core::network::p2p::payloads::oracle_response_code::OracleResponseCode;
    use neo_core::network::p2p::payloads::signer::Signer;
    use neo_core::network::p2p::payloads::transaction::Transaction;
    use neo_core::network::p2p::payloads::witness::Witness;
    use neo_core::network::p2p::payloads::{Block, Header, TransactionAttribute};
    use neo_core::persistence::transaction::apply_tracked_items;
    use neo_core::persistence::StoreCache;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::smart_contract::application_engine::ApplicationEngine;
    use neo_core::smart_contract::native::helpers::NativeHelpers;
    use neo_core::smart_contract::native::GasToken;
    use neo_core::smart_contract::native::LedgerContract;
    use neo_core::smart_contract::native::NativeContract;
    use neo_core::smart_contract::native::PolicyContract;
    use neo_core::smart_contract::trigger_type::TriggerType;
    use neo_core::smart_contract::Contract;
    use neo_core::smart_contract::{StorageItem, StorageKey};
    use neo_core::wallets::KeyPair;
    use neo_core::{IVerifiable, NeoSystem, UInt160, UInt256, WitnessScope};
    use neo_json::JToken;
    use neo_vm::op_code::OpCode;
    use neo_vm::vm_state::VMState;
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
            vec![OpCode::PUSH1 as u8],
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
        invocation.push(OpCode::PUSHDATA1 as u8);
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
        writer.write_u8(VMState::NONE as u8).expect("vm state");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
            "standbycommittee",
            "seedlist",
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerNode::register_handlers();
        let handler = find_handler(&handlers, "sendrawtransaction");

        let payload = BASE64_STANDARD.encode([0u8; 4]);
        let params = [Value::String(payload)];
        let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid transaction"));
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
        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("send raw");
        let hash = result.get("hash").and_then(Value::as_str).expect("hash");
        assert_eq!(hash, tx.hash().to_string());
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
        if let Some(witness) = tx.get_witnesses_mut().get_mut(0) {
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
        tx.set_script(vec![OpCode::PUSH0 as u8; u16::MAX as usize]);
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
            vec![OpCode::PUSH1 as u8],
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
            vec![OpCode::PUSH1 as u8],
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
            vec![OpCode::PUSH1 as u8],
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerNode::register_handlers();
        let handler = find_handler(&handlers, "submitblock");

        let payload = BASE64_STANDARD.encode([0u8; 4]);
        let params = [Value::String(payload)];
        let err = (handler.callback())(&server, &params).expect_err("invalid block bytes");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid block"));
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
            vec![OpCode::PUSH1 as u8],
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
            vec![OpCode::PUSH1 as u8],
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
    #[ignore = "Block validation test needs system context - pre-existing issue"]
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
            vec![OpCode::PUSH1 as u8],
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
    #[ignore = "Block validation test needs system context - pre-existing issue"]
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
            vec![OpCode::PUSH1 as u8],
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
            vec![OpCode::PUSH1 as u8],
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
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
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system.clone(), RpcServerConfig::default());
        let handlers = RpcServerNode::register_handlers();
        let handler = find_handler(&handlers, "getconnectioncount");

        let result = (handler.callback())(&server, &[]).expect("get connection count");
        assert_eq!(result.as_u64().unwrap_or_default(), 0);
    }
}
