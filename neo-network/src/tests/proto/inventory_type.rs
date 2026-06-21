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
