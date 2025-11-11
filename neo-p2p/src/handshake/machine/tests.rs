use std::net::{IpAddr, Ipv4Addr};

use crate::{
    handshake::{build_version_payload, HandshakeMachine, HandshakeRole},
    message::{Endpoint, Message, VersionPayload},
};

fn sample_endpoint(port: u16) -> Endpoint {
    Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn sample_version(port: u16) -> VersionPayload {
    build_version_payload(
        0x74746e41,
        0x03,
        1,
        sample_endpoint(port),
        sample_endpoint(port + 1),
        100,
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
