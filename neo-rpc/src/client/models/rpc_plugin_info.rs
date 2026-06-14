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
mod tests {
    use super::*;

    #[test]
    fn rpc_plugin_info_ser_de() {
        let info = RpcPluginInfo {
            name: "RpcServer".into(),
            version: "1.0.0".into(),
            description: Some("RPC endpoint".into()),
        };

        let json = serde_json::to_string(&info).expect("serialize");
        let parsed: RpcPluginInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, info.name);
        assert_eq!(parsed.description, info.description);
    }

    #[test]
    fn rpc_plugin_info_defaults_description() {
        let json = r#"{"name": "Test", "version": "0.1.0"}"#;
        let parsed: RpcPluginInfo = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.description.is_none());
    }
}
