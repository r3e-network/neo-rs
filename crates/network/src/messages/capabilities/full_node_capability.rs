//! Full node capability implementation (mirrors `FullNodeCapability.cs`).

use super::node_capability::NodeCapability;

/// Creates a capability describing the node's current chain height.
pub fn full_node(start_height: u32) -> NodeCapability {
    NodeCapability::full_node(start_height)
}
