//! Neo network capability descriptors.
//!
//! This module mirrors the C# `Neo.Network.P2P.Capabilities` namespace by
//! splitting each capability into its own Rust module while still providing the
//! same ergonomic constructors that the rest of the Rust port expects.

pub mod archival_node_capability;
pub mod disable_compression_capability;
pub mod full_node_capability;
pub mod node_capability;
pub mod node_capability_type;
pub mod server_capability;
pub mod unknown_capability;

pub use archival_node_capability::archival_node;
pub use disable_compression_capability::disable_compression;
pub use full_node_capability::full_node;
pub use node_capability::{NodeCapability, MAX_UNKNOWN_CAPABILITY_DATA};
pub use node_capability_type::NodeCapabilityType;
pub use server_capability::{tcp_server, ws_server};
pub use unknown_capability::{unknown, unknown_from_byte};