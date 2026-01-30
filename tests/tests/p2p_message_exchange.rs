//! P2P Message Exchange Integration Tests
//!
//! Tests for peer-to-peer networking protocol

use neo_core::network::p2p::{
    message::Message, message_command::MessageCommand, message_flags::MessageFlags,
    payloads::PingPayload,
};

use neo_p2p::VerifyResult;

#[test]
fn test_ping_pong_messages() {
    let ping = PingPayload {
        timestamp: 1234567890,
        nonce: 42,
        last_block_index: 100,
    };

    let ping_msg = Message::create(MessageCommand::Ping, Some(&ping), false).unwrap();

    assert_eq!(ping_msg.command, MessageCommand::Ping);

    let pong = PingPayload {
        timestamp: 1234567891,
        nonce: 42,
        last_block_index: 100,
    };

    let pong_msg = Message::create(MessageCommand::Pong, Some(&pong), false).unwrap();

    assert_eq!(pong_msg.command, MessageCommand::Pong);
}

#[test]
fn test_message_command_byte_conversion() {
    let commands = vec![
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

    for cmd in commands {
        let byte = cmd.to_byte();
        let restored = MessageCommand::from_byte(byte);
        assert!(restored.is_ok());
        assert_eq!(
            restored.unwrap(),
            cmd,
            "Command {:?} should round-trip",
            cmd
        );
    }
}

#[test]
fn test_message_flags_compression() {
    let flags_none = MessageFlags::NONE;
    assert!(!flags_none.is_compressed());

    let flags_compressed = MessageFlags::COMPRESSED;
    assert!(flags_compressed.is_compressed());
}

#[test]
fn test_verify_result_variants() {
    let results = vec![
        VerifyResult::Succeed,
        VerifyResult::AlreadyExists,
        VerifyResult::UnableToVerify,
        VerifyResult::Invalid,
        VerifyResult::Unknown,
    ];

    for result in results {
        let byte = result.to_byte();
        let restored = VerifyResult::from_byte(byte);
        assert_eq!(
            restored,
            Some(result),
            "VerifyResult {:?} should round-trip",
            result
        );
    }
}

#[tokio::test]
async fn test_concurrent_message_creation() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let messages = Arc::new(Mutex::new(Vec::new()));

    let mut handles = vec![];
    for i in 0u8..10 {
        let msgs_clone = messages.clone();
        let handle = tokio::spawn(async move {
            let ping = PingPayload {
                timestamp: i as u32 * 1000,
                nonce: i as u32,
                last_block_index: i as u32 * 10,
            };

            let msg = Message::create(MessageCommand::Ping, Some(&ping), false).unwrap();

            let mut msgs = msgs_clone.lock().await;
            msgs.push(msg);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let final_messages = messages.lock().await;
    assert_eq!(final_messages.len(), 10);
}
