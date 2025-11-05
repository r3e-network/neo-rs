use std::io;

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use neo_base::encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, SliceReader,
};

use crate::message::{Message, MAX_MESSAGE_SIZE};

#[derive(Default)]
pub struct NeoMessageCodec {
    expected_len: Option<usize>,
}

impl NeoMessageCodec {
    pub fn new() -> Self {
        Self { expected_len: None }
    }

    fn read_length(src: &mut BytesMut) -> io::Result<Option<usize>> {
        if src.is_empty() {
            return Ok(None);
        }

        let mut reader = SliceReader::new(src.as_ref());
        match read_varint(&mut reader) {
            Ok(len) => {
                let consumed = reader.consumed();
                src.advance(consumed);
                Ok(Some(len as usize))
            }
            Err(DecodeError::UnexpectedEof { .. }) => Ok(None),
            Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
        }
    }
}

impl Encoder<Message> for NeoMessageCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut payload = Vec::new();
        item.neo_encode(&mut payload);
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }

        write_varint(dst, payload.len() as u64);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

impl Decoder for NeoMessageCodec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let len = match self.expected_len {
            Some(len) => len,
            None => match Self::read_length(src)? {
                Some(len) => {
                    if len > MAX_MESSAGE_SIZE {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "message too large",
                        ));
                    }
                    self.expected_len = Some(len);
                    len
                }
                None => return Ok(None),
            },
        };

        if src.len() < len {
            return Ok(None);
        }

        let payload = src.split_to(len);
        self.expected_len = None;

        let mut reader = SliceReader::new(payload.as_ref());
        Message::neo_decode(&mut reader)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{
        AddressEntry, AddressPayload, Endpoint, InventoryItem, InventoryKind, InventoryPayload,
        Message, NetworkAddress, PingPayload, VersionPayload,
    };
    use neo_base::hash::Hash256;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn codec_roundtrip() {
        let mut codec = NeoMessageCodec::new();
        let version = Message::Version(VersionPayload::new(
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
        let ping = Message::Ping(PingPayload { nonce: 99 });
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

        let decoded_one = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded_one, version);
        let decoded_two = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded_two, ping);
        let decoded_three = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded_three, inventory);
        let decoded_four = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded_four, getaddr);
        let decoded_five = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded_five, addr);
    }
}
