use super::*;
use crate::wire::NetworkMessage;

#[test]
fn protocol_message_command_matches_variant() {
    assert_eq!(ProtocolMessage::Verack.command(), MessageCommand::Verack);
    assert_eq!(ProtocolMessage::pong(42).command(), MessageCommand::Pong);
}

#[test]
fn empty_command_round_trip() {
    let payload = ProtocolMessage::Verack
        .serialize_payload()
        .expect("serialize");
    assert!(payload.is_empty());
    let decoded = ProtocolMessage::deserialize_payload(MessageCommand::Verack, &payload)
        .expect("deserialize");
    matches!(decoded, ProtocolMessage::Verack);
}

#[test]
fn no_payload_commands_ignore_extra_payload_like_csharp_reflection_cache() {
    let frame =
        crate::wire::Message::from_payload_bytes(MessageCommand::Verack, vec![0xCA, 0xFE], false)
            .expect("message")
            .to_bytes()
            .expect("wire bytes");

    let decoded = NetworkMessage::from_bytes(&frame)
        .expect("C# ReflectionCache leaves undecorated command payloads untyped");
    assert!(matches!(decoded.payload, ProtocolMessage::Verack));

    let decoded = ProtocolMessage::deserialize_payload(MessageCommand::Mempool, &[1, 2, 3])
        .expect("Mempool is undecorated in C# MessageCommand");
    assert!(matches!(decoded, ProtocolMessage::Mempool));
}

#[test]
fn ping_round_trip() {
    let ping = ProtocolMessage::Ping(PingPayload::create(7));
    let bytes = ping.serialize_payload().expect("serialize");
    let decoded =
        ProtocolMessage::deserialize_payload(MessageCommand::Ping, &bytes).expect("deserialize");
    match decoded {
        ProtocolMessage::Ping(p) => assert_eq!(p.last_block_index, 7),
        other => panic!("unexpected variant: {other:?}"),
    }
}
