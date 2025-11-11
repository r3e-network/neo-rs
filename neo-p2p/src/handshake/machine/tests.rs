use crate::{
    handshake::{build_version_payload, HandshakeError, HandshakeMachine, HandshakeRole},
    message::{Capability, Message, VersionPayload},
};

fn sample_version(port: u16) -> VersionPayload {
    build_version_payload(
        0x74746e41,
        0x03,
        format!("/test-node:{port}"),
        vec![
            Capability::tcp_server(port),
            Capability::full_node(100),
            Capability::ArchivalNode,
        ],
    )
}

#[test]
fn outbound_handshake_flow() {
    let mut machine = HandshakeMachine::new(HandshakeRole::Outbound, sample_version(2000));
    let initial = machine.start().unwrap();
    assert!(matches!(initial, Message::Version(_)));

    let replies = machine
        .on_message(&Message::Version(sample_version(3000)))
        .unwrap();
    assert_eq!(replies, vec![Message::Verack]);
    assert!(!machine.is_complete());

    let replies = machine.on_message(&Message::Verack).unwrap();
    assert!(replies.is_empty());
    assert!(machine.is_complete());
}

#[test]
fn rejects_network_mismatch() {
    let mut machine = HandshakeMachine::new(HandshakeRole::Outbound, sample_version(9000));
    let mut other = sample_version(9001);
    other.network = 0xDEADBEEF;
    let err = machine
        .on_message(&Message::Version(other))
        .expect_err("network mismatch error");
    assert!(matches!(err, HandshakeError::NetworkMismatch { .. }));
}

#[test]
fn rejects_self_connection() {
    let version = sample_version(9100);
    let other = version.clone();
    let mut machine = HandshakeMachine::new(HandshakeRole::Outbound, version);
    let err = machine
        .on_message(&Message::Version(other))
        .expect_err("self connection");
    assert!(matches!(err, HandshakeError::SelfConnection));
}

#[test]
fn inbound_handshake_flow() {
    let mut machine = HandshakeMachine::new(HandshakeRole::Inbound, sample_version(4000));
    assert!(machine.start().is_none());

    let replies = machine
        .on_message(&Message::Version(sample_version(5000)))
        .unwrap();
    assert_eq!(replies.len(), 2);
    assert!(matches!(replies[0], Message::Version(_)));
    assert_eq!(replies[1], Message::Verack);
    assert!(!machine.is_complete());

    machine.on_message(&Message::Verack).unwrap();
    assert!(machine.is_complete());
}
