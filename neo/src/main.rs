// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::fs::File;
use std::io::Read;

use clap::{Parser, Subcommand};

use crate::node::{Config, NodeCmd, run_node};

mod node;


#[derive(Parser)]
#[command(author = "R3E Network Team")]
#[command(version = neo_base::VERSION)]
#[command(about = "A rust implementation for NEO")]
struct Cli {
    #[arg(long, help = "The log config file path")]
    log: String,

    #[command(subcommand)]
    commands: Commands,
}


#[derive(Subcommand)]
enum Commands {
    Node(NodeCmd),
}


fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    #[cfg(not(test))]
    log4rs::init_file(&cli.log, Default::default())?;

    match &cli.commands {
        Commands::Node(cmd) => {
            let mut file = File::open(&cmd.config)?;
            let mut content = String::new();
            let _ = file.read_to_string(&mut content)?;

            let config: Config = serde_yaml::from_str(&content)?;
            run_node(config)
        }
    }
}