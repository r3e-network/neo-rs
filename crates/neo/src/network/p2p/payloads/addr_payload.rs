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
use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

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
        1 + self.address_list.iter().map(|a| a.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        if self.address_list.len() > MAX_COUNT_TO_SEND {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Too many addresses",
            ));
        }

        writer.write_all(&[self.address_list.len() as u8])?;
        for address in &self.address_list {
            address.serialize(writer)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let count = reader.read_var_int().map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Empty address list".to_string());
        }
        if count > MAX_COUNT_TO_SEND as u64 {
            return Err("Too many addresses".to_string());
        }

        let mut address_list = Vec::with_capacity(count as usize);
        for _ in 0..count {
            address_list.push(NetworkAddressWithTime::deserialize(reader)?);
        }

        Ok(Self { address_list })
    }
}
