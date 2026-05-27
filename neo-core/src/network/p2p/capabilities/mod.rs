//! Neo network capability descriptors.
//!
//! `NodeCapability` is the single wire container. Free constructors remain for
//! callers that prefer the old module-level helper style.

/// Base node capability trait.
pub mod node_capability;
/// Node capability type enumeration.
pub mod node_capability_type {
    pub use neo_primitives::NodeCapabilityType;
}

use crate::neo_io::IoResult;

pub use node_capability::{MAX_UNKNOWN_CAPABILITY_DATA, NodeCapability};
pub(crate) use node_capability::{
    deserialize_node_capabilities, node_capabilities_size, serialize_node_capabilities,
};
pub use node_capability_type::NodeCapabilityType;

/// Creates an archival node capability descriptor.
pub fn archival_node() -> NodeCapability {
    NodeCapability::archival_node()
}

/// Creates a disable-compression capability descriptor.
pub fn disable_compression() -> NodeCapability {
    NodeCapability::disable_compression()
}

/// Creates a full node capability descriptor.
pub fn full_node(start_height: u32) -> NodeCapability {
    NodeCapability::full_node(start_height)
}

/// Creates a TCP server capability descriptor.
pub fn tcp_server(port: u16) -> NodeCapability {
    NodeCapability::tcp_server(port)
}

/// Creates a WebSocket server capability descriptor.
pub fn ws_server(port: u16) -> NodeCapability {
    NodeCapability::ws_server(port)
}

/// Builds an opaque capability descriptor, preserving the raw payload bytes.
pub fn unknown(ty: NodeCapabilityType, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown(ty, data)
}

/// Convenience helper for constructing unknown capabilities from a raw byte identifier.
pub fn unknown_from_byte(raw_type: u8, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown_from_byte(raw_type, data)
}
