
//! Neo Node - Neo N3 node daemon (server).
//!
//! `neo-node` is the binary entry point for a Neo N3 node: a `tokio`-based
//! daemon that parses CLI arguments, loads a TOML configuration file, builds a
//! [`neo_system::Node`] via [`neo_system::NodeBuilder`], spawns the
//! blockchain/network/RPC service tasks, and waits for `Ctrl-C` before shutting
//! down gracefully.

mod consensus;
mod node;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    crate::node::run().await
}
