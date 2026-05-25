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
use crate::neo_io::serializable::helper::{
    deserialize_array, get_var_size_serializable_slice, serialize_array,
};
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
        get_var_size_serializable_slice(&self.address_list)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.address_list.len() > MAX_COUNT_TO_SEND {
            return Err(IoError::invalid_data("Too many addresses"));
        }

        serialize_array(&self.address_list, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let address_list = deserialize_array(reader, MAX_COUNT_TO_SEND)?;
        if address_list.is_empty() {
            return Err(IoError::invalid_data("Empty address list"));
        }

        Ok(Self { address_list })
    }
}
