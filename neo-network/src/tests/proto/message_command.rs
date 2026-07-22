use super::*;

#[test]
fn known_commands_match_the_neo_n3_wire_table() {
    let commands = [
        (MessageCommand::Version, 0x00, "version"),
        (MessageCommand::Verack, 0x01, "verack"),
        (MessageCommand::GetAddr, 0x10, "getaddr"),
        (MessageCommand::Addr, 0x11, "addr"),
        (MessageCommand::Ping, 0x18, "ping"),
        (MessageCommand::Pong, 0x19, "pong"),
        (MessageCommand::GetHeaders, 0x20, "getheaders"),
        (MessageCommand::Headers, 0x21, "headers"),
        (MessageCommand::GetBlocks, 0x24, "getblocks"),
        (MessageCommand::Mempool, 0x25, "mempool"),
        (MessageCommand::Inv, 0x27, "inv"),
        (MessageCommand::GetData, 0x28, "getdata"),
        (MessageCommand::GetBlockByIndex, 0x29, "getblkbyidx"),
        (MessageCommand::NotFound, 0x2a, "notfound"),
        (MessageCommand::Transaction, 0x2b, "tx"),
        (MessageCommand::Block, 0x2c, "block"),
        (MessageCommand::Extensible, 0x2e, "extensible"),
        (MessageCommand::Reject, 0x2f, "reject"),
        (MessageCommand::FilterLoad, 0x30, "filterload"),
        (MessageCommand::FilterAdd, 0x31, "filteradd"),
        (MessageCommand::FilterClear, 0x32, "filterclear"),
        (MessageCommand::MerkleBlock, 0x38, "merkleblock"),
        (MessageCommand::Alert, 0x40, "alert"),
    ];

    for (command, byte, name) in commands {
        assert_eq!(command.to_byte(), byte);
        assert_eq!(MessageCommand::from_byte(byte), command);
        assert_eq!(command.as_str(), name);
        assert_eq!(command.to_string(), name);
        assert_eq!(name.parse::<MessageCommand>().unwrap(), command);
    }
}

#[test]
fn inventory_type_maps_to_its_network_command() {
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
fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
    let command = MessageCommand::from_byte(0x99);
    assert_eq!(command, MessageCommand::Unknown(0x99));
    assert_eq!(command.to_byte(), 0x99);
    assert_eq!(command.as_byte(), 0x99);
    assert_eq!(command.as_str(), "unknown");
    assert!(!command.is_known());

    let serialized = serde_json::to_string(&command).unwrap();
    assert_eq!(serialized, "153");
    let deserialized: MessageCommand = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, command);
}

#[test]
fn compression_whitelist_matches_protocol_commands() {
    let compressible = [
        MessageCommand::Block,
        MessageCommand::Extensible,
        MessageCommand::Transaction,
        MessageCommand::Headers,
        MessageCommand::Addr,
        MessageCommand::MerkleBlock,
        MessageCommand::FilterLoad,
        MessageCommand::FilterAdd,
    ];

    for command in compressible {
        assert!(command.allows_compression(), "{command:?}");
    }

    assert!(!MessageCommand::Ping.allows_compression());
    assert!(!MessageCommand::Unknown(0x99).allows_compression());
}

#[test]
fn extended_parse_aliases_preserve_legacy_unknown_commands() {
    assert_eq!(
        MessageCommand::parse_str("versionwithpayload").unwrap(),
        MessageCommand::Unknown(0x55)
    );
    assert_eq!(
        MessageCommand::parse_str("extendedc0").unwrap(),
        MessageCommand::Unknown(0xc0)
    );
}

#[test]
fn unknown_text_returns_the_network_owned_parse_error() {
    let error = "future-command".parse::<MessageCommand>().unwrap_err();
    assert_eq!(error, MessageCommandParseError("future-command".to_owned()));
    assert_eq!(error.to_string(), "Unknown message command: future-command");
}

#[test]
fn outbound_queue_single_command_set_matches_remote_node_policy() {
    assert_eq!(
        MessageCommand::SINGLE_QUEUED_COMMANDS,
        [
            MessageCommand::Addr,
            MessageCommand::GetAddr,
            MessageCommand::GetBlocks,
            MessageCommand::GetHeaders,
            MessageCommand::Mempool,
            MessageCommand::Ping,
            MessageCommand::Pong,
        ]
    );

    for command in MessageCommand::SINGLE_QUEUED_COMMANDS {
        assert!(command.is_single_queued(), "{command:?}");
    }

    assert!(!MessageCommand::Inv.is_single_queued());
    assert!(!MessageCommand::Unknown(0x99).is_single_queued());
}

#[test]
fn outbound_queue_high_priority_set_matches_remote_node_policy() {
    assert_eq!(
        MessageCommand::HIGH_PRIORITY_COMMANDS,
        [
            MessageCommand::Alert,
            MessageCommand::Extensible,
            MessageCommand::FilterAdd,
            MessageCommand::FilterClear,
            MessageCommand::FilterLoad,
            MessageCommand::GetAddr,
            MessageCommand::Mempool,
        ]
    );

    for command in MessageCommand::HIGH_PRIORITY_COMMANDS {
        assert!(command.is_high_priority_queue(), "{command:?}");
    }

    assert!(!MessageCommand::Ping.is_high_priority_queue());
    assert!(!MessageCommand::Unknown(0x99).is_high_priority_queue());
}
