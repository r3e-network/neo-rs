// InventoryType now lives in neo-primitives; this module re-exports it and keeps
// the MessageCommand conversion that is specific to the P2P networking layer.

pub use neo_primitives::InventoryType;

use super::message_command::MessageCommand;

impl From<InventoryType> for MessageCommand {
    fn from(inv_type: InventoryType) -> Self {
        match inv_type {
            InventoryType::Transaction => MessageCommand::Transaction,
            InventoryType::Block => MessageCommand::Block,
            InventoryType::Extensible => MessageCommand::Extensible,
        }
    }
}

#[cfg(test)]
#[path = "../tests/proto/inventory_type.rs"]
mod tests;
