//! Archival node capability implementation (mirrors `ArchivalNodeCapability.cs`).

use super::node_capability::NodeCapability;

/// Creates a capability indicating that the node keeps full historical data.
pub fn archival_node() -> NodeCapability {
    NodeCapability::archival_node()
}
