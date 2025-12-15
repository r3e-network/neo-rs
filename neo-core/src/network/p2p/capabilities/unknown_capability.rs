//! Unknown capability support (mirrors `UnknownCapability.cs`).

use super::{node_capability::NodeCapability, node_capability_type::NodeCapabilityType};
use crate::neo_io::IoResult;

/// Builds an opaque capability descriptor, preserving the raw payload bytes.
pub fn unknown(ty: NodeCapabilityType, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown(ty, data)
}

/// Convenience helper for constructing unknown capabilities from a raw byte identifier.
pub fn unknown_from_byte(raw_type: u8, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown_from_byte(raw_type, data)
}
