//! Node capability descriptors.
//!
//! The canonical `NodeCapability` type lives in [`neo_p2p::payloads`];
//! this module re-exports it alongside helper constructors for common
//! capability shapes (full node, archival, TCP/WS server).

use neo_io::IoResult;
pub use neo_p2p::NodeCapabilityType;
pub use neo_p2p::payloads::node_capability::{
    MAX_UNKNOWN_CAPABILITY_DATA, NodeCapability, deserialize_node_capabilities,
    node_capabilities_size, serialize_node_capabilities,
};

/// Constructs an archival-node capability descriptor.
pub fn archival_node() -> NodeCapability {
    NodeCapability::archival_node()
}

/// Constructs a disable-compression capability descriptor.
pub fn disable_compression() -> NodeCapability {
    NodeCapability::disable_compression()
}

/// Constructs a full-node capability descriptor with the given start height.
pub fn full_node(start_height: u32) -> NodeCapability {
    NodeCapability::full_node(start_height)
}

/// Constructs a TCP-server capability descriptor with the given port.
pub fn tcp_server(port: u16) -> NodeCapability {
    NodeCapability::tcp_server(port)
}

/// Constructs a WebSocket-server capability descriptor with the given port.
pub fn ws_server(port: u16) -> NodeCapability {
    NodeCapability::ws_server(port)
}

/// Builds an opaque capability descriptor, preserving the raw payload bytes.
pub fn unknown(ty: NodeCapabilityType, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown(ty, data)
}

/// Convenience helper for constructing unknown capabilities from a
/// raw byte identifier.
pub fn unknown_from_byte(raw_type: u8, data: Vec<u8>) -> IoResult<NodeCapability> {
    NodeCapability::unknown_from_byte(raw_type, data)
}
