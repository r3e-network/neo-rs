use serde::{Deserialize, Serialize};

/// Metadata describing an RPC plugin exposed by the node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcPluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version string.
    pub version: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_plugin_info.rs"]
mod tests;
