//! # neo-node
//!
//! Runnable Neo N3 node daemon and operator-facing modes.
//!
//! ## Boundary
//!
//! This application crate may compose lower layers but must not define protocol
//! bytes, storage formats, consensus rules, or VM semantics.
//!
//! ## Contents
//!
//! - `consensus`: Consensus-facing node adapters and startup helpers.
//! - `node`: Daemon composition, CLI modes, and long-running node startup.
//! - `state_root`: Active signed-StateRoot (StateValidators) consensus driver.

#[path = "consensus/mod.rs"]
mod consensus;
#[path = "node/mod.rs"]
mod node;
#[path = "state_root/mod.rs"]
mod state_root;

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
