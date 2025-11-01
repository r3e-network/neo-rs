// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_get_peers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::rpc_peers::{RpcPeer, RpcPeers};

/// Alias for the `getpeers` RPC response model.
///
/// The C# implementation exposes a dedicated type that simply wraps the
/// `RpcPeers` structure. We mirror that behaviour by providing a type alias
/// so downstream code can depend on the `RpcGetPeers` symbol while sharing the
/// same serialization logic as `RpcPeers`.
pub type RpcGetPeers = RpcPeers;

/// Re-export peer entry type for callers expecting it from this module.
pub use RpcPeer as RpcGetPeersPeer;
