use anyhow::Result;
use clap::{Parser, Subcommand};
use neo_core::config::NodeConfig;
use neo_core::node::Node;
use std::path::PathBuf;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "neo-node")]
#[command(about = "Neo N3 blockchain node daemon - Professional implementation")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "R3E Network <jimmy@r3e.network>")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Network to connect to
    #[arg(short, long, env = "NEO_NETWORK", default_value = "testnet")]
    network: String,

    /// Configuration file path
    #[arg(short, long, env = "NEO_CONFIG")]
    config: Option<PathBuf>,

    /// Data directory
    #[arg(short, long, env = "NEO_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// RPC server port
    #[arg(long, env = "NEO_RPC_PORT")]
    rpc_port: Option<u16>,

    /// P2P port
    #[arg(long, env = "NEO_P2P_PORT")]
    p2p_port: Option<u16>,

    /// Logging level
    #[arg(long, env = "NEO_LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Enable metrics endpoint
    #[arg(long, env = "NEO_METRICS")]
    metrics: bool,

    /// Metrics port
    #[arg(long, env = "NEO_METRICS_PORT", default_value = "9090")]
    metrics_port: u16,

    /// Disable RPC server
    #[arg(long)]
    no_rpc: bool,

    /// Enable consensus (validator mode)
    #[arg(long, env = "NEO_CONSENSUS")]
    consensus: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the node daemon
    Start,
    /// Show node configuration
    Config,
    /// Show node version and build info
    Version,
    /// Validate configuration file
    Validate {
        /// Configuration file to validate
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.log_level));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(true)
                .with_level(true)
                .with_ansi(true),
        )
        .with(filter)
        .init();

    match args.command.unwrap_or(Commands::Start) {
        Commands::Start => start_node(args).await,
        Commands::Config => show_config(args).await,
        Commands::Version => show_version(),
        Commands::Validate { config } => validate_config(config).await,
    }
}

async fn start_node(args: Args) -> Result<()> {
    info!("Starting Neo N3 node daemon v{}", env!("CARGO_PKG_VERSION"));
    info!("Network: {}", args.network);

    // Load configuration
    let mut config = load_config(&args).await?;

    // Apply CLI overrides
    apply_cli_overrides(&mut config, &args)?;

    info!("Configuration loaded successfully");
    info!("Data directory: {:?}", config.storage.data_dir);
    info!("RPC enabled: {}", !args.no_rpc);
    info!("Consensus enabled: {}", args.consensus);

    // Create and start node
    let node = Node::new(config).await?;
    
    // Run node
    if let Err(e) = node.run().await {
        error!("Node error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

async fn load_config(args: &Args) -> Result<NodeConfig> {
    if let Some(config_path) = &args.config {
        info!("Loading configuration from: {:?}", config_path);
        NodeConfig::from_file(config_path)
    } else {
        // Load default config based on network
        let config_name = match args.network.as_str() {
            "mainnet" => "neo_mainnet_node.toml",
            "testnet" => "neo_testnet_persistent.toml",
            _ => {
                warn!("Unknown network '{}', using testnet configuration", args.network);
                "neo_testnet_persistent.toml"
            }
        };
        
        let config_path = PathBuf::from(config_name);
        if config_path.exists() {
            info!("Loading default {} configuration", args.network);
            NodeConfig::from_file(&config_path)
        } else {
            info!("Using built-in default configuration");
            Ok(NodeConfig::default())
        }
    }
}

fn apply_cli_overrides(config: &mut NodeConfig, args: &Args) -> Result<()> {
    // Override data directory
    if let Some(data_dir) = &args.data_dir {
        config.storage.data_dir = data_dir.clone();
    }

    // Override RPC port
    if let Some(rpc_port) = args.rpc_port {
        config.rpc.port = rpc_port;
    }

    // Override P2P port
    if let Some(p2p_port) = args.p2p_port {
        config.p2p.port = p2p_port;
    }

    // Disable RPC if requested
    if args.no_rpc {
        config.rpc.enabled = false;
    }

    // Enable consensus if requested
    if args.consensus {
        config.consensus.enabled = true;
    }

    // Configure metrics
    if args.metrics {
        config.telemetry.metrics.enabled = true;
        config.telemetry.metrics.port = args.metrics_port;
    }

    Ok(())
}

async fn show_config(args: Args) -> Result<()> {
    let config = load_config(&args).await?;
    println!("{}", toml::to_string_pretty(&config)?);
    Ok(())
}

fn show_version() -> Result<()> {
    println!("neo-node {}", env!("CARGO_PKG_VERSION"));
    println!("Build: {}", env!("VERGEN_BUILD_TIMESTAMP"));
    println!("Commit: {}", env!("VERGEN_GIT_SHA"));
    println!("Rust: {}", env!("VERGEN_RUSTC_SEMVER"));
    Ok(())
}

async fn validate_config(config_path: PathBuf) -> Result<()> {
    info!("Validating configuration: {:?}", config_path);
    
    match NodeConfig::from_file(&config_path) {
        Ok(_) => {
            println!("✅ Configuration is valid");
            Ok(())
        }
        Err(e) => {
            error!("❌ Configuration validation failed: {}", e);
            std::process::exit(1);
        }
    }
}
