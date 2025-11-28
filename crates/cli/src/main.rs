//! Neo CLI - Command-line client for Neo N3 blockchain nodes
//!
//! This CLI tool communicates with a Neo node via JSON-RPC.
//!
//! Usage:
//!   neo-cli [OPTIONS] <COMMAND> [ARGS]
//!
//! Examples:
//!   neo-cli version                          # Get node version
//!   neo-cli state                            # Show node state
//!   neo-cli block 1000                       # Get block by index
//!   neo-cli tx <hash>                        # Get transaction
//!   neo-cli --rpc-url http://localhost:10332 state

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use neo_rpc_client::RpcClient;
use url::Url;

mod commands;

use commands::*;

/// Neo CLI - Command-line client for Neo N3 blockchain nodes
#[derive(Parser, Debug)]
#[command(name = "neo-cli", version, about, long_about = None)]
struct Cli {
    /// RPC server URL
    #[arg(
        long,
        short = 'u',
        default_value = "http://localhost:10332",
        global = true
    )]
    rpc_url: String,

    /// RPC basic auth username
    #[arg(long, global = true)]
    rpc_user: Option<String>,

    /// RPC basic auth password
    #[arg(long, global = true)]
    rpc_pass: Option<String>,

    /// Output format (json, table, plain)
    #[arg(long, short = 'o', default_value = "plain", global = true)]
    output: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Table,
    Plain,
}

#[derive(Subcommand, Debug)]
enum Commands {
    // ==================== Node Information ====================
    /// Get node version information
    Version,

    /// Show node state (block height, network, etc.)
    State,

    /// Show connected peers (alias: show node)
    Peers,

    /// Show memory pool status (alias: show pool)
    Mempool {
        /// Show verbose transaction list
        #[arg(long, short = 'v')]
        verbose: bool,
    },

    /// List loaded plugins on the node
    Plugins,

    // ==================== Blockchain Queries ====================
    /// Get block by index or hash (alias: show block)
    Block {
        /// Block index (number) or hash
        index_or_hash: String,

        /// Output raw hex instead of parsed block
        #[arg(long)]
        raw: bool,
    },

    /// Get block header by index or hash
    Header {
        /// Block index (number) or hash
        index_or_hash: String,

        /// Output raw hex instead of parsed header
        #[arg(long)]
        raw: bool,
    },

    /// Get transaction by hash (alias: show tx)
    Tx {
        /// Transaction hash (hex string)
        hash: String,

        /// Output raw hex instead of parsed transaction
        #[arg(long)]
        raw: bool,
    },

    /// Get contract state by hash (alias: show contract)
    Contract {
        /// Contract script hash or name
        hash: String,
    },

    /// Get best block hash
    BestBlockHash,

    /// Get block count (height + 1)
    BlockCount,

    /// Get block hash by index
    BlockHash {
        /// Block index
        index: u32,
    },

    // ==================== Token Operations ====================
    /// Get NEP-17 token balances for an address
    Balance {
        /// Account address
        address: String,
    },

    /// Get NEP-17 transfer history for an address
    Transfers {
        /// Account address
        address: String,

        /// Start timestamp (optional)
        #[arg(long)]
        from: Option<u64>,

        /// End timestamp (optional)
        #[arg(long)]
        to: Option<u64>,
    },

    /// Show unclaimed GAS for an address (alias: show gas)
    Gas {
        /// Account address
        address: String,
    },

    // ==================== Contract Invocation ====================
    /// Invoke a contract method (read-only)
    Invoke {
        /// Contract script hash
        hash: String,

        /// Method name
        method: String,

        /// Method parameters as JSON array
        #[arg(default_value = "[]")]
        params: String,
    },

    /// Test invoke a script (read-only, alias: testinvoke)
    TestInvoke {
        /// Base64-encoded script
        script: String,
    },

    // ==================== Wallet Commands ====================
    /// Wallet management commands
    #[command(subcommand)]
    Wallet(WalletCommands),

    // ==================== Send/Transfer ====================
    /// Send assets to an address
    Send {
        /// Asset hash (e.g., NEO, GAS, or contract hash)
        asset: String,

        /// Recipient address
        to: String,

        /// Amount to send
        amount: String,

        /// Sender address (optional, uses default if not specified)
        #[arg(long)]
        from: Option<String>,
    },

    /// Transfer NEP-17 tokens
    Transfer {
        /// Token contract hash
        token: String,

        /// Recipient address
        to: String,

        /// Amount to transfer
        amount: String,

        /// Sender address (optional)
        #[arg(long)]
        from: Option<String>,

        /// Additional data (optional)
        #[arg(long)]
        data: Option<String>,
    },

    // ==================== Voting ====================
    /// Vote for a candidate
    Vote {
        /// Voter address
        address: String,

        /// Candidate public key (hex)
        pubkey: String,
    },

    /// Remove vote
    Unvote {
        /// Voter address
        address: String,
    },

    /// Register as a candidate
    RegisterCandidate {
        /// Public key to register
        pubkey: String,
    },

    /// Unregister as a candidate
    UnregisterCandidate {
        /// Public key to unregister
        pubkey: String,
    },

    /// Get list of candidates
    Candidates,

    /// Get committee members
    Committee,

    /// Get next block validators
    Validators,

    // ==================== Native Contracts ====================
    /// List native contracts
    NativeContracts,

    /// Query NEO token info
    Neo {
        #[command(subcommand)]
        cmd: NeoTokenCmd,
    },

    /// Query GAS token info
    GasToken {
        #[command(subcommand)]
        cmd: GasTokenCmd,
    },

    // ==================== Tools ====================
    /// Parse address, script hash, or public key
    Parse {
        /// Value to parse
        value: String,
    },

    /// Parse and disassemble a script
    ParseScript {
        /// Base64-encoded script
        script: String,
    },

    /// Validate an address
    ValidateAddress {
        /// Address to validate
        address: String,
    },

    /// Sign data with a private key
    Sign {
        /// Data to sign (hex)
        data: String,

        /// Private key (WIF format)
        #[arg(long)]
        key: String,
    },

    /// Relay a signed transaction
    Relay {
        /// Signed transaction (hex or base64)
        transaction: String,
    },

    // ==================== Network ====================
    /// Broadcast commands
    #[command(subcommand)]
    Broadcast(BroadcastCommands),

    // ==================== Import/Export ====================
    /// Export blocks to a file
    ExportBlocks {
        /// Output file path
        #[arg(default_value = "chain.acc")]
        path: String,

        /// Start block index
        #[arg(long, default_value = "0")]
        start: u32,

        /// Number of blocks to export (0 = all)
        #[arg(long, default_value = "0")]
        count: u32,
    },
}

#[derive(Subcommand, Debug)]
pub enum WalletCommands {
    /// Open a wallet file
    Open {
        /// Path to wallet file
        path: String,

        /// Wallet password
        #[arg(long, short = 'p')]
        password: Option<String>,
    },

    /// Create a new wallet
    Create {
        /// Path for new wallet file
        path: String,
    },

    /// List addresses in wallet
    List,

    /// List assets in wallet
    Assets,

    /// List keys in wallet
    Keys,

    /// Create new address in wallet
    CreateAddress {
        /// Number of addresses to create
        #[arg(default_value = "1")]
        count: u32,
    },

    /// Delete an address from wallet
    DeleteAddress {
        /// Address to delete
        address: String,
    },

    /// Import private key
    ImportKey {
        /// WIF private key or path to key file
        key: String,
    },

    /// Import watch-only address
    ImportWatchOnly {
        /// Address or public key
        address: String,
    },

    /// Export private key
    ExportKey {
        /// Address to export (optional, exports all if not specified)
        address: Option<String>,

        /// Output file path
        #[arg(long)]
        path: Option<String>,
    },

    /// Change wallet password
    ChangePassword,

    /// Upgrade wallet format
    Upgrade {
        /// Path to wallet file
        path: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum BroadcastCommands {
    /// Broadcast address message
    Addr,

    /// Broadcast block
    Block {
        /// Block hash or index
        block: String,
    },

    /// Broadcast getblocks request
    GetBlocks {
        /// Start hash
        start: String,
    },

    /// Broadcast getdata request
    GetData {
        /// Inventory type (tx, block, consensus)
        inv_type: String,

        /// Hash
        hash: String,
    },

    /// Broadcast getheaders request
    GetHeaders {
        /// Start hash
        start: String,
    },

    /// Broadcast inventory
    Inv {
        /// Inventory type
        inv_type: String,

        /// Hash
        hash: String,
    },

    /// Broadcast transaction
    Transaction {
        /// Transaction hash
        hash: String,
    },

    /// Broadcast ping
    Ping,
}

#[derive(Subcommand, Debug)]
pub enum NeoTokenCmd {
    /// Get total supply
    TotalSupply,

    /// Get decimals
    Decimals,

    /// Get symbol
    Symbol,

    /// Get balance of address
    BalanceOf {
        /// Address
        address: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum GasTokenCmd {
    /// Get total supply
    TotalSupply,

    /// Get decimals
    Decimals,

    /// Get symbol
    Symbol,

    /// Get balance of address
    BalanceOf {
        /// Address
        address: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create RPC client
    let url =
        Url::parse(&cli.rpc_url).with_context(|| format!("Invalid RPC URL: {}", cli.rpc_url))?;

    let client = RpcClient::new(url, cli.rpc_user.clone(), cli.rpc_pass.clone(), None)
        .map_err(|e| anyhow::anyhow!("Failed to create RPC client: {}", e))?;

    // Execute command
    let result = match cli.command {
        // Node Information
        Commands::Version => version::execute(&client).await,
        Commands::State => state::execute(&client).await,
        Commands::Peers => peers::execute(&client).await,
        Commands::Mempool { verbose } => mempool::execute(&client, verbose).await,
        Commands::Plugins => plugins::execute(&client).await,

        // Blockchain Queries
        Commands::Block { index_or_hash, raw } => {
            block::execute(&client, &index_or_hash, raw).await
        }
        Commands::Header { index_or_hash, raw } => {
            header::execute(&client, &index_or_hash, raw).await
        }
        Commands::Tx { hash, raw } => tx::execute(&client, &hash, raw).await,
        Commands::Contract { hash } => contract::execute(&client, &hash).await,
        Commands::BestBlockHash => best_block_hash::execute(&client).await,
        Commands::BlockCount => block_count::execute(&client).await,
        Commands::BlockHash { index } => block_hash::execute(&client, index).await,

        // Token Operations
        Commands::Balance { address } => balance::execute(&client, &address).await,
        Commands::Transfers { address, from, to } => {
            transfers::execute(&client, &address, from, to).await
        }
        Commands::Gas { address } => gas::execute(&client, &address).await,

        // Contract Invocation
        Commands::Invoke {
            hash,
            method,
            params,
        } => invoke::execute(&client, &hash, &method, &params).await,
        Commands::TestInvoke { script } => test_invoke::execute(&client, &script).await,

        // Wallet
        Commands::Wallet(cmd) => wallet::execute(&client, cmd).await,

        // Send/Transfer
        Commands::Send {
            asset,
            to,
            amount,
            from,
        } => send::execute(&client, &asset, &to, &amount, from.as_deref()).await,
        Commands::Transfer {
            token,
            to,
            amount,
            from,
            data,
        } => {
            transfer::execute(
                &client,
                &token,
                &to,
                &amount,
                from.as_deref(),
                data.as_deref(),
            )
            .await
        }

        // Voting
        Commands::Vote { address, pubkey } => vote::execute(&client, &address, &pubkey).await,
        Commands::Unvote { address } => vote::unvote(&client, &address).await,
        Commands::RegisterCandidate { pubkey } => vote::register_candidate(&client, &pubkey).await,
        Commands::UnregisterCandidate { pubkey } => {
            vote::unregister_candidate(&client, &pubkey).await
        }
        Commands::Candidates => vote::get_candidates(&client).await,
        Commands::Committee => vote::get_committee(&client).await,
        Commands::Validators => vote::get_validators(&client).await,

        // Native Contracts
        Commands::NativeContracts => native::list_contracts(&client).await,
        Commands::Neo { cmd } => native::neo_token(&client, cmd).await,
        Commands::GasToken { cmd } => native::gas_token(&client, cmd).await,

        // Tools
        Commands::Parse { value } => tools::parse(&value).await,
        Commands::ParseScript { script } => tools::parse_script(&client, &script).await,
        Commands::ValidateAddress { address } => validate::execute(&client, &address).await,
        Commands::Sign { data, key } => tools::sign(&data, &key).await,
        Commands::Relay { transaction } => relay::execute(&client, &transaction).await,

        // Network/Broadcast
        Commands::Broadcast(cmd) => broadcast::execute(&client, cmd).await,

        // Import/Export
        Commands::ExportBlocks { path, start, count } => {
            export::execute(&client, &path, start, count).await
        }
    };

    // Handle result
    match result {
        Ok(output) => {
            println!("{}", output);
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
