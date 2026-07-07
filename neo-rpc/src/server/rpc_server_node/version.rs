//! C#-compatible `getversion` response construction.

use neo_config::ProtocolSettings;
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_primitives::hardfork::Hardfork;
use neo_primitives::hex_util;
use neo_storage::StorageKey;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map, Value, json};

use super::RpcServerNode;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;

/// C# `LedgerContract.Prefix_CurrentBlock` — the current-block pointer
/// key (the prefix is `private` in `neo-native-contracts`, so the
/// documented byte value is mirrored here).
pub(super) const LEDGER_PREFIX_CURRENT_BLOCK: u8 = 12;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
pub(super) const POLICY_PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
pub(super) const POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
pub(super) const POLICY_PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

impl RpcServerNode {
    pub(super) fn get_version(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
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
                            "name": format_hardfork(*fork),
                            "blockheight": height})
                    })
                })
                .collect();
            protocol_info.insert("hardforks".to_string(), Value::Array(hardforks));

            let committee: Vec<Value> = protocol
                .standby_committee
                .iter()
                .map(|point| Value::String(format_public_key(point.as_bytes())))
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
}

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
    // pointer key is absent (-> settings fallback); the Rust reader
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

fn format_hardfork(fork: Hardfork) -> String {
    format!("{fork:?}").trim_start_matches("Hf").to_string()
}

fn format_public_key(bytes: &[u8]) -> String {
    hex_util::encode_hex(bytes)
}
