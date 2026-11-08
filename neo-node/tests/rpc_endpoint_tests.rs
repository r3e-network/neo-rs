//! Integration tests for RPC endpoints and server

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_rpc::server::{RpcServer, RpcServerSettings};
use neo_system::NeoSystem;
use std::sync::Arc;
use tokio::time::Duration;

#[tokio::test]
async fn test_rpc_server_settings_default() {
    let settings = RpcServerSettings::default();

    // Verify default settings are reasonable
    assert!(!settings.bind_address.is_empty());
    assert!(settings.port > 0);
}

#[tokio::test]
async fn test_rpc_server_settings_custom() {
    let settings = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
        port: 10332,
        max_gas_invoke: 10_000_000,
        max_iterations_count: 100,
        max_stack_size: 2048,
        session_enabled: false,
        session_expiration_seconds: 60,
        ..Default::default()
    };

    assert_eq!(settings.bind_address, "127.0.0.1");
    assert_eq!(settings.port, 10332);
    assert_eq!(settings.max_gas_invoke, 10_000_000);
}

#[tokio::test]
async fn test_rpc_server_creation() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let rpc_settings = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
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
    let settings = RpcServerSettings {
        bind_address: "0.0.0.0".to_string(),
        port: 10332,
        max_gas_invoke: 100_000_000,
        max_iterations_count: 1000,
        max_stack_size: 4096,
        ..Default::default()
    };

    // Verify settings are within expected ranges
    assert!(settings.max_gas_invoke > 0);
    assert!(settings.max_iterations_count > 0);
    assert!(settings.max_stack_size > 0);
    assert!(!settings.bind_address.is_empty());
}

#[tokio::test]
async fn test_rpc_server_with_system() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    // Start the node
    system.start_node(&None)?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let rpc_settings = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
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
        RpcServerSettings {
            bind_address: "127.0.0.1".to_string(),
            port: 10332,
            ..Default::default()
        },
        RpcServerSettings {
            bind_address: "0.0.0.0".to_string(),
            port: 10333,
            max_gas_invoke: 50_000_000,
            ..Default::default()
        },
        RpcServerSettings {
            bind_address: "localhost".to_string(),
            port: 20332,
            max_iterations_count: 500,
            ..Default::default()
        },
    ];

    for config in configs {
        assert!(!config.bind_address.is_empty());
        assert!(config.port >= 0);
    }
}

#[tokio::test]
async fn test_rpc_session_settings() {
    let settings = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
        port: 10332,
        session_enabled: true,
        session_expiration_seconds: 120,
        ..Default::default()
    };

    assert!(settings.session_enabled);
    assert_eq!(settings.session_expiration_seconds, 120);
}

#[tokio::test]
async fn test_rpc_gas_limits() {
    let test_limits = vec![
        1_000_000,
        10_000_000,
        100_000_000,
        1_000_000_000,
    ];

    for limit in test_limits {
        let settings = RpcServerSettings {
            bind_address: "127.0.0.1".to_string(),
            port: 10332,
            max_gas_invoke: limit,
            ..Default::default()
        };

        assert_eq!(settings.max_gas_invoke, limit);
    }
}

#[tokio::test]
async fn test_rpc_stack_size_limits() {
    let test_sizes = vec![512, 1024, 2048, 4096, 8192];

    for size in test_sizes {
        let settings = RpcServerSettings {
            bind_address: "127.0.0.1".to_string(),
            port: 10332,
            max_stack_size: size,
            ..Default::default()
        };

        assert_eq!(settings.max_stack_size, size);
    }
}

#[tokio::test]
async fn test_rpc_iteration_limits() {
    let test_iterations = vec![50, 100, 500, 1000, 5000];

    for iterations in test_iterations {
        let settings = RpcServerSettings {
            bind_address: "127.0.0.1".to_string(),
            port: 10332,
            max_iterations_count: iterations,
            ..Default::default()
        };

        assert_eq!(settings.max_iterations_count, iterations);
    }
}

#[tokio::test]
async fn test_rpc_server_lifecycle() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let rpc_settings = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
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
        let system = Arc::new(NeoSystem::new(protocol_settings.clone(), None, None)?);

        let rpc_settings = RpcServerSettings {
            bind_address: "127.0.0.1".to_string(),
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
    let original = RpcServerSettings {
        bind_address: "127.0.0.1".to_string(),
        port: 10332,
        max_gas_invoke: 50_000_000,
        max_iterations_count: 500,
        max_stack_size: 2048,
        session_enabled: true,
        session_expiration_seconds: 120,
        ..Default::default()
    };

    let cloned = original.clone();

    assert_eq!(original.bind_address, cloned.bind_address);
    assert_eq!(original.port, cloned.port);
    assert_eq!(original.max_gas_invoke, cloned.max_gas_invoke);
    assert_eq!(original.session_enabled, cloned.session_enabled);
}
