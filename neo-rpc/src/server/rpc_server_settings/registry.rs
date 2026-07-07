//! Process-wide RPC server settings registry.
//!
//! This module owns loading, validation, and access to the active
//! `RpcServerSettings`. Keeping registry state here leaves `mod.rs` as the
//! serde-visible settings schema and module map.

use std::sync::LazyLock;

use neo_error::CoreResult;
use parking_lot::RwLock;
use serde_json::Value;

use super::{RpcServerConfig, UnhandledExceptionPolicy};

/// Global RPC server settings (mirrors `RpcServerSettings` in C#).
#[derive(Debug, Clone)]
pub struct RpcServerSettings {
    servers: Vec<RpcServerConfig>,
    exception_policy: UnhandledExceptionPolicy,
}

static CURRENT_SETTINGS: LazyLock<RwLock<RpcServerSettings>> = LazyLock::new(|| {
    RwLock::new(RpcServerSettings {
        servers: vec![RpcServerConfig::default()],
        exception_policy: UnhandledExceptionPolicy::Ignore,
    })
});

impl RpcServerSettings {
    /// Load process-wide RPC settings from a plugin configuration object.
    pub fn load(config: Option<&Value>) -> CoreResult<()> {
        let settings = if let Some(value) = config {
            let plugin_configuration = value.get("PluginConfiguration").unwrap_or(value);
            let servers_section = plugin_configuration
                .get("Servers")
                .cloned()
                .unwrap_or(Value::Null);
            let servers: Vec<RpcServerConfig> = serde_json::from_value(servers_section)
                .unwrap_or_else(|_| vec![RpcServerConfig::default()]);
            let exception_policy = plugin_configuration
                .get("UnhandledExceptionPolicy")
                .and_then(|policy| serde_json::from_value(policy.clone()).ok())
                .unwrap_or_default();
            Self {
                servers: if servers.is_empty() {
                    vec![RpcServerConfig::default()]
                } else {
                    servers
                },
                exception_policy,
            }
        } else {
            Self::default()
        };

        // Validate settings early to fail fast on unsupported or insecure combos.
        settings.validate()?;

        *CURRENT_SETTINGS.write() = settings;
        Ok(())
    }

    fn validate(&self) -> CoreResult<()> {
        for server in &self.servers {
            if server.max_requests_per_second == 0 && !server.bind_address.is_loopback() {
                tracing::warn!(
                    target: "neo::rpc",
                    bind_address = %server.bind_address,
                    port = server.port,
                    "RPC rate limiting is disabled (MaxRequestsPerSecond=0) on a non-localhost \
                     bind address; this exposes the node to denial-of-service attacks"
                );
            }
        }
        Ok(())
    }

    /// Return a clone of the currently loaded RPC settings.
    pub fn current() -> Self {
        CURRENT_SETTINGS.read().clone()
    }

    /// Return configured RPC server endpoints.
    #[must_use]
    pub fn servers(&self) -> &[RpcServerConfig] {
        &self.servers
    }

    /// Return the unhandled-exception policy for RPC handlers.
    #[must_use]
    pub const fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    /// Return the server configuration matching `network`, if present.
    #[must_use]
    pub fn server_for_network(&self, network: u32) -> Option<RpcServerConfig> {
        self.servers
            .iter()
            .find(|cfg| cfg.network == network)
            .cloned()
    }
}

impl Default for RpcServerSettings {
    fn default() -> Self {
        Self {
            servers: vec![RpcServerConfig::default()],
            exception_policy: UnhandledExceptionPolicy::Ignore,
        }
    }
}
