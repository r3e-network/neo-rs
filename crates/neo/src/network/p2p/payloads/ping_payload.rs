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

use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

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
                .unwrap()
                .as_secs() as u32,
            nonce,
        }
    }
}

impl Serializable for PingPayload {
    fn size(&self) -> usize {
        4 + // LastBlockIndex
        4 + // Timestamp
        4 // Nonce
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.last_block_index.to_le_bytes())?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.nonce.to_le_bytes())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let last_block_index = reader.read_u32().map_err(|e| e.to_string())?;
        let timestamp = reader.read_u32().map_err(|e| e.to_string())?;
        let nonce = reader.read_u32().map_err(|e| e.to_string())?;

        Ok(Self {
            last_block_index,
            timestamp,
            nonce,
        })
    }
}
