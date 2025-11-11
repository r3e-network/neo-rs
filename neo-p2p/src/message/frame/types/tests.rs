use super::Message;
use crate::message::command::MessageCommand;
use crate::message::types::VersionPayload;
use crate::message::{AddressEntry, AddressPayload, Endpoint, NetworkAddress, RejectPayload};
use neo_base::{Bytes, NeoDecode, NeoEncode, SliceReader};
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn message_roundtrip() {
    let msg = Message::Version(VersionPayload::new(
        860_833_102,
        0x03,
        1,
        1_700_000_000,
        Endpoint::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 20333),
        Endpoint::new(IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2)), 30333),
        42,
        "/neo-rs:0.1.0".to_string(),
        12345,
        true,
    ));

    let mut buf = Vec::new();
    msg.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = Message::neo_decode(&mut reader).unwrap();
    assert!(matches!(decoded, Message::Version(_)));
}

#[test]
fn addr_payload_roundtrip() {
    let entry = AddressEntry::new(
        1_700_000_000,
        NetworkAddress::new(
            1,
            Endpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 30333),
        ),
    );

    let payload = AddressPayload::new(vec![entry]);
    let message = Message::Address(payload.clone());

    let mut buf = Vec::new();
    message.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = Message::neo_decode(&mut reader).unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn compression_flag_roundtrip() {
    let entries = (0..300)
        .map(|i| {
            AddressEntry::new(
                i,
                NetworkAddress::new(
                    1,
                    Endpoint::new(IpAddr::V4(Ipv4Addr::new(1, 1, (i & 0xFF) as u8, 1)), 2000),
                ),
            )
        })
        .collect::<Vec<_>>();
    let message = Message::Address(AddressPayload::new(entries));
    let mut buf = Vec::new();
    message.neo_encode(&mut buf);

    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = Message::neo_decode(&mut reader).unwrap();
    assert!(matches!(decoded, Message::Address(_)));
}

#[test]
fn reject_payload_roundtrip() {
    let payload = RejectPayload {
        command: MessageCommand::Ping,
        code: 0x01,
        reason: "bad".into(),
        data: Bytes::from(vec![1, 2, 3]),
    };
    let message = Message::Reject(payload.clone());

    let mut buf = Vec::new();
    message.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = Message::neo_decode(&mut reader).unwrap();
    assert_eq!(decoded, Message::Reject(payload));
}
