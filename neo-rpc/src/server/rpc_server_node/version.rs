//! Dynamic `getversion` policy lookup.

use serde_json::{Map, Value};

use super::native_provider::{NativeNodeProvider, NodeNativeProvider, VersionPolicyValues};
use super::{RpcServerNode, request::NoParamsRequest, response::version_to_json};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;

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

fn dynamic_policy_values(server: &RpcServer) -> Result<VersionPolicyValues, RpcException> {
    if let Some(remote) = server.remote_ledger_rpc() {
        let version = remote.call("getversion", &[]).map_err(RpcException::from)?;
        return remote_version_dynamic_policy_values(&version);
    }

    let system = server.system();
    let protocol = system.settings();
    let store = system.store_cache();
    let snapshot = store.data_cache();
    NativeNodeProvider::new(system.native_contract_provider())
        .version_policy_values(snapshot, &protocol)
}

fn remote_version_dynamic_policy_values(
    version: &Value,
) -> Result<VersionPolicyValues, RpcException> {
    let protocol = version
        .get("protocol")
        .and_then(Value::as_object)
        .ok_or_else(|| internal_error("remote getversion response missing protocol object"))?;
    Ok(VersionPolicyValues {
        milliseconds_per_block: remote_protocol_u32(protocol, "msperblock")?,
        max_traceable_blocks: remote_protocol_u32(protocol, "maxtraceableblocks")?,
        max_valid_until_block_increment: remote_protocol_u32(
            protocol,
            "maxvaliduntilblockincrement",
        )?,
    })
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
