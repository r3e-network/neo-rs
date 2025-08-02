//! Example of using snapshot-based sync for fast blockchain synchronization

use neo_network::snapshot_config::{example_mainnet_config, SnapshotConfig};
use neo_network::sync::SyncManager;
use std::sync::Arc;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create blockchain and P2P node instances (placeholder)
    // In a real application, these would be properly initialized
    // let blockchain = Arc::new(neo_ledger::Blockchain::new(...));
    // let p2p_node = Arc::new(neo_network::P2pNode::new(...));

    // Create sync manager
    // let sync_manager = SyncManager::new(blockchain, p2p_node);

    // Option 1: Load snapshot config from file
    // sync_manager.load_snapshot_config("snapshots.json").await?;

    // Option 2: Use example mainnet configuration
    let snapshot_config = example_mainnet_config();
    println!("Example snapshot configuration:");
    println!("{}", serde_json::to_string_pretty(&snapshot_config)?);

    // Option 3: Create custom snapshot configuration
    let custom_config = SnapshotConfig {
        providers: vec![
            // Add your snapshot providers here
        ],
        min_trust_level: 90,
        max_age_seconds: 3 * 24 * 3600, // 3 days
        preferred_compression: vec!["zstd".to_string()],
    };

    // Set the custom configuration
    // sync_manager.set_snapshot_config(custom_config).await;

    // Start sync - will automatically use snapshot if beneficial
    // sync_manager.start_sync().await?;

    println!("\nSnapshot sync configuration example completed!");
    println!("\nTo use in your application:");
    println!("1. Create a snapshot configuration file or use the default");
    println!("2. Load it into the sync manager");
    println!("3. Start sync - it will automatically use snapshots when beneficial");
    println!("\nSnapshot sync benefits:");
    println!("- Fast initial sync by downloading pre-validated state");
    println!("- Reduces sync time from days to hours");
    println!("- Automatic checksum verification");
    println!("- Falls back to normal sync if snapshot fails");

    Ok(())
}
