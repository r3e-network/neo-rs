//! P2P Message Exchange Integration Tests
//!
//! Tests for peer-to-peer networking protocol.
//!
//! These tests validate the public network-owned command and flag surface.
//! Shared verification outcomes remain owned by `neo-primitives`.

use neo_network::{MessageCommand, MessageFlags};
use neo_primitives::VerifyResult;

#[tokio::test]
async fn message_command_byte_conversion_round_trips() {
    let commands = [
        MessageCommand::Version,
        MessageCommand::Verack,
        MessageCommand::Ping,
        MessageCommand::Pong,
        MessageCommand::GetAddr,
        MessageCommand::Addr,
        MessageCommand::GetBlocks,
        MessageCommand::Block,
        MessageCommand::Transaction,
        MessageCommand::Inv,
        MessageCommand::GetData,
        MessageCommand::Headers,
        MessageCommand::GetHeaders,
    ];

    for cmd in commands.iter() {
        let byte = cmd.to_byte();
        let restored = MessageCommand::from_byte(byte);
        assert_eq!(restored, *cmd, "Command {:?} did not round-trip", cmd);
    }
}

#[tokio::test]
async fn message_flags_compression_predicate() {
    assert!(!MessageFlags::NONE.is_compressed());
    assert!(MessageFlags::COMPRESSED.is_compressed());
}

#[tokio::test]
async fn verify_result_variants_round_trip() {
    let results = [
        VerifyResult::Succeed,
        VerifyResult::AlreadyExists,
        VerifyResult::UnableToVerify,
        VerifyResult::Invalid,
        VerifyResult::Unknown,
    ];

    for result in results.iter() {
        let byte = result.to_byte();
        let restored = VerifyResult::from_byte(byte);
        assert_eq!(
            restored,
            Some(*result),
            "VerifyResult {:?} did not round-trip",
            result
        );
    }
}
