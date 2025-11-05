use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use neo_node::{run, NodeConfig, DEFAULT_STAGE_STALE_AFTER_MS};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "neo-node", about = "Minimal Neo N3 Rust node runner")]
struct NodeArgs {
    /// Network name (mainnet, testnet, privatenet)
    #[arg(long, default_value = "mainnet")]
    network: String,

    /// Override the consensus network magic value
    #[arg(long)]
    network_magic: Option<u32>,

    /// Path to store consensus snapshots
    #[arg(long)]
    snapshot_path: Option<PathBuf>,

    /// Address to bind the RPC endpoint to
    #[arg(long, default_value = "127.0.0.1:20332")]
    rpc: SocketAddr,

    /// Consider a consensus stage stale after this many milliseconds (0 disables)
    #[arg(long, default_value_t = DEFAULT_STAGE_STALE_AFTER_MS as u64)]
    stage_stale_after_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("neo-node=info".parse()?))
        .init();

    let args = NodeArgs::parse();
    let magic = args
        .network_magic
        .unwrap_or_else(|| NodeConfig::magic_for_network(&args.network));
    let snapshot_path = args
        .snapshot_path
        .unwrap_or_else(|| NodeConfig::default_snapshot_path(&args.network));
    let config = NodeConfig {
        network: args.network,
        network_magic: magic,
        rpc_bind: args.rpc,
        snapshot_path,
        stage_stale_after_ms: args.stage_stale_after_ms as u128,
        validators: None,
    };

    run(config).await
}
