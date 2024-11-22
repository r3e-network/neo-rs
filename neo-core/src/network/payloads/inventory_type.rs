use crate::network::connection::message_command::MessageCommand;

/// Represents the type of inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InventoryType {
    /// Indicates that the inventory is a Transaction.
    TX = MessageCommand::Transaction as u8,

    /// Indicates that the inventory is a Block.
    Block = MessageCommand::Block as u8,

    /// Indicates that the inventory is an ExtensiblePayload.
    Extensible = MessageCommand::Extensible as u8,
}
