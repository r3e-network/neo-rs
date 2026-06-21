use super::*;

#[test]
fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
    let command = MessageCommand::from_byte(0x99).unwrap();
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
