//! Neo N3 Network Connection Test
//!
//! A simple example that demonstrates how to connect the Neo Rust node
//! to the real Neo N3 MainNet or TestNet and verify connectivity.
//!
//! Usage:
//!   cargo run --example network_connection_test

use neo_cli::{
    args::{LogLevel, Network},
    service::MainService,
    CliArgs,
};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Test both MainNet and TestNet connectivity
    for network in [Network::Mainnet, Network::Testnet] {
        let network_name = match network {
            Network::Mainnet => "Neo N3 MainNet",
            Network::Testnet => "Neo N3 TestNet",
            Network::Private => "Private Network",
        };

        info!(
            "🚀 Testing Neo Rust Node Network Connectivity for {}",
            network_name
        );
        info!("───────────────────────────────────────────");

        // Create CLI arguments for the test
        let cli_args = CliArgs {
            config: None,
            wallet: None,
            password: None,
            db_engine: None,
            db_path: None,
            no_verify: false,
            plugins: vec![],
            verbose: LogLevel::Info,
            daemon: false,
            network,
            rpc_port: None,
            p2p_port: match network {
                Network::Mainnet => Some(10333),
                Network::Testnet => Some(20333),
                Network::Private => Some(30333),
            },
            max_connections: Some(50),
            min_connections: Some(5),
            data_dir: Some(std::env::temp_dir().join("neo-rs-test")),
            show_version: false,
        };

        // Initialize the main service
        info!("🔧 Initializing Neo Rust node for {}...", network_name);
        let mut service = match MainService::new(cli_args).await {
            Ok(service) => {
                info!("✅ Neo Rust node initialized successfully");
                service
            }
            Err(e) => {
                error!("❌ Failed to initialize Neo Rust node: {}", e);
                continue; // Try next network
            }
        };

        // Start the service with timeout
        info!(
            "🚀 Starting Neo Rust node and connecting to {}...",
            network_name
        );
        info!("⏳ This may take a moment to connect to peers...");

        match timeout(Duration::from_secs(30), service.start()).await {
            Ok(Ok(_)) => {
                info!("✅ Neo Rust node started successfully!");
                info!(
                    "🔗 Node is now attempting to connect to {} peers",
                    network_name
                );
            }
            Ok(Err(e)) => {
                error!("❌ Failed to start Neo Rust node: {}", e);
                continue; // Try next network
            }
            Err(_) => {
                error!("❌ Node startup timed out after 30 seconds");
                continue; // Try next network
            }
        }

        // Monitor connectivity for a short duration
        info!("📊 Monitoring network connectivity for 20 seconds...");
        let start_time = std::time::Instant::now();
        let mut connection_established = false;

        while start_time.elapsed().as_secs() < 20 {
            sleep(Duration::from_secs(2)).await;

            // Check connectivity status
            if let Some(p2p_node) = service.p2p_node() {
                let peer_manager = p2p_node.peer_manager();
                let stats = peer_manager.get_stats().await;
                let peer_count = stats.connected_peers;

                if peer_count > 0 && !connection_established {
                    info!(
                        "🎉 Successfully connected to {} network with {} peers!",
                        network_name, peer_count
                    );
                    connection_established = true;
                    break;
                }
            }
        }

        // Final status report
        info!("───────────────────────────────────────────");
        if connection_established {
            info!(
                "🎯 SUCCESS: Neo Rust node successfully connected to {}!",
                network_name
            );
        } else {
            warn!("⚠️  No peer connections established for {}", network_name);
            warn!("   This could be due to network connectivity or seed node availability");
        }

        // Graceful shutdown
        info!("🛑 Shutting down Neo Rust node...");
        if let Err(e) = service.stop().await {
            warn!("Warning during shutdown: {}", e);
        }
        info!("✅ Shutdown complete");

        // Wait a moment before testing next network
        sleep(Duration::from_secs(2)).await;
    }

    // Overall test result
    info!("🏁 Neo N3 Network Connectivity Test Completed!");
    Ok(())
}
