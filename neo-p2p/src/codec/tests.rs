use super::NeoMessageCodec;
use crate::message::{
    AddressEntry, AddressPayload, Capability, Endpoint, InventoryItem, InventoryKind,
    InventoryPayload, Message, NetworkAddress, PingPayload, VersionPayload,
};
use bytes::BytesMut;
use neo_base::hash::Hash256;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use tokio_util::codec::{Decoder, Encoder};

const HEADER_LEN: usize = 4 + 12 + 4 + 4;

#[test]
fn codec_roundtrip() {
    const MAGIC: u32 = 0x7474_6E41;
    let mut codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let version = Message::Version(VersionPayload::new(
        860_833_102,
        1,
        1,
        42,
        "/neo".into(),
        vec![Capability::tcp_server(20333), Capability::full_node(100)],
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
    const MAGIC: u32 = 0x7474_6E41;
    let mut encoder = NeoMessageCodec::new().with_network_magic(MAGIC);
    let message = Message::Ping(PingPayload {
        last_block_index: 123,
        timestamp: 456,
        nonce: 789,
    });
    let mut encoded = BytesMut::new();
    encoder.encode(message.clone(), &mut encoded).unwrap();
    let full = encoded.freeze();

    let mut codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let mut buf = BytesMut::from(&full[..10]);
    assert!(codec.decode(&mut buf).unwrap().is_none());

    buf.extend_from_slice(&full[10..HEADER_LEN]);
    assert!(codec.decode(&mut buf).unwrap().is_none());

    buf.extend_from_slice(&full[HEADER_LEN..HEADER_LEN + 2]);
    assert!(codec.decode(&mut buf).unwrap().is_none());

    buf.extend_from_slice(&full[HEADER_LEN + 2..]);
    let decoded = codec.decode(&mut buf).unwrap().unwrap();
    assert_eq!(decoded, message);
    assert!(buf.is_empty());
}

#[test]
fn codec_rejects_wrong_magic() {
    const MAGIC: u32 = 0x7474_6E41;
    let mut codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let message = Message::GetAddr;
    let mut buf = BytesMut::new();
    codec.encode(message.clone(), &mut buf).unwrap();

    // Flip magic to force mismatch.
    buf[..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    let mut decode_codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let err = decode_codec.decode(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn codec_rejects_bad_checksum() {
    const MAGIC: u32 = 0x7474_6E41;
    let mut codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let message = Message::Ping(PingPayload {
        last_block_index: 1,
        timestamp: 2,
        nonce: 3,
    });
    let mut buf = BytesMut::new();
    codec.encode(message.clone(), &mut buf).unwrap();

    // Flip one payload byte.
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;

    let mut decode_codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let err = decode_codec.decode(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn codec_rejects_command_mismatch() {
    const MAGIC: u32 = 0x1956_3210;
    let mut codec = NeoMessageCodec::new().with_network_magic(MAGIC);
    let inventory = Message::Inventory(InventoryPayload::new(vec![InventoryItem {
        kind: InventoryKind::Block,
        hash: Hash256::new([1u8; 32]),
    }]));
    let mut buf = BytesMut::new();
    codec.encode(inventory.clone(), &mut buf).unwrap();

    // Rewrite command name to "ping".
    let mut name = [0u8; 12];
    name[..4].copy_from_slice(b"ping");
    buf[4..16].copy_from_slice(&name);

    let mut decoder = NeoMessageCodec::new().with_network_magic(MAGIC);
    let err = decoder.decode(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}
