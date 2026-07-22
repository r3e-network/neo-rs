use super::*;
use neo_payloads::InventoryType;
use neo_payloads::p2p_payloads::{
    FilterAddPayload, GetBlocksPayload, InvPayload, NodeCapability, VersionPayload,
};
use neo_payloads::ping_payload::PingPayload;
use neo_primitives::UInt256;

#[test]
fn network_message_round_trip_ping() {
    let msg = NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create(11)));
    let bytes = msg.to_bytes(true).expect("encode");
    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.command(), MessageCommand::Ping);
    match decoded.payload {
        ProtocolMessage::Ping(p) => assert_eq!(p.last_block_index, 11),
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn network_message_round_trip_verack() {
    let msg = NetworkMessage::new(ProtocolMessage::Verack);
    let bytes = msg.to_bytes(false).expect("encode");
    assert_eq!(bytes, [0x00, 0x01, 0x00]);
    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    assert!(matches!(decoded.payload, ProtocolMessage::Verack));
}

#[test]
fn network_message_round_trip_version_uses_canonical_envelope() {
    let payload = VersionPayload {
        network: 0x0102_0304,
        version: 0x0506_0708,
        timestamp: 0x1112_1314,
        nonce: 0x2122_2324,
        user_agent: String::new(),
        start_height: 999,
        capabilities: vec![NodeCapability::full_node(42)],
    };
    let message = NetworkMessage::new(ProtocolMessage::Version(payload.clone()));
    let bytes = message.to_bytes(false).expect("encode");

    assert_eq!(
        bytes,
        [
            0x00, 0x00, 0x17, // flags, command, payload length
            0x04, 0x03, 0x02, 0x01, // network
            0x08, 0x07, 0x06, 0x05, // protocol version
            0x14, 0x13, 0x12, 0x11, // timestamp
            0x24, 0x23, 0x22, 0x21, // nonce
            0x00, // empty user agent
            0x01, 0x10, 0x2a, 0x00, 0x00, 0x00, // FullNode(42)
        ]
    );

    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    match decoded.payload {
        ProtocolMessage::Version(decoded) => {
            assert_eq!(decoded.network, payload.network);
            assert_eq!(decoded.version, payload.version);
            assert_eq!(decoded.timestamp, payload.timestamp);
            assert_eq!(decoded.nonce, payload.nonce);
            assert_eq!(decoded.user_agent, payload.user_agent);
            assert_eq!(decoded.capabilities, payload.capabilities);
            assert_eq!(decoded.start_height, 42);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn network_message_round_trip_getblocks_preserves_hash_and_count() {
    let hash = UInt256::from_bytes(&[0x01; UInt256::LENGTH]).expect("hash");
    let payload = GetBlocksPayload::create(hash, -1);
    let message = NetworkMessage::new(ProtocolMessage::GetBlocks(payload));
    let bytes = message.to_bytes(false).expect("encode");

    assert_eq!(bytes[0..3], [0x00, 0x24, 0x22]);
    assert_eq!(&bytes[3..35], &[0x01; UInt256::LENGTH]);
    assert_eq!(&bytes[35..], &[0xff, 0xff]);

    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    match decoded.payload {
        ProtocolMessage::GetBlocks(decoded) => {
            assert_eq!(decoded.hash_start, hash);
            assert_eq!(decoded.count, -1);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn network_message_round_trip_inv_preserves_inventory_type_and_hashes() {
    let hashes = vec![
        UInt256::from_bytes(&[0x11; UInt256::LENGTH]).expect("first hash"),
        UInt256::from_bytes(&[0x22; UInt256::LENGTH]).expect("second hash"),
    ];
    let payload = InvPayload::create(InventoryType::Block, &hashes);
    let message = NetworkMessage::new(ProtocolMessage::Inv(payload));
    let bytes = message.to_bytes(false).expect("encode");

    assert_eq!(bytes[0..3], [0x00, 0x27, 0x42]);
    assert_eq!(bytes[3], InventoryType::Block.to_byte());
    assert_eq!(bytes[4], 0x02); // two hashes
    assert_eq!(&bytes[5..37], &[0x11; UInt256::LENGTH]);
    assert_eq!(&bytes[37..], &[0x22; UInt256::LENGTH]);

    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    match decoded.payload {
        ProtocolMessage::Inv(decoded) => {
            assert_eq!(decoded.inventory_type, InventoryType::Block);
            assert_eq!(decoded.hashes, hashes);
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn network_message_with_flags_preserves_reserved_bits_and_recomputes_compression() {
    let payload = ProtocolMessage::FilterAdd(FilterAddPayload::new(vec![0xab; 200]));
    let message = NetworkMessage::with_flags(payload, MessageFlags::from_byte(0x81));

    let compressed = message.to_bytes(true).expect("encode compressed");
    assert_eq!(compressed[0], 0x81, "reserved bits plus actual compression");
    let decoded = NetworkMessage::from_bytes(&compressed).expect("decode compressed");
    assert_eq!(decoded.flags.to_byte(), 0x81);

    let uncompressed = message.to_bytes(false).expect("encode uncompressed");
    assert_eq!(
        uncompressed[0], 0x80,
        "compression must be disabled by policy"
    );
    let decoded = NetworkMessage::from_bytes(&uncompressed).expect("decode uncompressed");
    assert_eq!(decoded.flags.to_byte(), 0x80);
}

#[test]
fn network_message_with_stale_compressed_flag_never_marks_raw_payload_compressed() {
    let payload = ProtocolMessage::FilterAdd(FilterAddPayload::new(vec![0x01, 0x02, 0x03]));
    let message = NetworkMessage::with_flags(payload, MessageFlags::COMPRESSED);

    let bytes = message.to_bytes(true).expect("encode");
    assert_eq!(
        bytes[0], 0x00,
        "short payload does not meet compression threshold"
    );
    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.flags, MessageFlags::NONE);
}
