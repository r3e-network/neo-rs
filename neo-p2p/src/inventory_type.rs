//! Inventory type identifiers (mirrors `Neo.Network.P2P.Payloads.InventoryType`).

use crate::MessageCommand;
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

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transaction => "TX",
            Self::Block => "Block",
            Self::Consensus => "Consensus",
            Self::Extensible => "Extensible",
        }
    }
}

impl std::fmt::Display for InventoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<InventoryType> for MessageCommand {
    fn from(inv_type: InventoryType) -> Self {
        match inv_type {
            InventoryType::Transaction => MessageCommand::Transaction,
            InventoryType::Block => MessageCommand::Block,
            InventoryType::Consensus => MessageCommand::Extensible,
            InventoryType::Extensible => MessageCommand::Extensible,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_type_values() {
        assert_eq!(InventoryType::Transaction as u8, 0x2b);
        assert_eq!(InventoryType::Block as u8, 0x2c);
        assert_eq!(InventoryType::Consensus as u8, 0x2d);
        assert_eq!(InventoryType::Extensible as u8, 0x2e);
    }

    #[test]
    fn test_inventory_type_from_byte() {
        assert_eq!(InventoryType::from_byte(0x2b), Some(InventoryType::Transaction));
        assert_eq!(InventoryType::from_byte(0x2c), Some(InventoryType::Block));
        assert_eq!(InventoryType::from_byte(0x2d), Some(InventoryType::Consensus));
        assert_eq!(InventoryType::from_byte(0x2e), Some(InventoryType::Extensible));
        assert_eq!(InventoryType::from_byte(0x00), None);
    }

    #[test]
    fn test_inventory_type_to_message_command() {
        assert_eq!(
            MessageCommand::from(InventoryType::Transaction),
            MessageCommand::Transaction
        );
        assert_eq!(
            MessageCommand::from(InventoryType::Block),
            MessageCommand::Block
        );
        assert_eq!(
            MessageCommand::from(InventoryType::Extensible),
            MessageCommand::Extensible
        );
    }

    #[test]
    fn test_inventory_type_display() {
        assert_eq!(InventoryType::Transaction.to_string(), "TX");
        assert_eq!(InventoryType::Block.to_string(), "Block");
    }
}
