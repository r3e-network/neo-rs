#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Neo Node - Neo N3 node daemon (server).
//!
//! `neo-node` is the binary entry point for a Neo N3 node. It
//! supports two build modes:
//!
//! - **Default** (`cargo build -p neo-node`): a real `tokio`-based node
//!   that parses CLI arguments, loads a TOML configuration file, builds a
//!   [`neo_system::Node`] via [`neo_system::NodeBuilder`], spawns the
//!   blockchain/network/RPC service tasks, and waits for `Ctrl-C` before
//!   shutting down gracefully.
//!
//! - **Minimal** (`cargo build -p neo-node --no-default-features`): a tiny
//!   stub binary for dependency-only checks where the full node dependency
//!   graph is intentionally disabled.
//!
//! ## Back-compat
//!
//! The historical `wip` and `full` features are preserved as compatibility
//! names for the default daemon feature set.

#[cfg(feature = "wip")]
mod consensus;
#[cfg(feature = "wip")]
mod node;

#[cfg(not(feature = "wip"))]
fn main() {
    eprintln!(
        "neo-node: the runnable daemon is disabled because default features are off.\n\
         \n\
         Build with `cargo build -p neo-node` to get the full node daemon."
    );
    std::process::exit(1);
}

#[cfg(feature = "wip")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    crate::node::run().await
}
