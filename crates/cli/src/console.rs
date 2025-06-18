//! Console Interface for Neo CLI
//!
//! This module provides an interactive console interface for the Neo CLI.

use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::wallet::WalletManager;

/// Console service for interactive CLI commands
pub struct ConsoleService {
    wallet_manager: Arc<RwLock<WalletManager>>,
    is_running: bool,
}

type ConsoleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

impl ConsoleService {
    /// Create a new console service
    pub fn new() -> Self {
        Self {
            wallet_manager: Arc::new(RwLock::new(WalletManager::new())),
            is_running: false,
        }
    }

    /// Start the console service
    pub async fn start(&mut self) -> ConsoleResult<()> {
        info!("🎮 Starting interactive console");
        self.is_running = true;

        self.print_banner();
        self.print_help();

        // Main console loop
        loop {
            print!("neo> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            
            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            if input == "exit" || input == "quit" {
                break;
            }

            match self.process_command(input).await {
                Ok(_) => {}
                Err(e) => println!("Error: {}", e),
            }
        }

        info!("👋 Console session ended");
        Ok(())
    }

    /// Process a command
    async fn process_command(&self, input: &str) -> ConsoleResult<()> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        match parts[0] {
            "help" => self.print_help(),
            "version" => {
                println!("neo-cli {}", env!("CARGO_PKG_VERSION"));
                println!("Neo N3 compatibility: 3.6.0");
                println!("Neo VM compatibility: 3.6.0");
            },
            "clear" => {
                print!("\x1B[2J\x1B[1;1H");
                io::stdout().flush().unwrap();
            },
            "wallet" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "list" => self.list_wallets().await?,
                        "create" => println!("Wallet creation not implemented yet"),
                        "open" => println!("Wallet opening not implemented yet"),
                        _ => println!("Unknown wallet command. Type 'help' for usage."),
                    }
                } else {
                    println!("Usage: wallet [list|create|open]");
                }
            },
            "show" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "state" => self.show_state().await?,
                        "version" => {
                            println!("neo-cli {}", env!("CARGO_PKG_VERSION"));
                        },
                        _ => println!("Unknown show command. Type 'help' for usage."),
                    }
                } else {
                    println!("Usage: show [state|version]");
                }
            },
            _ => println!("Unknown command '{}'. Type 'help' for available commands.", parts[0]),
        }

        Ok(())
    }

    /// Print banner
    fn print_banner(&self) {
        println!("╔══════════════════════════════════════════════════════════════════╗");
        println!("║                       🎮 Neo-Rust Console 🎮                      ║");
        println!("║                                                                  ║");
        println!("║                Interactive Neo Blockchain Interface              ║");
        println!("║                                                                  ║");
        println!("╚══════════════════════════════════════════════════════════════════╝");
        println!();
    }

    /// Print help
    fn print_help(&self) {
        println!("Available commands:");
        println!("  help           - Show this help message");
        println!("  version        - Show version information");
        println!("  clear          - Clear the screen");
        println!("  wallet list    - List wallet accounts");
        println!("  wallet create  - Create a new wallet");
        println!("  wallet open    - Open an existing wallet");
        println!("  show state     - Show blockchain state");
        println!("  show version   - Show version information");
        println!("  exit           - Exit the console");
        println!();
    }

    /// List wallets
    async fn list_wallets(&self) -> ConsoleResult<()> {
        let wallet_manager = self.wallet_manager.read().await;
        if wallet_manager.has_open_wallet() {
            println!("Current wallet accounts:");
            let accounts = wallet_manager.accounts();
            for (i, (script_hash, _account)) in accounts.iter().enumerate() {
                println!("  {}. {}", i + 1, script_hash.to_address());
            }
        } else {
            println!("No wallet currently open.");
        }
        Ok(())
    }

    /// Show blockchain state
    async fn show_state(&self) -> ConsoleResult<()> {
        println!("Blockchain State:");
        println!("  Height: 0 (not connected)");
        println!("  Peers: 0");
        println!("  Mempool: 0 transactions");
        Ok(())
    }

    /// Stop the console service
    pub fn stop(&mut self) {
        self.is_running = false;
    }

    /// Check if the console is running
    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

impl Default for ConsoleService {
    fn default() -> Self {
        Self::new()
    }
} 