use anyhow::Result;
use clap::{Parser, Subcommand};
use neo_rpc::client::RpcClient;
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;
use std::str::FromStr;
use url::Url;

#[derive(Parser)]
#[command(name = "neo-cli")]
#[command(about = "Neo N3 blockchain command-line client - Professional implementation")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "R3E Network <jimmy@r3e.network>")]
struct Args {
    /// RPC server URL
    #[arg(short, long, env = "NEO_RPC_URL", default_value = "http://127.0.0.1:20332")]
    rpc_url: String,

    /// Output format
    #[arg(short, long, default_value = "json")]
    format: OutputFormat,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Debug)]
enum OutputFormat {
    Json,
    Table,
    Raw,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "table" => Ok(OutputFormat::Table),
            "raw" => Ok(OutputFormat::Raw),
            _ => Err(format!("Invalid format: {}", s)),
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Node information commands
    Node {
        #[command(subcommand)]
        command: NodeCommands,
    },
    /// Blockchain query commands
    Blockchain {
        #[command(subcommand)]
        command: BlockchainCommands,
    },
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        command: WalletCommands,
    },
    /// Smart contract operations
    Contract {
        #[command(subcommand)]
        command: ContractCommands,
    },
    /// Network and peer information
    Network {
        #[command(subcommand)]
        command: NetworkCommands,
    },
}

#[derive(Subcommand)]
enum NodeCommands {
    /// Get node version
    Version,
    /// Get node status
    Status,
    /// Get connection count
    Connections,
    /// List connected peers
    Peers,
}

#[derive(Subcommand)]
enum BlockchainCommands {
    /// Get current block count
    Height,
    /// Get best block hash
    BestHash,
    /// Get block by hash or index
    Block {
        /// Block hash or index
        identifier: String,
        /// Include verbose transaction data
        #[arg(short, long)]
        verbose: bool,
    },
    /// Get transaction by hash
    Transaction {
        /// Transaction hash
        hash: String,
        /// Include verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Get account state
    Account {
        /// Account address or script hash
        address: String,
    },
}

#[derive(Subcommand)]
enum WalletCommands {
    /// Get wallet balance
    Balance {
        /// Asset hash (optional, defaults to NEO and GAS)
        asset: Option<String>,
    },
    /// List wallet addresses
    Addresses,
    /// Create new address
    NewAddress,
    /// Send assets
    Send {
        /// Recipient address
        to: String,
        /// Asset hash
        asset: String,
        /// Amount to send
        amount: String,
    },
}

#[derive(Subcommand)]
enum ContractCommands {
    /// Invoke contract method (read-only)
    Invoke {
        /// Contract hash
        contract: String,
        /// Method name
        method: String,
        /// Parameters (JSON array)
        #[arg(short, long)]
        params: Option<String>,
    },
    /// Get contract state
    State {
        /// Contract hash
        contract: String,
    },
    /// Get contract storage
    Storage {
        /// Contract hash
        contract: String,
        /// Storage key (hex)
        key: String,
    },
}

#[derive(Subcommand)]
enum NetworkCommands {
    /// Get network information
    Info,
    /// Get peer information
    Peers,
    /// Get mempool information
    Mempool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Validate RPC URL
    let _url = Url::parse(&args.rpc_url)
        .map_err(|e| anyhow::anyhow!("Invalid RPC URL '{}': {}", args.rpc_url, e))?;

    let client = RpcClient::new(&args.rpc_url)?;

    match args.command {
        Commands::Node { command } => handle_node_commands(command, &client, &args).await,
        Commands::Blockchain { command } => handle_blockchain_commands(command, &client, &args).await,
        Commands::Wallet { command } => handle_wallet_commands(command, &client, &args).await,
        Commands::Contract { command } => handle_contract_commands(command, &client, &args).await,
        Commands::Network { command } => handle_network_commands(command, &client, &args).await,
    }
}

async fn handle_node_commands(command: NodeCommands, client: &RpcClient, args: &Args) -> Result<()> {
    match command {
        NodeCommands::Version => {
            let result = client.get_version().await?;
            print_result(&result, &args.format)?;
        }
        NodeCommands::Status => {
            let height = client.get_block_count().await?;
            let connections = client.get_connection_count().await?;
            let best_hash = client.get_best_block_hash().await?;
            
            let status = serde_json::json!({
                "height": height,
                "connections": connections,
                "best_block_hash": best_hash,
                "syncing": false // TODO: Implement sync status
            });
            print_result(&status, &args.format)?;
        }
        NodeCommands::Connections => {
            let result = client.get_connection_count().await?;
            print_result(&result, &args.format)?;
        }
        NodeCommands::Peers => {
            let result = client.get_peers().await?;
            print_result(&result, &args.format)?;
        }
    }
    Ok(())
}

async fn handle_blockchain_commands(command: BlockchainCommands, client: &RpcClient, args: &Args) -> Result<()> {
    match command {
        BlockchainCommands::Height => {
            let result = client.get_block_count().await?;
            print_result(&result, &args.format)?;
        }
        BlockchainCommands::BestHash => {
            let result = client.get_best_block_hash().await?;
            print_result(&result, &args.format)?;
        }
        BlockchainCommands::Block { identifier, verbose } => {
            let result = if let Ok(index) = identifier.parse::<u32>() {
                client.get_block_by_index(index, verbose).await?
            } else {
                let hash = UInt256::from_str(&identifier)
                    .map_err(|_| anyhow::anyhow!("Invalid block hash or index: {}", identifier))?;
                client.get_block_by_hash(hash, verbose).await?
            };
            print_result(&result, &args.format)?;
        }
        BlockchainCommands::Transaction { hash, verbose } => {
            let tx_hash = UInt256::from_str(&hash)
                .map_err(|_| anyhow::anyhow!("Invalid transaction hash: {}", hash))?;
            let result = client.get_raw_transaction(tx_hash, verbose).await?;
            print_result(&result, &args.format)?;
        }
        BlockchainCommands::Account { address } => {
            // Try to parse as script hash first, then as address
            let script_hash = if address.starts_with("0x") {
                UInt160::from_str(&address)
                    .map_err(|_| anyhow::anyhow!("Invalid script hash: {}", address))?
            } else {
                // TODO: Convert address to script hash
                return Err(anyhow::anyhow!("Address to script hash conversion not implemented"));
            };
            
            let result = client.get_nep17_balances(script_hash).await?;
            print_result(&result, &args.format)?;
        }
    }
    Ok(())
}

async fn handle_wallet_commands(command: WalletCommands, _client: &RpcClient, _args: &Args) -> Result<()> {
    match command {
        WalletCommands::Balance { asset: _ } => {
            println!("Wallet commands require an open wallet. Use RPC server wallet functionality.");
        }
        WalletCommands::Addresses => {
            println!("Wallet commands require an open wallet. Use RPC server wallet functionality.");
        }
        WalletCommands::NewAddress => {
            println!("Wallet commands require an open wallet. Use RPC server wallet functionality.");
        }
        WalletCommands::Send { to: _, asset: _, amount: _ } => {
            println!("Wallet commands require an open wallet. Use RPC server wallet functionality.");
        }
    }
    Ok(())
}

async fn handle_contract_commands(command: ContractCommands, client: &RpcClient, args: &Args) -> Result<()> {
    match command {
        ContractCommands::Invoke { contract, method, params } => {
            let contract_hash = UInt160::from_str(&contract)
                .map_err(|_| anyhow::anyhow!("Invalid contract hash: {}", contract))?;
            
            let params_array = if let Some(params_str) = params {
                serde_json::from_str(&params_str)
                    .map_err(|e| anyhow::anyhow!("Invalid parameters JSON: {}", e))?
            } else {
                serde_json::Value::Array(vec![])
            };
            
            let result = client.invoke_function(contract_hash, &method, params_array).await?;
            print_result(&result, &args.format)?;
        }
        ContractCommands::State { contract } => {
            let contract_hash = UInt160::from_str(&contract)
                .map_err(|_| anyhow::anyhow!("Invalid contract hash: {}", contract))?;
            let result = client.get_contract_state(contract_hash).await?;
            print_result(&result, &args.format)?;
        }
        ContractCommands::Storage { contract, key } => {
            let contract_hash = UInt160::from_str(&contract)
                .map_err(|_| anyhow::anyhow!("Invalid contract hash: {}", contract))?;
            let result = client.get_storage(contract_hash, &key).await?;
            print_result(&result, &args.format)?;
        }
    }
    Ok(())
}

async fn handle_network_commands(command: NetworkCommands, client: &RpcClient, args: &Args) -> Result<()> {
    match command {
        NetworkCommands::Info => {
            let version = client.get_version().await?;
            let height = client.get_block_count().await?;
            let connections = client.get_connection_count().await?;
            
            let info = serde_json::json!({
                "version": version,
                "height": height,
                "connections": connections
            });
            print_result(&info, &args.format)?;
        }
        NetworkCommands::Peers => {
            let result = client.get_peers().await?;
            print_result(&result, &args.format)?;
        }
        NetworkCommands::Mempool => {
            let result = client.get_raw_mempool(true).await?;
            print_result(&result, &args.format)?;
        }
    }
    Ok(())
}

fn print_result(result: &Value, format: &OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
        OutputFormat::Table => {
            // TODO: Implement table formatting
            println!("{}", serde_json::to_string_pretty(result)?);
        }
        OutputFormat::Raw => {
            if let Some(s) = result.as_str() {
                println!("{}", s);
            } else if let Some(n) = result.as_u64() {
                println!("{}", n);
            } else {
                println!("{}", result);
            }
        }
    }
    Ok(())
}
