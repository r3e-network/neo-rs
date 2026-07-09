//! Response construction helpers for node status RPC methods.

use crate::server::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_network::handle::LocalNodeInfo;
use neo_primitives::hardfork::Hardfork;
use neo_primitives::hex_util;
use serde_json::{Value, json};

use super::native_provider::VersionPolicyValues;

pub(super) fn version_to_json(
    node: &LocalNodeInfo,
    protocol: &ProtocolSettings,
    rpc_settings: &RpcServerConfig,
    dynamic_policy_values: VersionPolicyValues,
) -> Value {
    json!({
        "tcpport": node.port(),
        "nonce": node.nonce,
        "useragent": node.user_agent,
        "rpc": {
            "maxiteratorresultitems": rpc_settings.max_iterator_result_items,
            "sessionenabled": rpc_settings.session_enabled,
        },
        "protocol": {
            "addressversion": protocol.address_version,
            "network": protocol.network,
            "validatorscount": protocol.validators_count,
            "msperblock": dynamic_policy_values.milliseconds_per_block,
            "maxtraceableblocks": dynamic_policy_values.max_traceable_blocks,
            "maxvaliduntilblockincrement": dynamic_policy_values.max_valid_until_block_increment,
            "maxtransactionsperblock": protocol.max_transactions_per_block,
            "memorypoolmaxtransactions": protocol.memory_pool_max_transactions,
            "initialgasdistribution": protocol.initial_gas_distribution,
            "hardforks": hardforks_to_json(protocol),
            "standbycommittee": standby_committee_to_json(protocol),
            "seedlist": seed_list_to_json(protocol),
        },
    })
}

fn hardforks_to_json(protocol: &ProtocolSettings) -> Vec<Value> {
    Hardfork::all()
        .iter()
        .filter_map(|fork| {
            protocol.hardforks.get(fork).map(|height| {
                json!({
                    "name": format_hardfork(*fork),
                    "blockheight": height,
                })
            })
        })
        .collect()
}

fn standby_committee_to_json(protocol: &ProtocolSettings) -> Vec<Value> {
    protocol
        .standby_committee
        .iter()
        .map(|point| Value::String(format_public_key(point.as_bytes())))
        .collect()
}

fn seed_list_to_json(protocol: &ProtocolSettings) -> Vec<Value> {
    protocol
        .seed_list
        .iter()
        .map(|seed| Value::String(seed.clone()))
        .collect()
}

fn format_hardfork(fork: Hardfork) -> String {
    format!("{fork:?}").trim_start_matches("Hf").to_string()
}

fn format_public_key(bytes: &[u8]) -> String {
    hex_util::encode_hex(bytes)
}

pub(super) fn connection_count_to_json(node: &LocalNodeInfo) -> Value {
    json!(node.connected_peers_count())
}

pub(super) fn peers_to_json(node: &LocalNodeInfo) -> Value {
    // C# `RpcServer.GetPeers` (RpcServer.Node.cs): three arrays of
    // `{"address": ..., "port": ...}` objects.
    //
    // - `unconnected`: C# serves `LocalNode.GetUnconnectedPeers()`.
    //   The reth-style network service keeps no unconnected address
    //   book (no `addr`-message peer discovery yet), so the list is
    //   served empty rather than invented.
    // - `bad`: always an empty array in C# v3.10.1 (no bad-peer book).
    // - `connected`: C# serves `Remote.Address` + `ListenerTcpPort`
    //   per remote node. The handle-side tracker folds the service's
    //   `PeerConnected` events, which carry exactly that pair:
    //   outbound dials publish the dialed endpoint (the peer's
    //   listener); inbound accepts publish `(remote_ip, 0)` - the
    //   C# unknown-listener form - and the per-peer service
    //   re-publishes the upgraded
    //   `(remote_ip, advertised_listener_port)` endpoint once the
    //   version handshake captures the peer's `TcpServer`
    //   capability (see `neo_runtime::NetworkEvent::PeerConnected`).
    //   Peers whose address never became known at the handle seam are
    //   counted by `getconnectioncount` but omitted here, since
    //   fabricating an address would corrupt the shape.
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
}
