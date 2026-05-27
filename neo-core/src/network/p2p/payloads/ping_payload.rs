// Copyright (C) 2015-2025 The Neo Project.
//
// ping_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_io::impl_serializable;
use serde::{Deserialize, Serialize};

/// Sent to detect whether the connection has been disconnected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingPayload {
    /// The latest block index.
    pub last_block_index: u32,

    /// The timestamp when the message was sent.
    pub timestamp: u32,

    /// A random number. This number must be the same in
    /// Ping and Pong messages.
    pub nonce: u32,
}

impl PingPayload {
    /// Creates a new instance of the PingPayload class.
    pub fn create(height: u32) -> Self {
        let nonce = rand::random::<u32>();
        Self::create_with_nonce(height, nonce)
    }

    /// Creates a new instance of the PingPayload class with a specific nonce.
    pub fn create_with_nonce(height: u32, nonce: u32) -> Self {
        Self {
            last_block_index: height,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            nonce,
        }
    }
}

impl_serializable! {
    struct PingPayload {
        last_block_index: u32,
        timestamp: u32,
        nonce: u32,
    }
}
