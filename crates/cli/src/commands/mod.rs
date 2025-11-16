use anyhow::{anyhow, Result};

pub type CommandResult = Result<()>;

fn not_implemented(command: &str) -> CommandResult {
    Err(anyhow!(
        "{}: command not implemented yet. See PORTING_PLAN.md for the migration roadmap.",
        command
    ))
}

pub mod block;
pub mod blockchain;
pub mod command_line;
pub mod contracts;
pub mod logger;
pub mod native;
pub mod nep17;
pub mod network;
pub mod node;
pub mod plugins;
pub mod tools;
pub mod vote;
pub mod wallet;
