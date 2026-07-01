//! # neo-rpc::server::rpc_server_node
//!
//! Node and network RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, internal_error, invalid_params,
};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hex;
use neo_config::ProtocolSettings;
use neo_io::{MemoryReader, Serializable};
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_network::handle::LocalNodeInfo;
use neo_payloads::{block::Block, transaction::Transaction};
use neo_primitives::hardfork::Hardfork;
use neo_storage::StorageKey;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map, Value, json};

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_node.rs"]
mod tests;

/// C# `LedgerContract.Prefix_CurrentBlock` — the current-block pointer
/// key (the prefix is `private` in `neo-native-contracts`, so the
/// documented byte value is mirrored here).
const LEDGER_PREFIX_CURRENT_BLOCK: u8 = 12;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
const POLICY_PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
const POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
const POLICY_PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Port of the C# `NeoSystemExtensions` dynamic-settings readers
/// (`GetTimePerBlock` / `GetMaxValidUntilBlockIncrement` /
/// `GetMaxTraceableBlocks`, `Neo/Extensions/NeoSystemExtensions.cs`):
/// from HF_Echidna the value is the committee-adjustable Policy
/// storage entry; before the hardfork the static `ProtocolSettings`
/// value applies.
///
/// The C# methods catch `KeyNotFoundException` from both reads inside
/// the `try` block, so two absences fall back to the static setting:
/// the ledger current-block pointer (genesis not yet persisted) and
/// the Policy entry itself (Echidna active from height 0 before
/// genesis persists). Both fallbacks are reproduced here exactly.
fn dynamic_policy_value(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    policy_prefix: u8,
    fallback: u32,
) -> Result<u32, RpcException> {
    // C# `NativeContract.Ledger.CurrentIndex(snapshot)` throws when the
    // pointer key is absent (→ settings fallback); the Rust reader
    // reports index 0 instead, so probe key presence first to keep the
    // C# fallback semantics exact.
    let pointer_key = StorageKey::new(LedgerContract::ID, vec![LEDGER_PREFIX_CURRENT_BLOCK]);
    if snapshot.get(&pointer_key).is_none() {
        return Ok(fallback);
    }
    let index = LedgerContract::new()
        .current_index(snapshot)
        .map_err(internal_error)?;
    if !settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
        return Ok(fallback);
    }
    let key = StorageKey::new(PolicyContract::ID, vec![policy_prefix]);
    match snapshot.get(&key) {
        // C# `(uint)(BigInteger)snapshot[key]`: signed little-endian
        // BigInteger bytes, range-guarded to `uint` by the Policy
        // setters; an out-of-range record is corrupt state and maps to
        // an internal error like the C# `OverflowException` would.
        Some(item) => {
            let value = BigInt::from_signed_bytes_le(&item.value_bytes());
            value.to_u32().ok_or_else(|| {
                internal_error(format!(
                    "Policy storage value under prefix {policy_prefix} is out of u32 range: {value}"
                ))
            })
        }
        None => Ok(fallback),
    }
}

fn dynamic_policy_values(server: &RpcServer) -> Result<(u32, u32, u32), RpcException> {
    if let Some(remote) = server.remote_ledger_rpc() {
        let version = remote.call("getversion", &[]).map_err(RpcException::from)?;
        return remote_version_dynamic_policy_values(&version);
    }

    let system = server.system();
    let protocol = system.settings();
    let store = system.store_cache();
    let snapshot = store.data_cache();
    Ok((
        dynamic_policy_value(
            snapshot,
            &protocol,
            POLICY_PREFIX_MILLISECONDS_PER_BLOCK,
            protocol.milliseconds_per_block,
        )?,
        dynamic_policy_value(
            snapshot,
            &protocol,
            POLICY_PREFIX_MAX_TRACEABLE_BLOCKS,
            protocol.max_traceable_blocks,
        )?,
        dynamic_policy_value(
            snapshot,
            &protocol,
            POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
            protocol.max_valid_until_block_increment,
        )?,
    ))
}

fn remote_version_dynamic_policy_values(version: &Value) -> Result<(u32, u32, u32), RpcException> {
    let protocol = version
        .get("protocol")
        .and_then(Value::as_object)
        .ok_or_else(|| internal_error("remote getversion response missing protocol object"))?;
    Ok((
        remote_protocol_u32(protocol, "msperblock")?,
        remote_protocol_u32(protocol, "maxtraceableblocks")?,
        remote_protocol_u32(protocol, "maxvaliduntilblockincrement")?,
    ))
}

fn remote_protocol_u32(
    protocol: &Map<String, Value>,
    field: &'static str,
) -> Result<u32, RpcException> {
    let value = protocol
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| internal_error(format!("remote getversion protocol.{field} is missing")))?;
    u32::try_from(value).map_err(|_| {
        internal_error(format!(
            "remote getversion protocol.{field} is out of u32 range: {value}"
        ))
    })
}

/// RPC handler group for node status and relay methods.
pub struct RpcServerNode;

impl RpcServerNode {
    /// Register node RPC handlers.
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
        // C# `RpcServer.GetPeers` (RpcServer.Node.cs): three arrays of
        // `{"address": ..., "port": ...}` objects.
        //
        // - `unconnected`: C# serves `LocalNode.GetUnconnectedPeers()`.
        //   The reth-style network service keeps no unconnected address
        //   book (no `addr`-message peer discovery yet), so the list is
        //   served empty rather than invented.
        // - `bad`: always an empty array in C# v3.9.1 (no bad-peer book).
        // - `connected`: C# serves `Remote.Address` + `ListenerTcpPort`
        //   per remote node. The handle-side tracker folds the service's
        //   `PeerConnected` events, which carry exactly that pair:
        //   outbound dials publish the dialed endpoint (the peer's
        //   listener); inbound accepts publish `(remote_ip, 0)` — the
        //   C# unknown-listener form — and the per-peer service
        //   re-publishes the upgraded
        //   `(remote_ip, advertised_listener_port)` endpoint once the
        //   version handshake captures the peer's `TcpServer`
        //   capability (see
        //   `neo_runtime::NetworkEvent::PeerConnected`). Peers whose
        //   address never became known at the handle seam are counted
        //   by `getconnectioncount` but omitted here, since fabricating
        //   an address would corrupt the shape.
        Self::with_local_node(server, |node| {
            let connected: Vec<Value> = node
                .connected_peers()
                .iter()
                .filter_map(|peer| {
                    peer.address.map(|addr| {
                        json!({
                            "address": addr.ip().to_string(),
                            "port": addr.port()})
                    })
                })
                .collect();
            json!({
                "unconnected": Vec::<Value>::new(),
                "bad": Vec::<Value>::new(),
                "connected": connected})
        })
    }

    fn get_version(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        // C# `GetVersion` reads msperblock / maxtraceableblocks /
        // maxvaliduntilblockincrement through the `NeoSystemExtensions`
        // dynamic readers (Policy storage post-Echidna, static settings
        // before), not from `ProtocolSettings` directly.
        let dynamic_settings = dynamic_policy_values(server)?;
        Self::with_local_node(server, |node| {
            let system = server.system();
            let protocol = system.settings();
            let rpc_settings = server.settings();
            let (time_per_block_ms, max_traceable_blocks, max_valid_until_block_increment) =
                dynamic_settings;

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
}
