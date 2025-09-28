// Copyright (C) 2015-2025 The Neo Project.
//
// filter_load_payload.rs file belongs to the neo project and is free
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

/// Maximum filter size (36000 bytes)
const MAX_FILTER_SIZE: usize = 36000;

/// Maximum number of hash functions (50)
const MAX_K: u8 = 50;

/// This message is sent to load the BloomFilter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterLoadPayload {
    /// The data of the BloomFilter.
    pub filter: Vec<u8>,

    /// The number of hash functions used by the BloomFilter.
    pub k: u8,

    /// Used to generate the seeds of the murmur hash functions.
    pub tweak: u32,
}

impl FilterLoadPayload {
    /// Creates a new filter load payload.
    pub fn new(filter: Vec<u8>, k: u8, tweak: u32) -> Self {
        Self { filter, k, tweak }
    }

    /// Creates from a bloom filter.
    pub fn create_from_bloom_filter(m: usize, k: u8, tweak: u32, bits: Vec<u8>) -> Self {
        // The bits would be extracted from a BloomFilter
        // For now, use the provided bits directly
        Self {
            filter: bits,
            k,
            tweak,
        }
    }
}

impl Serializable for FilterLoadPayload {
    fn size(&self) -> usize {
        2 + self.filter.len() + // Filter with var length prefix
        1 + // K
        4 // Tweak
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write filter as var bytes
        if self.filter.len() > MAX_FILTER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Filter too large",
            ));
        }
        writer.write_all(&(self.filter.len() as u16).to_le_bytes())?;
        writer.write_all(&self.filter)?;

        writer.write_all(&[self.k])?;
        writer.write_all(&self.tweak.to_le_bytes())?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let filter_len = reader.read_var_int().map_err(|e| e.to_string())?;
        if filter_len > MAX_FILTER_SIZE as u64 {
            return Err("Filter too large".to_string());
        }

        let filter = reader
            .read_bytes(filter_len as usize)
            .map_err(|e| e.to_string())?;

        let k = reader.read_u8().map_err(|e| e.to_string())?;
        if k > MAX_K {
            return Err(format!("K value {} exceeds maximum {}", k, MAX_K));
        }

        let tweak = reader.read_u32().map_err(|e| e.to_string())?;

        Ok(Self { filter, k, tweak })
    }
}
