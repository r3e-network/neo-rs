// Converted from C# Neo.UnitTests.Network.P2P.UT_Message
use crate::network::p2p::*;
use crate::network::p2p::payloads::*;
use crate::network::p2p::message::*;
use neo_io::*;

#[cfg(test)]
mod message_tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let payload = PingPayload::create(u32::MAX);
        let msg = Message::create(MessageCommand::Ping, Some(Box::new(payload.clone())));
        let buffer = msg.to_array().unwrap();
        let copy = Message::from_bytes(&buffer).unwrap();
        
        if let Some(payload_copy) = copy.payload() {
            if let Ok(ping_copy) = payload_copy.downcast::<PingPayload>() {
                assert_eq!(msg.command(), copy.command());
                assert_eq!(msg.flags(), copy.flags());
                assert_eq!(payload.size() + 3, msg.size());
                
                assert_eq!(payload.last_block_index(), ping_copy.last_block_index());
                assert_eq!(payload.nonce(), ping_copy.nonce());
                assert_eq!(payload.timestamp(), ping_copy.timestamp());
            } else {
                panic!("Failed to downcast payload");
            }
        } else {
            panic!("Payload is None");
        }
    }

    #[test]
    fn test_serialize_deserialize_without_payload() {
        let msg = Message::create(MessageCommand::GetAddr, None);
        let buffer = msg.to_array().unwrap();
        let copy = Message::from_bytes(&buffer).unwrap();
        
        assert_eq!(msg.command(), copy.command());
        assert_eq!(msg.flags(), copy.flags());
        assert!(copy.payload().is_none());
    }

    #[test]
    fn test_message_commands() {
        // Test all message command types
        let commands = vec![
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
            MessageCommand::GetBlockByIndex,
            MessageCommand::NotFound,
            MessageCommand::Tx,
            MessageCommand::Block,
            MessageCommand::Consensus,
            MessageCommand::Reject,
            MessageCommand::FilterLoad,
            MessageCommand::FilterAdd,
            MessageCommand::FilterClear,
            MessageCommand::MerkleBlock,
            MessageCommand::Alert,
        ];
        
        for command in commands {
            let msg = Message::create(command, None);
            assert_eq!(msg.command(), command);
        }
    }

    #[test]
    fn test_message_size_limits() {
        let msg = Message::create(MessageCommand::GetAddr, None);
        assert!(msg.size() <= Message::MAX_SIZE);
    }

    #[test]
    fn test_placeholder() {
        // Placeholder for additional message tests
        assert!(true);
    }
}
