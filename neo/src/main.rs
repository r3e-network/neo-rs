// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::fs::File;
use std::io::Read;

use clap::{Parser, Subcommand};

use crate::node::{Config, NodeCmd, run_node};
use crate::tools::{Nef3Cmd, parse_nef3_file};

mod node;
mod tools;


#[derive(Parser)]
#[command(author = "R3E Network Team")]
#[command(version = neo_base::VERSION)]
#[command(about = "A rust implementation for NEO")]
struct Cli {
    #[arg(long, help = "The log config file path")]
    log: Option<String>,

    #[command(subcommand)]
    commands: Commands,
}


#[derive(Subcommand)]
enum Commands {
    Node(NodeCmd),

    Nef3(Nef3Cmd),
}


fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    #[cfg(not(test))]
    if let Some(log) = &cli.log {
        log4rs::init_file(log, Default::default())?;
    }

    match &cli.commands {
        Commands::Node(cmd) => {
            let config = read_file(&cmd.config)?;
            let config: Config = serde_yaml::from_str(&config)?;
            run_node(config)
        }
        Commands::Nef3(cmd) => { parse_nef3_file(&cmd.file) }
    }
}


pub(crate) fn read_file(file: &str) -> anyhow::Result<String> {
    let mut file = File::open(file)?;
    let mut content = String::new();

    let _ = file.read_to_string(&mut content)?;
    Ok(content)
}