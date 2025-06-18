//! Neo Configuration Module
//!
//! This module provides configuration types for the Neo N3 Rust node.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Network type for Neo blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkType {
    MainNet,
    TestNet,
    Private,
}

impl Default for NetworkType {
    fn default() -> Self {
        NetworkType::TestNet
    }
}

impl NetworkType {
    /// Gets the network magic number
    pub fn magic(&self) -> u32 {
        match self {
            NetworkType::MainNet => 0x334f454e, // "NEO3" in little endian
            NetworkType::TestNet => 0x3254334e, // "N3T2" in little endian
            NetworkType::Private => 0x00000000,
        }
    }

    /// Gets the address version
    pub fn address_version(&self) -> u8 {
        match self {
            NetworkType::MainNet => 0x35,
            NetworkType::TestNet => 0x35,
            NetworkType::Private => 0x35,
        }
    }
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub consensus_enabled: bool,
    pub network_type: NetworkType,
    pub consensus_config: ConsensusConfig,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            consensus_enabled: false,
            network_type: NetworkType::TestNet,
            consensus_config: ConsensusConfig::default(),
        }
    }
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub enabled: bool,
    pub view_timeout_ms: u64,
    pub block_time_ms: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            view_timeout_ms: 10000,
            block_time_ms: 15000,
        }
    }
}

/// Ledger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerConfig {
    pub max_transactions_per_block: usize,
    pub max_block_size: usize,
    pub milliseconds_per_block: u64,
}

impl Default for LedgerConfig {
    fn default() -> Self {
        Self {
            max_transactions_per_block: 512,
            max_block_size: 1048576, // 1MB
            milliseconds_per_block: 15000, // 15 seconds
        }
    }
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub port: u16,
    pub max_outbound_connections: usize,
    pub max_inbound_connections: usize,
    pub connection_timeout_secs: u64,
    pub seed_nodes: Vec<SocketAddr>,
    pub user_agent: String,
    pub protocol_version: u32,
    pub websocket_enabled: bool,
    pub websocket_port: u16,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 20333, // TestNet port
            max_outbound_connections: 10,
            max_inbound_connections: 40,
            connection_timeout_secs: 30,
            seed_nodes: vec![
                "168.62.167.190:20333".parse().unwrap(),  // seed1t.neo.org
                "52.187.47.33:20333".parse().unwrap(),    // seed2t.neo.org
                "52.166.72.196:20333".parse().unwrap(),   // seed3t.neo.org
                "13.75.254.144:20333".parse().unwrap(),   // seed4t.neo.org
                "13.71.130.1:20333".parse().unwrap(),     // seed5t.neo.org
            ],
            user_agent: "Neo-Rust/0.1.0".to_string(),
            protocol_version: 3,
            websocket_enabled: false,
            websocket_port: 20334,
        }
    }
}

/// RPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcServerConfig {
    pub enabled: bool,
    pub port: u16,
    pub bind_address: String,
    pub max_connections: usize,
    pub cors_enabled: bool,
    pub ssl_enabled: bool,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 20332, // TestNet RPC port
            bind_address: "127.0.0.1".to_string(),
            max_connections: 50,
            cors_enabled: true,
            ssl_enabled: false,
        }
    }
}