//! Main settings for Neo node configuration

use crate::{
    ConfigError, ConfigResult, ConsensusSettings, GenesisConfig, LoggingSettings, NetworkConfig,
    NetworkType, NodeSettings, ProtocolSettings, RpcSettings, StorageSettings, TelemetrySettings,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Complete node settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Node identification
    #[serde(default)]
    pub node: NodeSettings,

    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Protocol settings.
    ///
    /// Not (de)serialized in the node-settings TOML: the canonical typed
    /// `ProtocolSettings` (committee as `ECPoint`s, hardforks as a typed map) is
    /// loaded from the C#-compatible protocol config (JSON) via
    /// `ProtocolSettings::load`, not the TOML node config. Defaulted here.
    #[serde(skip)]
    pub protocol: ProtocolSettings,

    /// Genesis configuration
    #[serde(default)]
    pub genesis: GenesisConfig,

    /// Storage settings
    #[serde(default)]
    pub storage: StorageSettings,

    /// RPC server settings
    #[serde(default)]
    pub rpc: RpcSettings,

    /// Consensus settings
    #[serde(default)]
    pub consensus: ConsensusSettings,

    /// Logging settings
    #[serde(default)]
    pub logging: LoggingSettings,

    /// Telemetry settings
    #[serde(default)]
    pub telemetry: TelemetrySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self::for_network(NetworkType::MainNet)
    }
}

impl Settings {
    /// Create settings for a specific network
    #[must_use]
    pub fn for_network(network_type: NetworkType) -> Self {
        let (protocol, genesis, network) = match network_type {
            NetworkType::MainNet => (
                ProtocolSettings::mainnet(),
                GenesisConfig::mainnet(),
                NetworkConfig::for_network(NetworkType::MainNet),
            ),
            NetworkType::TestNet => (
                ProtocolSettings::testnet(),
                GenesisConfig::testnet(),
                NetworkConfig::for_network(NetworkType::TestNet),
            ),
            NetworkType::Private => (
                ProtocolSettings::private(0x01020304),
                GenesisConfig::default(),
                NetworkConfig::for_network(NetworkType::Private),
            ),
        };

        // Update node settings based on network
        let node = NodeSettings {
            p2p_port: match network_type {
                NetworkType::MainNet => 10333,
                NetworkType::TestNet => 20333,
                NetworkType::Private => 30333,
            },
            ..Default::default()
        };

        let rpc = RpcSettings {
            port: match network_type {
                NetworkType::MainNet => 10332,
                NetworkType::TestNet => 20332,
                NetworkType::Private => 30332,
            },
            ..Default::default()
        };

        Self {
            node,
            network,
            protocol,
            genesis,
            storage: StorageSettings::default(),
            rpc,
            consensus: ConsensusSettings::default(),
            logging: LoggingSettings::default(),
            telemetry: TelemetrySettings::default(),
        }
    }

    /// Load settings from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path)?;
        let settings: Self = toml::from_str(&content)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Load settings from a TOML string
    pub fn from_toml_str(content: &str) -> ConfigResult<Self> {
        content.parse()
    }

    /// Save settings to a TOML file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> ConfigResult<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Convert to TOML string
    pub fn to_toml(&self) -> ConfigResult<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Validate settings
    pub fn validate(&self) -> ConfigResult<()> {
        // Validate node settings
        if self.node.p2p_port == 0 {
            return Err(ConfigError::InvalidValue(
                "P2P port cannot be 0".to_string(),
            ));
        }

        // Validate RPC settings
        if self.rpc.enabled && self.rpc.port == 0 {
            return Err(ConfigError::InvalidValue(
                "RPC port cannot be 0".to_string(),
            ));
        }

        // Validate consensus settings
        if self.consensus.enabled && self.consensus.wallet_path.is_none() {
            return Err(ConfigError::MissingField(
                "consensus.wallet_path is required when consensus is enabled".to_string(),
            ));
        }

        // Validate genesis
        self.genesis.validate()?;

        Ok(())
    }

    /// Get the effective network magic
    #[must_use]
    pub fn network_magic(&self) -> u32 {
        self.network.effective_magic()
    }

    /// Get the effective address version
    #[must_use]
    pub fn address_version(&self) -> u8 {
        self.network.effective_address_version()
    }

    /// Get P2P socket address.
    ///
    /// Returns [`ConfigError::InvalidValue`] when the configured listen address
    /// and port do not form a parseable socket address, rather than panicking.
    pub fn p2p_socket_addr(&self) -> Result<std::net::SocketAddr, ConfigError> {
        let raw = format!("{}:{}", self.node.listen_address, self.node.p2p_port);
        raw.parse()
            .map_err(|_| ConfigError::InvalidValue(format!("invalid P2P socket address: {raw}")))
    }

    /// Get RPC socket address.
    ///
    /// Returns [`ConfigError::InvalidValue`] when the configured RPC address and
    /// port do not form a parseable socket address, rather than panicking.
    pub fn rpc_socket_addr(&self) -> Result<std::net::SocketAddr, ConfigError> {
        let raw = format!("{}:{}", self.rpc.address, self.rpc.port);
        raw.parse()
            .map_err(|_| ConfigError::InvalidValue(format!("invalid RPC socket address: {raw}")))
    }
}

impl std::str::FromStr for Settings {
    type Err = ConfigError;

    fn from_str(content: &str) -> Result<Self, Self::Err> {
        let settings: Self = toml::from_str(content)?;
        settings.validate()?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert!(settings.validate().is_ok());
        assert_eq!(settings.node.p2p_port, 10333);
    }

    #[test]
    fn test_testnet_settings() {
        let settings = Settings::for_network(NetworkType::TestNet);
        assert_eq!(settings.node.p2p_port, 20333);
        assert_eq!(settings.rpc.port, 20332);
    }

    #[test]
    fn test_toml_roundtrip() {
        let settings = Settings::default();
        let toml = settings.to_toml().unwrap();
        let parsed = Settings::from_toml_str(&toml).unwrap();
        assert_eq!(settings.node.p2p_port, parsed.node.p2p_port);
    }

    #[test]
    fn test_validation() {
        let mut settings = Settings::default();
        settings.node.p2p_port = 0;
        assert!(settings.validate().is_err());
    }
}
