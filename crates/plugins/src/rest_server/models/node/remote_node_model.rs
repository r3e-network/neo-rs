// Copyright (C) 2015-2025 The Neo Project.
//
// remote_node_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::network::p2p::RemoteNodeSnapshot;
use serde::{Deserialize, Serialize};

/// Remote node model matching C# RemoteNodeModel exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteNodeModel {
    /// Remote peer's ip address
    /// Matches C# RemoteAddress property
    pub remote_address: String,

    /// Remote peer's port number
    /// Matches C# RemotePort property
    pub remote_port: i32,

    /// Remote peer's listening tcp port
    /// Matches C# ListenTcpPort property
    pub listen_tcp_port: i32,

    /// Remote peer's last synced block height
    /// Matches C# LastBlockIndex property
    pub last_block_index: u32,
}

impl RemoteNodeModel {
    /// Creates a new RemoteNodeModel
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            remote_address: String::new(),
            remote_port: 0,
            listen_tcp_port: 0,
            last_block_index: 0,
        }
    }

    /// Creates a new RemoteNodeModel with parameters
    /// Matches C# constructor with parameters
    pub fn with_params(
        remote_address: String,
        remote_port: i32,
        listen_tcp_port: i32,
        last_block_index: u32,
    ) -> Self {
        Self {
            remote_address,
            remote_port,
            listen_tcp_port,
            last_block_index,
        }
    }

    /// Builds a `RemoteNodeModel` from a runtime snapshot.
    pub fn from_snapshot(snapshot: &RemoteNodeSnapshot) -> Self {
        Self {
            remote_address: snapshot.remote_address.ip().to_string(),
            remote_port: snapshot.remote_port as i32,
            listen_tcp_port: snapshot.listen_tcp_port as i32,
            last_block_index: snapshot.last_block_index,
        }
    }
}

impl Default for RemoteNodeModel {
    fn default() -> Self {
        Self::new()
    }
}
