//! Neo Node - Neo N3 node daemon (server).
//!
//! `neo-node` is the binary entry point for a Neo N3 node. It
//! supports two build modes:
//!
//! - **Default** (`cargo build -p neo-node`): a stub that prints a
//!   clear message and exits. This keeps the workspace build green
//!   while the in-progress full implementation is being migrated to
//!   the new reth-style service architecture.
//!
//! - **WIP** (`cargo build -p neo-node --features wip`): a real
//!   `tokio`-based node that parses CLI arguments, loads a TOML
//!   configuration file, builds a [`neo_system::Node`] via
//!   [`neo_system::NodeBuilder`], spawns the blockchain and network
//!   service tasks, and waits for `Ctrl-C` before shutting down
//!   gracefully.
//!
//! ## Back-compat
//!
//! The historical `full` feature is preserved as an alias for `wip`.

#![warn(missing_docs)]

#[cfg(feature = "wip")]
mod consensus;
#[cfg(feature = "wip")]
mod node;

#[cfg(not(feature = "wip"))]
fn main() {
    eprintln!(
        "neo-node: the full node daemon is gated behind the `wip` Cargo feature.\n\
         \n\
         Build with `cargo build -p neo-node --features wip` (or `--features full`)\n\
         to get the full node daemon."
    );
    std::process::exit(1);
}

#[cfg(feature = "wip")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    crate::node::run().await
}
