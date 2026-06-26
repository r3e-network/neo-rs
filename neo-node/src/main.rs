//! Neo Node - Neo N3 node daemon (server).
//!
//! `neo-node` is the binary entry point for a Neo N3 node: a `tokio`-based
//! daemon that parses CLI arguments, loads a TOML configuration file, builds a
//! [`neo_system::Node`] via [`neo_system::NodeBuilder`], spawns the
//! blockchain/network/RPC service tasks, and waits for `Ctrl-C` before shutting
//! down gracefully.

mod consensus;
mod node;

// Use mimalloc as the global allocator. The node is allocation-heavy on the
// block-execution hot path (per-opcode StackItem clones, per-block/per-tx
// DataCache change maps, prefix-scan result vectors); profiling showed the
// system allocator's tiny/small malloc+free among the top self-time frames.
// mimalloc was already a dependency but was never wired in.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    crate::node::run().await
}
