//! Dynamic `getversion` policy lookup.

use neo_blockchain::{ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory};
use neo_config::ProtocolSettings;
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_primitives::hardfork::Hardfork;
use neo_storage::StorageKey;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map, Value};

use super::{RpcServerNode, request::NoParamsRequest, response::version_to_json};
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
    pub(super) fn get_version(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getversion")?;
        // C# `GetVersion` reads msperblock / maxtraceableblocks /
        // maxvaliduntilblockincrement through the `NeoSystemExtensions`
        // dynamic readers (Policy storage post-Echidna, static settings
        // before), not from `ProtocolSettings` directly.
        let dynamic_settings = dynamic_policy_values(server)?;
        Self::with_local_node(server, |node| {
            let system = server.system();
            let protocol = system.settings();
            let rpc_settings = server.settings();
            version_to_json(node, &protocol, rpc_settings, dynamic_settings)
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
    let provider = StorageLedgerProviderFactory.provider(snapshot);
    let index = provider.current_index().map_err(internal_error)?;
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
