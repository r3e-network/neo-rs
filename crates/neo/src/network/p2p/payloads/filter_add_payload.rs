// Copyright (C) 2015-2025 The Neo Project.
//
// filter_add_payload.rs file belongs to the neo project and is free
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

/// Maximum data size (520 bytes)
const MAX_DATA_SIZE: usize = 520;

/// This message is sent to update the items for the BloomFilter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterAddPayload {
    /// The items to be added.
    pub data: Vec<u8>,
}

impl FilterAddPayload {
    /// Creates a new filter add payload.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl Serializable for FilterAddPayload {
    fn size(&self) -> usize {
        2 + self.data.len() // Data with var length prefix
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write data as var bytes
        if self.data.len() > MAX_DATA_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Data too large",
            ));
        }
        writer.write_all(&(self.data.len() as u16).to_le_bytes())?;
        writer.write_all(&self.data)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let data_len = reader.read_var_int().map_err(|e| e.to_string())?;
        if data_len > MAX_DATA_SIZE as u64 {
            return Err("Data too large".to_string());
        }

        let data = reader
            .read_bytes(data_len as usize)
            .map_err(|e| e.to_string())?;

        Ok(Self { data })
    }
}
