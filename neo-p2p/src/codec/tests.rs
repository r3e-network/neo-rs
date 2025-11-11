use super::NeoMessageCodec;
use crate::message::{
    AddressEntry, AddressPayload, Endpoint, InventoryItem, InventoryKind, InventoryPayload,
    Message, NetworkAddress, PingPayload, VersionPayload,
};
use bytes::BytesMut;
use neo_base::hash::Hash256;
use std::net::{IpAddr, Ipv4Addr};
use tokio_util::codec::{Decoder, Encoder};

#[test]
fn codec_roundtrip() {
    let mut codec = NeoMessageCodec::new();
    let version = Message::Version(VersionPayload::new(
        860_833_102,
        1,
        1,
        0,
        Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20333),
        Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20334),
        7,
        "/neo".into(),
        100,
        true,
    ));
    let ping = Message::Ping(PingPayload {
        last_block_index: 42,
        timestamp: 1_700_000_123,
        nonce: 99,
    });
    let inventory = Message::Inventory(InventoryPayload::new(vec![InventoryItem {
        kind: InventoryKind::Transaction,
        hash: Hash256::new([42u8; 32]),
    }]));
    let getaddr = Message::GetAddr;
    let addr = Message::Address(AddressPayload::new(vec![AddressEntry::new(
        1_700_000_000,
        NetworkAddress::new(1, Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 30335)),
    )]));

    let mut buf = BytesMut::new();
    codec.encode(version.clone(), &mut buf).unwrap();
    codec.encode(ping.clone(), &mut buf).unwrap();
    codec.encode(inventory.clone(), &mut buf).unwrap();
    codec.encode(getaddr.clone(), &mut buf).unwrap();
    codec.encode(addr.clone(), &mut buf).unwrap();

    assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), version);
    assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), ping);
    assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), inventory);
    assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), getaddr);
    assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), addr);
}

#[test]
fn codec_waits_for_varint_bytes() {
    let mut encoder = NeoMessageCodec::new();
    let message = Message::Ping(PingPayload {
        last_block_index: 123,
        timestamp: 456,
        nonce: 789,
    });
    let mut encoded = BytesMut::new();
    encoder.encode(message.clone(), &mut encoded).unwrap();
    let full = encoded.freeze();

    let mut codec = NeoMessageCodec::new();
    let mut buf = BytesMut::from(&full[..2]);
    assert!(codec.decode(&mut buf).unwrap().is_none());

    buf.extend_from_slice(&full[2..3]);
    assert!(codec.decode(&mut buf).unwrap().is_none());

    buf.extend_from_slice(&full[3..]);
    let decoded = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(decoded, message);
    assert!(buf.is_empty());
}
