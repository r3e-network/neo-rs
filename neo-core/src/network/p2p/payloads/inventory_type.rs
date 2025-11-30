// Copyright (C) 2015-2025 The Neo Project.
//
// inventory_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::network::p2p::message_command::MessageCommand;
use serde::{Deserialize, Serialize};

/// Represents the type of an inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum InventoryType {
    /// Indicates that the inventory is a Transaction.
    Transaction = 0x2b,

    /// Indicates that the inventory is a Block.
    Block = 0x2c,

    /// Indicates that the inventory is a consensus payload.
    Consensus = 0x2d,

    /// Indicates that the inventory is an ExtensiblePayload.
    Extensible = 0x2e,
}

impl InventoryType {
    /// Convert from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x2b => Some(Self::Transaction),
            0x2c => Some(Self::Block),
            0x2d => Some(Self::Consensus),
            0x2e => Some(Self::Extensible),
            _ => None,
        }
    }

    /// Convert to byte value.
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

impl From<InventoryType> for MessageCommand {
    fn from(inv_type: InventoryType) -> Self {
        match inv_type {
            InventoryType::Transaction => MessageCommand::Transaction,
            InventoryType::Block => MessageCommand::Block,
            InventoryType::Consensus => MessageCommand::Extensible, // Placeholder; consensus maps to Extensible in N3
            InventoryType::Extensible => MessageCommand::Extensible,
        }
    }
}
