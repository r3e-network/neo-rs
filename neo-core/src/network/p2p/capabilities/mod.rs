//! Neo network capability descriptors.
//!
//! `NodeCapability` and the constructor helpers are defined in
//! `neo_p2p::payloads::node_capability` (task 199 consolidation) and
//! re-exported here for backward compatibility.

/// Node capability type enumeration.
pub mod node_capability_type {
    pub use neo_primitives::NodeCapabilityType;
}

pub use neo_p2p::payloads::node_capability::MAX_UNKNOWN_CAPABILITY_DATA;
pub use neo_p2p::payloads::node_capability::NodeCapability;
pub use neo_p2p::payloads::node_capability::{
    archival_node, disable_compression, full_node, tcp_server, unknown, unknown_from_byte,
    ws_server,
};
pub use node_capability_type::NodeCapabilityType;
