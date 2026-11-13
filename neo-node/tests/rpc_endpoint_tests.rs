//! Integration tests for RPC endpoints and server

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_rpc::server::{RpcServer, RpcServerConfig, RpcServerSettings};
use neo_system::NeoSystem;
use neo_system::network::p2p::ChannelsConfig;
use std::net::{IpAddr, Ipv4Addr};
use tokio::time::Duration;

/// Channels configuration that keeps the P2P layer dormant in tests:
/// no TCP listener and no desired peers.
fn offline_channels_config() -> ChannelsConfig {
    ChannelsConfig {
        tcp: None,
        min_desired_connections: 0,
        max_connections: 0,
        ..Default::default()
    }
}

fn localhost() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

#[tokio::test]
async fn test_rpc_server_settings_default() {
    let settings = RpcServerSettings::default();

    // Verify default settings are reasonable
    let servers = settings.servers();
    assert!(!servers.is_empty());
    let server = &servers[0];
    assert!(!server.bind_address.is_unspecified());
    assert!(server.port > 0);
    assert!(server.max_gas_invoke > 0);
    assert!(server.max_stack_size > 0);
}

#[tokio::test]
async fn test_rpc_server_settings_custom() {
    // Per-server settings now live on `RpcServerConfig`; the aggregate
    // `RpcServerSettings` groups them under `servers`.
    let server = RpcServerConfig {
        bind_address: localhost(),
        port: 10332,
        max_gas_invoke: 10_000_000,
        max_stack_size: 2048,
        session_enabled: false,
        session_expiration_time: 60,
        ..Default::default()
    };

    assert_eq!(server.bind_address, localhost());
    assert_eq!(server.port, 10332);
    assert_eq!(server.max_gas_invoke, 10_000_000);
    assert_eq!(server.max_stack_size, 2048);
    assert!(!server.session_enabled);
    assert_eq!(server.session_expiration_time, 60);
}

#[tokio::test]
async fn test_rpc_settings_lookup_by_network() {
    let settings = RpcServerSettings::default();
    let default_network = RpcServerConfig::default().network;

    let server = settings
        .server_for_network(default_network)
        .expect("default network server should be configured");

    assert_eq!(server.network, default_network);
    assert_eq!(server.port, RpcServerConfig::default().port);

    // Unknown networks have no matching server entry.
    assert!(settings.server_for_network(u32::MAX).is_none());
}

#[tokio::test]
async fn test_rpc_server_creation() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let rpc_settings = RpcServerConfig {
        bind_address: localhost(),
        port: 0, // Use port 0 to let OS assign a free port
        ..Default::default()
    };

    // Create RPC server (but don't start it to avoid port conflicts)
    let _rpc_server = RpcServer::new(system.clone(), rpc_settings);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_rpc_settings_validation() {
    let server = RpcServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        port: 10332,
        max_gas_invoke: 100_000_000,
        max_stack_size: 4096,
        ..Default::default()
    };

    // Verify settings are within expected ranges
    assert!(server.max_gas_invoke > 0);
    assert!(server.max_stack_size > 0);
    assert!(server.max_requests_per_second > 0);
    assert!(server.max_concurrent_connections > 0);
    assert!(!server.bind_address.is_unspecified() || server.bind_address.is_ipv4());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_rpc_server_with_system() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start the node with the P2P layer dormant
    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let rpc_settings = RpcServerConfig {
        bind_address: localhost(),
        port: 0,
        ..Default::default()
    };

    let _rpc_server = RpcServer::new(system.clone(), rpc_settings);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_rpc_multiple_settings_configurations() {
    let configs = vec![
        RpcServerConfig {
            bind_address: localhost(),
            port: 10332,
            ..Default::default()
        },
        RpcServerConfig {
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: 10333,
            max_gas_invoke: 50_000_000,
            ..Default::default()
        },
        RpcServerConfig {
            bind_address: localhost(),
            port: 20332,
            max_requests_per_second: 500,
            ..Default::default()
        },
    ];

    for config in configs {
        assert!(!config.bind_address.is_unspecified() || config.bind_address.is_ipv4());
        assert!(config.port > 0);
    }
}

#[tokio::test]
async fn test_rpc_session_settings() {
    let server = RpcServerConfig {
        bind_address: localhost(),
        port: 10332,
        session_enabled: true,
        session_expiration_time: 120,
        ..Default::default()
    };

    assert!(server.session_enabled);
    assert_eq!(server.session_expiration_time, 120);
}

#[tokio::test]
async fn test_rpc_gas_limits() {
    let test_limits = vec![1_000_000, 10_000_000, 100_000_000, 1_000_000_000];

    for limit in test_limits {
        let server = RpcServerConfig {
            bind_address: localhost(),
            port: 10332,
            max_gas_invoke: limit,
            ..Default::default()
        };

        assert_eq!(server.max_gas_invoke, limit);
    }
}

#[tokio::test]
async fn test_rpc_stack_size_limits() {
    let test_sizes = vec![512, 1024, 2048, 4096, 8192];

    for size in test_sizes {
        let server = RpcServerConfig {
            bind_address: localhost(),
            port: 10332,
            max_stack_size: size,
            ..Default::default()
        };

        assert_eq!(server.max_stack_size, size);
    }
}

#[tokio::test]
async fn test_rpc_iterator_result_limits() {
    // The former `max_iterations_count` setting no longer exists on the RPC
    // server settings; the iteration-workload bound is now expressed through
    // `max_iterator_result_items`.
    let test_limits = vec![50, 100, 500, 1000, 5000];

    for limit in test_limits {
        let server = RpcServerConfig {
            bind_address: localhost(),
            port: 10332,
            max_iterator_result_items: limit,
            ..Default::default()
        };

        assert_eq!(server.max_iterator_result_items, limit);
    }
}

#[tokio::test]
async fn test_rpc_server_lifecycle() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let rpc_settings = RpcServerConfig {
        bind_address: localhost(),
        port: 0,
        ..Default::default()
    };

    // Create server
    let _rpc_server = RpcServer::new(system.clone(), rpc_settings);

    // Simulate operation time
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Clean shutdown
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_rpc_concurrent_server_creation() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();

    // Create multiple systems with RPC servers
    let mut systems = vec![];
    let mut servers = vec![];

    for _ in 0..3 {
        let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

        let rpc_settings = RpcServerConfig {
            bind_address: localhost(),
            port: 0, // OS assigns port to avoid conflicts
            ..Default::default()
        };

        let server = RpcServer::new(system.clone(), rpc_settings);

        systems.push(system);
        servers.push(server);
    }

    // Shutdown all
    for system in systems {
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_rpc_settings_cloning() {
    let original = RpcServerConfig {
        bind_address: localhost(),
        port: 10332,
        max_gas_invoke: 50_000_000,
        max_stack_size: 2048,
        session_enabled: true,
        session_expiration_time: 120,
        ..Default::default()
    };

    let cloned = original.clone();

    assert_eq!(original.bind_address, cloned.bind_address);
    assert_eq!(original.port, cloned.port);
    assert_eq!(original.max_gas_invoke, cloned.max_gas_invoke);
    assert_eq!(original.max_stack_size, cloned.max_stack_size);
    assert_eq!(original.session_enabled, cloned.session_enabled);
    assert_eq!(original, cloned);
}
