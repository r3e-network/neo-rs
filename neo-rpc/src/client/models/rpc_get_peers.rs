use super::rpc_peers::RpcPeers;

/// Alias for the `getpeers` RPC response model.
///
/// The C# implementation exposes a dedicated type that simply wraps the
/// `RpcPeers` structure. We mirror that behaviour by providing a type alias
/// so downstream code can depend on the `RpcGetPeers` symbol while sharing the
/// same serialization logic as `RpcPeers`.
pub type RpcGetPeers = RpcPeers;

/// Re-export peer entry type for callers expecting it from this module.
pub use super::rpc_peers::RpcPeer as RpcGetPeersPeer;
