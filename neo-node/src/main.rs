use std::net::SocketAddr;

use clap::Parser;
use neo_node::{default_config, run, ConfigArgs, DEFAULT_STAGE_STALE_AFTER_MS};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "neo-node", about = "Minimal Neo N3 Rust node runner")]
struct NodeArgs {
    #[command(flatten)]
    config: ConfigArgs,

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
    let mut config = default_config(&args.config);
    config.rpc_bind = args.rpc;
    config.stage_stale_after_ms = args.stage_stale_after_ms as u128;

    run(config).await
}
