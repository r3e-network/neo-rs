//! Server capability helpers (mirrors `ServerCapability.cs`).

use super::node_capability::NodeCapability;

/// Builds a TCP server capability descriptor.
pub fn tcp_server(port: u16) -> NodeCapability {
    NodeCapability::tcp_server(port)
}

/// Builds a WebSocket server capability descriptor.
#[allow(dead_code)]
pub fn ws_server(port: u16) -> NodeCapability {
    NodeCapability::ws_server(port)
}
