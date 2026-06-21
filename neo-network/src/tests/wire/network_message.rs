use super::*;
use neo_payloads::ping_payload::PingPayload;

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
    let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
    assert!(matches!(decoded.payload, ProtocolMessage::Verack));
}
