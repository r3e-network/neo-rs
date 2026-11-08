use neo_p2p::{InventoryType, MessageCommand, MessageFlags};

#[test]
fn message_commands_preserve_known_and_unknown_wire_values() {
    for command in [
        MessageCommand::Block,
        MessageCommand::Transaction,
        MessageCommand::Extensible,
    ] {
        let byte = command.to_byte();
        assert_eq!(MessageCommand::from_byte(byte).expect("command"), command);
    }

    let unknown = MessageCommand::from_byte(0x99).expect("unknown command is represented");
    assert_eq!(unknown.to_byte(), 0x99);
    assert!(!unknown.is_known());
}

#[test]
fn message_flags_preserve_compression_and_unknown_bits() {
    let compressed = MessageFlags::from_byte(0x01);
    assert!(compressed.is_compressed());
    assert_eq!(compressed.to_byte(), 0x01);

    let extension = MessageFlags::from_byte(0x81);
    assert!(extension.is_compressed());
    assert_eq!(extension.to_byte(), 0x81);
}

#[test]
fn inventory_types_map_to_wire_commands() {
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
fn unknown_command_is_not_compressible_or_high_priority() {
    let unknown = MessageCommand::from_byte(0xfe).expect("unknown command");
    assert!(!unknown.allows_compression());
    assert!(!unknown.is_high_priority_queue());
    assert!(!unknown.is_single_queued());
}
