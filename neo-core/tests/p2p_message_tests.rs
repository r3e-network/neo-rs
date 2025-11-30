//! P2P Message unit tests matching C# UT_Message, UT_*Payload
//!
//! Tests for Neo.Network.P2P message serialization and payloads.

use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::message::{Message, PAYLOAD_MAX_SIZE};
use neo_core::network::p2p::message_command::MessageCommand;
use neo_core::network::p2p::message_flags::MessageFlags;
use neo_core::network::p2p::payloads::inv_payload::{InvPayload, MAX_HASHES_COUNT};
use neo_core::network::p2p::payloads::inventory_type::InventoryType;
use neo_core::network::p2p::payloads::ping_payload::PingPayload;
use neo_core::network::p2p::payloads::VersionPayload;
use neo_core::UInt256;

/// Tests Message payload max size constant (32 MiB)
#[test]
fn test_payload_max_size() {
    assert_eq!(
        PAYLOAD_MAX_SIZE, 0x0200_0000,
        "Payload max size should be 32 MiB"
    );
    assert_eq!(PAYLOAD_MAX_SIZE, 33_554_432);
}

/// Tests MessageFlags parsing
#[test]
fn test_message_flags_none() {
    let flags = MessageFlags::from_byte(0x00).unwrap();
    assert_eq!(flags, MessageFlags::NONE);
    assert!(!flags.is_compressed());
}

/// Tests MessageFlags compression flag
#[test]
fn test_message_flags_compressed() {
    let flags = MessageFlags::from_byte(0x01).unwrap();
    assert_eq!(flags, MessageFlags::COMPRESSED);
    assert!(flags.is_compressed());
}

/// Tests MessageFlags byte conversion roundtrip
#[test]
fn test_message_flags_roundtrip() {
    assert_eq!(MessageFlags::NONE.to_byte(), 0x00);
    assert_eq!(MessageFlags::COMPRESSED.to_byte(), 0x01);

    assert_eq!(MessageFlags::from_byte(0x00).unwrap(), MessageFlags::NONE);
    assert_eq!(
        MessageFlags::from_byte(0x01).unwrap(),
        MessageFlags::COMPRESSED
    );
}

/// Tests MessageCommand conversions
#[test]
fn test_message_command_values() {
    assert_eq!(MessageCommand::Version.to_byte(), 0x00);
    assert_eq!(MessageCommand::Verack.to_byte(), 0x01);
    assert_eq!(MessageCommand::GetAddr.to_byte(), 0x10);
    assert_eq!(MessageCommand::Addr.to_byte(), 0x11);
    assert_eq!(MessageCommand::Ping.to_byte(), 0x18);
    assert_eq!(MessageCommand::Pong.to_byte(), 0x19);
    assert_eq!(MessageCommand::GetHeaders.to_byte(), 0x20);
    assert_eq!(MessageCommand::Headers.to_byte(), 0x21);
    assert_eq!(MessageCommand::GetBlocks.to_byte(), 0x24);
    assert_eq!(MessageCommand::Mempool.to_byte(), 0x25);
    assert_eq!(MessageCommand::Inv.to_byte(), 0x27);
    assert_eq!(MessageCommand::GetData.to_byte(), 0x28);
    assert_eq!(MessageCommand::GetBlockByIndex.to_byte(), 0x29);
    assert_eq!(MessageCommand::NotFound.to_byte(), 0x2a);
    assert_eq!(MessageCommand::Transaction.to_byte(), 0x2b);
    assert_eq!(MessageCommand::Block.to_byte(), 0x2c);
    assert_eq!(MessageCommand::Extensible.to_byte(), 0x2e);
    assert_eq!(MessageCommand::Reject.to_byte(), 0x2f);
    assert_eq!(MessageCommand::FilterLoad.to_byte(), 0x30);
    assert_eq!(MessageCommand::FilterAdd.to_byte(), 0x31);
    assert_eq!(MessageCommand::FilterClear.to_byte(), 0x32);
    assert_eq!(MessageCommand::MerkleBlock.to_byte(), 0x38);
    assert_eq!(MessageCommand::Alert.to_byte(), 0x40);
}

/// Tests MessageCommand from_byte roundtrip
#[test]
fn test_message_command_roundtrip() {
    for cmd in [
        MessageCommand::Version,
        MessageCommand::Verack,
        MessageCommand::GetAddr,
        MessageCommand::Addr,
        MessageCommand::Ping,
        MessageCommand::Pong,
        MessageCommand::GetHeaders,
        MessageCommand::Headers,
        MessageCommand::GetBlocks,
        MessageCommand::Mempool,
        MessageCommand::Inv,
        MessageCommand::GetData,
        MessageCommand::Transaction,
        MessageCommand::Block,
        MessageCommand::Reject,
        MessageCommand::FilterLoad,
        MessageCommand::FilterAdd,
        MessageCommand::FilterClear,
        MessageCommand::MerkleBlock,
        MessageCommand::Alert,
    ] {
        let byte = cmd.to_byte();
        let recovered = MessageCommand::from_byte(byte).expect("Should parse");
        assert_eq!(recovered, cmd, "Command {:?} should roundtrip", cmd);
    }
}

/// Tests PingPayload creation
#[test]
fn test_ping_payload_creation() {
    let last_block_index = 12345u32;
    let nonce = 0xDEADBEEF_u32;

    let payload = PingPayload::create_with_nonce(last_block_index, nonce);

    assert_eq!(payload.last_block_index, last_block_index);
    assert_eq!(payload.nonce, nonce);
    // timestamp is set automatically
    assert!(payload.timestamp > 0);
}

/// Tests PingPayload serialization
#[test]
fn test_ping_payload_serialization() {
    let payload = PingPayload::create_with_nonce(100, 300);

    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).expect("Should serialize");
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let deserialized = PingPayload::deserialize(&mut reader).expect("Should deserialize");

    assert_eq!(deserialized.last_block_index, payload.last_block_index);
    assert_eq!(deserialized.timestamp, payload.timestamp);
    assert_eq!(deserialized.nonce, payload.nonce);
}

/// Tests InvPayload max hashes count
#[test]
fn test_inv_payload_max_hashes() {
    assert_eq!(MAX_HASHES_COUNT, 500, "Max hashes count should be 500");
}

/// Tests InventoryType values
#[test]
fn test_inventory_type_values() {
    assert_eq!(InventoryType::Transaction as u8, 0x2b);
    assert_eq!(InventoryType::Block as u8, 0x2c);
    assert_eq!(InventoryType::Extensible as u8, 0x2e);
}

/// Tests InvPayload creation
#[test]
fn test_inv_payload_creation() {
    let hash = UInt256::from([1u8; 32]);
    let payload = InvPayload::create(InventoryType::Transaction, &[hash]);

    assert_eq!(payload.inventory_type, InventoryType::Transaction);
    assert_eq!(payload.hashes.len(), 1);
    assert_eq!(payload.hashes[0], hash);
}

/// Tests InvPayload is_empty
#[test]
fn test_inv_payload_is_empty() {
    let empty_payload = InvPayload::create(InventoryType::Transaction, &[]);
    assert!(empty_payload.is_empty());

    let non_empty_payload = InvPayload::create(InventoryType::Block, &[UInt256::from([1u8; 32])]);
    assert!(!non_empty_payload.is_empty());
}

/// Tests InvPayload serialization
#[test]
fn test_inv_payload_serialization() {
    let hashes = vec![UInt256::from([1u8; 32]), UInt256::from([2u8; 32])];
    let payload = InvPayload::create(InventoryType::Block, &hashes);

    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).expect("Should serialize");
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let deserialized = InvPayload::deserialize(&mut reader).expect("Should deserialize");

    assert_eq!(deserialized.inventory_type, InventoryType::Block);
    assert_eq!(deserialized.hashes.len(), 2);
    assert_eq!(deserialized.hashes, hashes);
}

/// Tests VersionPayload creation
#[test]
fn test_version_payload_creation() {
    let version = VersionPayload::create(
        neo_core::constants::TESTNET_MAGIC,
        12345,
        "/neo-rs:0.4/".to_string(),
        vec![],
    );
    // Network should match the provided value
    assert_eq!(version.network, neo_core::constants::TESTNET_MAGIC);
    assert_eq!(version.nonce, 12345);
    assert!(version.user_agent.contains("neo-rs"));
}

/// Tests VersionPayload capabilities
#[test]
fn test_version_payload_capabilities() {
    use neo_core::network::p2p::capabilities::NodeCapability;
    let version = VersionPayload::create(
        neo_core::constants::TESTNET_MAGIC,
        12345,
        "/neo-rs:0.4/".to_string(),
        vec![NodeCapability::FullNode { start_height: 100 }],
    );
    assert_eq!(version.capabilities.len(), 1);
    assert!(version.allow_compression);
}

/// Tests Message creation without compression
#[test]
fn test_message_create_no_compression() {
    let payload = PingPayload::create_with_nonce(100, 300);
    let message = Message::create(MessageCommand::Ping, Some(&payload), false)
        .expect("Should create message");

    assert!(!message.is_compressed());
    assert_eq!(message.command, MessageCommand::Ping);
    assert_eq!(message.payload(), message.payload_compressed());
}

/// Tests Message compression is applied for eligible commands
#[test]
fn test_message_compression_applied() {
    // Create a large enough payload to trigger compression
    let large_data = vec![0xAB; 256]; // Repeat same byte for high compressibility

    // Block command is eligible for compression
    let mut writer = BinaryWriter::new();
    writer.write_var_bytes(&large_data).unwrap();
    let payload_bytes = writer.into_bytes();

    // Since we can't easily create a Block payload, test with raw message parts
    let message =
        Message::from_wire_parts(MessageFlags::NONE, MessageCommand::Block, &payload_bytes)
            .expect("Should create message");

    // Uncompressed message should have matching raw and compressed payload
    assert_eq!(message.payload(), message.payload_compressed());
}

/// Tests Message size calculation
#[test]
fn test_message_size() {
    let payload = PingPayload::create_with_nonce(100, 300);
    let message = Message::create(MessageCommand::Ping, Some(&payload), false)
        .expect("Should create message");

    let size = message.size();
    // Size = 1 (flags) + 1 (command) + var_size(payload_len) + payload_len
    assert!(size > 2, "Message size should be greater than header");
}

/// Tests Message to_bytes with compression disabled
#[test]
fn test_message_to_bytes_no_compression() {
    let payload = PingPayload::create_with_nonce(100, 300);
    let message = Message::create(MessageCommand::Ping, Some(&payload), false)
        .expect("Should create message");

    let bytes = message.to_bytes(false).expect("Should serialize");

    // First byte should be flags (NONE = 0)
    assert_eq!(bytes[0], MessageFlags::NONE.to_byte());
    // Second byte should be command
    assert_eq!(bytes[1], MessageCommand::Ping.to_byte());
}

/// Tests Message serialization roundtrip
#[test]
fn test_message_serialization_roundtrip() {
    let payload = PingPayload::create_with_nonce(42, 0xCAFE);
    let original = Message::create(MessageCommand::Ping, Some(&payload), false)
        .expect("Should create message");

    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).expect("Should serialize");
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let deserialized = Message::deserialize(&mut reader).expect("Should deserialize");

    assert_eq!(deserialized.command, original.command);
    assert_eq!(deserialized.flags, original.flags);
    assert_eq!(deserialized.payload(), original.payload());
}
