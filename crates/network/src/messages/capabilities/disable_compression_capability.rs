//! Disable-compression capability (mirrors `DisableCompressionCapability.cs`).

use super::node_capability::NodeCapability;

/// Creates a capability instructing peers not to use compression.
pub fn disable_compression() -> NodeCapability {
    NodeCapability::disable_compression()
}
