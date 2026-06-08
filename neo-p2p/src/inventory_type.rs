// InventoryType now lives in neo-primitives; this module re-exports it and keeps
// the MessageCommand conversion that is specific to neo-p2p networking.

pub use neo_primitives::InventoryType;

use crate::message_command::MessageCommand;

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
    fn converts_to_message_command() {
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
}
