use anyhow::Result;

pub type CommandResult = Result<()>;

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
