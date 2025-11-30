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

use crate::macros::ValidateLength;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

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
        get_var_size(self.data.len() as u64) + self.data.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        // Use ValidateLength trait to reduce boilerplate
        self.data.validate_max_length(MAX_DATA_SIZE, "Data")?;
        writer.write_var_bytes(&self.data)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let data = reader.read_var_bytes(MAX_DATA_SIZE)?;

        Ok(Self { data })
    }
}
