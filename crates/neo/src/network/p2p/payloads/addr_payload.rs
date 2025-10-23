// Copyright (C) 2015-2025 The Neo Project.
//
// addr_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::network_address_with_time::NetworkAddressWithTime;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates the maximum number of nodes sent each time.
pub const MAX_COUNT_TO_SEND: usize = 200;

/// This message is sent to respond to GetAddr messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddrPayload {
    /// The list of nodes.
    pub address_list: Vec<NetworkAddressWithTime>,
}

impl AddrPayload {
    /// Creates a new instance of the AddrPayload class.
    pub fn create(addresses: Vec<NetworkAddressWithTime>) -> Self {
        Self {
            address_list: addresses,
        }
    }
}

impl Serializable for AddrPayload {
    fn size(&self) -> usize {
        get_var_size(self.address_list.len() as u64)
            + self.address_list.iter().map(|a| a.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.address_list.len() > MAX_COUNT_TO_SEND {
            return Err(IoError::invalid_data("Too many addresses"));
        }

        writer.write_var_uint(self.address_list.len() as u64)?;
        for address in &self.address_list {
            writer.write_serializable(address)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let count = reader.read_var_int(MAX_COUNT_TO_SEND as u64)? as usize;
        if count == 0 {
            return Err(IoError::invalid_data("Empty address list"));
        }

        let mut address_list = Vec::with_capacity(count);
        for _ in 0..count {
            address_list.push(<NetworkAddressWithTime as Serializable>::deserialize(
                reader,
            )?);
        }

        Ok(Self { address_list })
    }
}
