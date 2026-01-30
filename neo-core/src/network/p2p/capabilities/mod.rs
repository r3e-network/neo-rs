//! Neo network capability descriptors.
//!
//! This module mirrors the C# `Neo.Network.P2P.Capabilities` namespace by
//! splitting each capability into its own Rust module while still providing the
//! same ergonomic constructors that the rest of the Rust port expects.

/// Archival node capability.
pub mod archival_node_capability;
/// Disable compression capability.
pub mod disable_compression_capability;
/// Full node capability.
pub mod full_node_capability;
/// Base node capability trait.
pub mod node_capability;
/// Node capability type enumeration.
pub mod node_capability_type;
/// Server capability (TCP/WS).
pub mod server_capability;
/// Unknown capability handler.
pub mod unknown_capability;

pub use archival_node_capability::archival_node;
pub use disable_compression_capability::{disable_compression, DisableCompressionCapability};
pub use full_node_capability::full_node;
pub use node_capability::{NodeCapability, MAX_UNKNOWN_CAPABILITY_DATA};
pub use node_capability_type::NodeCapabilityType;
pub use server_capability::{tcp_server, ws_server, ServerCapability};
pub use unknown_capability::{unknown, unknown_from_byte};
