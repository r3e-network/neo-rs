use std::time::Duration;
use std::fmt;

use crate::config::{self, netmode};
use zap::zapcore;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    // MinPeers is the minimum number of peers for normal operation.
    // When a node has less than this number of peers, it tries to
    // connect with some new ones.
    pub min_peers: i32,

    // AttemptConnPeers is the number of connection to try to
    // establish when the connection count drops below the MinPeers
    // value.
    pub attempt_conn_peers: i32,

    // MaxPeers is the maximum number of peers that can
    // be connected to the server.
    pub max_peers: i32,

    // The user agent of the server.
    pub user_agent: String,

    // Addresses stores the list of bind addresses for the node.
    pub addresses: Vec<config::AnnounceableAddress>,

    // The network mode the server will operate on.
    // ModePrivNet docker private network.
    // ModeTestNet Neo test network.
    // ModeMainNet Neo main network.
    pub net: netmode::Magic,

    // Relay determines whether the server is forwarding its inventory.
    pub relay: bool,

    // Seeds is a list of initial nodes used to establish connectivity.
    pub seeds: Vec<String>,

    // Maximum duration a single dial may take.
    pub dial_timeout: Duration,

    // The duration between protocol ticks with each connected peer.
    // When this is 0, the default interval of 5 seconds will be used.
    pub proto_tick_interval: Duration,

    // Interval used in pinging mechanism for syncing blocks.
    pub ping_interval: Duration,
    // Time to wait for pong(response for sent ping request).
    pub ping_timeout: Duration,

    // Level of the internal logger.
    pub log_level: zapcore::Level,

    // TimePerBlock is an interval which should pass between two successive blocks.
    pub time_per_block: Duration,

    // OracleCfg is oracle module configuration.
    pub oracle_cfg: config::OracleConfiguration,

    // P2PNotaryCfg is notary module configuration.
    pub p2p_notary_cfg: config::P2PNotary,

    // StateRootCfg is stateroot module configuration.
    pub state_root_cfg: config::StateRoot,

    // ExtensiblePoolSize is the size of the pool for extensible payloads from a single sender.
    pub extensible_pool_size: i32,

    // BroadcastFactor is the factor (0-100) for fan-out optimization.
    pub broadcast_factor: i32,

    pub neofs_block_fetcher_cfg: config::NeoFSBlockFetcher,
}

impl ServerConfig {
    // NewServerConfig creates a new ServerConfig struct
    // using the main applications config.
    pub fn new(cfg: config::Config) -> Result<ServerConfig, Box<dyn std::error::Error>> {
        let app_config = &cfg.application_configuration;
        let proto_config = &cfg.protocol_configuration;
        let addrs = app_config.get_addresses()?;
        Ok(ServerConfig {
            user_agent: cfg.generate_user_agent(),
            addresses: addrs,
            net: proto_config.magic,
            relay: app_config.relay,
            seeds: proto_config.seed_list.clone(),
            dial_timeout: app_config.p2p.dial_timeout,
            proto_tick_interval: app_config.p2p.proto_tick_interval,
            ping_interval: app_config.p2p.ping_interval,
            ping_timeout: app_config.p2p.ping_timeout,
            max_peers: app_config.p2p.max_peers,
            attempt_conn_peers: app_config.p2p.attempt_conn_peers,
            min_peers: app_config.p2p.min_peers,
            time_per_block: proto_config.time_per_block,
            oracle_cfg: app_config.oracle.clone(),
            p2p_notary_cfg: app_config.p2p_notary.clone(),
            state_root_cfg: app_config.state_root.clone(),
            extensible_pool_size: app_config.p2p.extensible_pool_size,
            broadcast_factor: app_config.p2p.broadcast_factor,
            neofs_block_fetcher_cfg: app_config.neofs_block_fetcher.clone(),
        })
    }
}
