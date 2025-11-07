use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash256,
    Bytes,
};

pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;
const MAX_INVENTORY_ITEMS: u64 = 4096;
const MAX_ADDRESS_COUNT: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub address: IpAddr,
    pub port: u16,
}

impl Endpoint {
    pub fn new(address: IpAddr, port: u16) -> Self {
        Self { address, port }
    }

    fn encode_ip(&self) -> [u8; 16] {
        match self.address {
            IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
            IpAddr::V6(v6) => v6.octets(),
        }
    }

    fn decode_ip(bytes: [u8; 16]) -> IpAddr {
        if let Some(v4) = Self::maybe_ipv4(bytes) {
            IpAddr::V4(v4)
        } else {
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
    }

    fn maybe_ipv4(bytes: [u8; 16]) -> Option<Ipv4Addr> {
        if bytes[..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF] {
            Some(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]))
        } else {
            None
        }
    }
}

impl NeoEncode for Endpoint {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.encode_ip());
        writer.write_u16(self.port);
    }
}

impl NeoDecode for Endpoint {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; 16];
        reader.read_into(&mut buf)?;
        let port = reader.read_u16()?;
        Ok(Endpoint {
            address: Self::decode_ip(buf),
            port,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPayload {
    pub network: u32,
    pub protocol: u32,
    pub services: u64,
    pub timestamp: i64,
    pub receiver: Endpoint,
    pub sender: Endpoint,
    pub nonce: u64,
    pub user_agent: String,
    pub start_height: u32,
    pub relay: bool,
}

impl VersionPayload {
    pub fn new(
        network: u32,
        protocol: u32,
        services: u64,
        timestamp: i64,
        receiver: Endpoint,
        sender: Endpoint,
        nonce: u64,
        user_agent: String,
        start_height: u32,
        relay: bool,
    ) -> Self {
        Self {
            network,
            protocol,
            services,
            timestamp,
            receiver,
            sender,
            nonce,
            user_agent,
            start_height,
            relay,
        }
    }
}

impl NeoEncode for VersionPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.network.neo_encode(writer);
        self.protocol.neo_encode(writer);
        self.services.neo_encode(writer);
        self.timestamp.neo_encode(writer);
        self.receiver.neo_encode(writer);
        self.sender.neo_encode(writer);
        self.nonce.neo_encode(writer);
        self.user_agent.neo_encode(writer);
        self.start_height.neo_encode(writer);
        self.relay.neo_encode(writer);
    }
}

impl NeoDecode for VersionPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            network: u32::neo_decode(reader)?,
            protocol: u32::neo_decode(reader)?,
            services: u64::neo_decode(reader)?,
            timestamp: i64::neo_decode(reader)?,
            receiver: Endpoint::neo_decode(reader)?,
            sender: Endpoint::neo_decode(reader)?,
            nonce: u64::neo_decode(reader)?,
            user_agent: String::neo_decode(reader)?,
            start_height: u32::neo_decode(reader)?,
            relay: bool::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingPayload {
    pub last_block_index: u32,
    pub timestamp: u32,
    pub nonce: u32,
}

impl NeoEncode for PingPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.last_block_index.neo_encode(writer);
        self.timestamp.neo_encode(writer);
        self.nonce.neo_encode(writer);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAddress {
    pub services: u64,
    pub endpoint: Endpoint,
}

impl NetworkAddress {
    pub fn new(services: u64, endpoint: Endpoint) -> Self {
        Self { services, endpoint }
    }
}

impl NeoEncode for NetworkAddress {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.services.neo_encode(writer);
        self.endpoint.neo_encode(writer);
    }
}

impl NeoDecode for NetworkAddress {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            services: u64::neo_decode(reader)?,
            endpoint: Endpoint::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressEntry {
    pub timestamp: u32,
    pub address: NetworkAddress,
}

impl AddressEntry {
    pub fn new(timestamp: u32, address: NetworkAddress) -> Self {
        Self { timestamp, address }
    }
}

impl NeoEncode for AddressEntry {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.timestamp.neo_encode(writer);
        self.address.neo_encode(writer);
    }
}

impl NeoDecode for AddressEntry {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            timestamp: u32::neo_decode(reader)?,
            address: NetworkAddress::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressPayload {
    pub entries: Vec<AddressEntry>,
}

impl AddressPayload {
    pub fn new(entries: Vec<AddressEntry>) -> Self {
        Self { entries }
    }
}

impl NeoEncode for AddressPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let count = self.entries.len() as u64;
        debug_assert!(count <= MAX_ADDRESS_COUNT, "address list too large");
        count.neo_encode(writer);
        for entry in &self.entries {
            entry.neo_encode(writer);
        }
    }
}

impl NeoDecode for AddressPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let count = u64::neo_decode(reader)?;
        if count > MAX_ADDRESS_COUNT {
            return Err(DecodeError::LengthOutOfRange {
                len: count,
                max: MAX_ADDRESS_COUNT,
            });
        }

        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(AddressEntry::neo_decode(reader)?);
        }
        Ok(Self { entries })
    }
}

impl NeoDecode for PingPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(PingPayload {
            last_block_index: u32::neo_decode(reader)?,
            timestamp: u32::neo_decode(reader)?,
            nonce: u32::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryKind {
    Transaction,
    Block,
}

impl InventoryKind {
    fn from_byte(byte: u8) -> Result<Self, DecodeError> {
        match byte {
            0x01 => Ok(Self::Transaction),
            0x02 => Ok(Self::Block),
            _ => Err(DecodeError::InvalidValue("inventory kind")),
        }
    }

    fn as_byte(self) -> u8 {
        match self {
            Self::Transaction => 0x01,
            Self::Block => 0x02,
        }
    }
}

impl NeoEncode for InventoryKind {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.as_byte());
    }
}

impl NeoDecode for InventoryKind {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let byte = reader.read_u8()?;
        Self::from_byte(byte)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    pub kind: InventoryKind,
    pub hash: Hash256,
}

impl NeoEncode for InventoryItem {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.kind.neo_encode(writer);
        self.hash.neo_encode(writer);
    }
}

impl NeoDecode for InventoryItem {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: InventoryKind::neo_decode(reader)?,
            hash: Hash256::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryPayload {
    pub items: Vec<InventoryItem>,
}

impl InventoryPayload {
    pub fn new(items: Vec<InventoryItem>) -> Self {
        Self { items }
    }
}

impl NeoEncode for InventoryPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let count = self.items.len() as u64;
        debug_assert!(count <= MAX_INVENTORY_ITEMS, "inventory too large");
        count.neo_encode(writer);
        for item in &self.items {
            item.neo_encode(writer);
        }
    }
}

impl NeoDecode for InventoryPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let count = u64::neo_decode(reader)?;
        if count > MAX_INVENTORY_ITEMS {
            return Err(DecodeError::LengthOutOfRange {
                len: count,
                max: MAX_INVENTORY_ITEMS,
            });
        }

        let mut items = Vec::with_capacity(count as usize);
        for _ in 0..count {
            items.push(InventoryItem::neo_decode(reader)?);
        }
        Ok(Self { items })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadWithData {
    pub hash: Hash256,
    pub data: Bytes,
}

impl PayloadWithData {
    pub fn new(hash: Hash256, data: Bytes) -> Self {
        Self { hash, data }
    }
}

impl NeoEncode for PayloadWithData {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.hash.neo_encode(writer);
        self.data.neo_encode(writer);
    }
}

impl NeoDecode for PayloadWithData {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            hash: Hash256::neo_decode(reader)?,
            data: Bytes::neo_decode(reader)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Version(VersionPayload),
    Verack,
    Ping(PingPayload),
    Pong(PingPayload),
    GetAddr,
    Address(AddressPayload),
    Inventory(InventoryPayload),
    GetData(InventoryPayload),
    Block(PayloadWithData),
    Transaction(PayloadWithData),
}

impl Message {
    pub fn command(&self) -> &'static str {
        match self {
            Message::Version(_) => "version",
            Message::Verack => "verack",
            Message::Ping(_) => "ping",
            Message::Pong(_) => "pong",
            Message::GetAddr => "getaddr",
            Message::Address(_) => "addr",
            Message::Inventory(_) => "inv",
            Message::GetData(_) => "getdata",
            Message::Block(_) => "block",
            Message::Transaction(_) => "tx",
        }
    }
}

impl NeoEncode for Message {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        match self {
            Message::Version(payload) => {
                writer.write_u8(0);
                payload.neo_encode(writer);
            }
            Message::Verack => {
                writer.write_u8(1);
            }
            Message::Ping(payload) => {
                writer.write_u8(2);
                payload.neo_encode(writer);
            }
            Message::Pong(payload) => {
                writer.write_u8(3);
                payload.neo_encode(writer);
            }
            Message::GetAddr => {
                writer.write_u8(4);
            }
            Message::Address(payload) => {
                writer.write_u8(5);
                payload.neo_encode(writer);
            }
            Message::Inventory(payload) => {
                writer.write_u8(6);
                payload.neo_encode(writer);
            }
            Message::GetData(payload) => {
                writer.write_u8(7);
                payload.neo_encode(writer);
            }
            Message::Block(payload) => {
                writer.write_u8(8);
                payload.neo_encode(writer);
            }
            Message::Transaction(payload) => {
                writer.write_u8(9);
                payload.neo_encode(writer);
            }
        }
    }
}

impl NeoDecode for Message {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0 => Ok(Message::Version(VersionPayload::neo_decode(reader)?)),
            1 => Ok(Message::Verack),
            2 => Ok(Message::Ping(PingPayload::neo_decode(reader)?)),
            3 => Ok(Message::Pong(PingPayload::neo_decode(reader)?)),
            4 => Ok(Message::GetAddr),
            5 => Ok(Message::Address(AddressPayload::neo_decode(reader)?)),
            6 => Ok(Message::Inventory(InventoryPayload::neo_decode(reader)?)),
            7 => Ok(Message::GetData(InventoryPayload::neo_decode(reader)?)),
            8 => Ok(Message::Block(PayloadWithData::neo_decode(reader)?)),
            9 => Ok(Message::Transaction(PayloadWithData::neo_decode(reader)?)),
            _tag => Err(DecodeError::InvalidValue("message")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::{
        double_sha256,
        encoding::{NeoDecode, NeoEncode, SliceReader},
        hash::Hash256,
    };
    use rand::{rngs::StdRng, RngCore, SeedableRng};

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
        assert_eq!(decoded, msg);
    }

    #[test]
    fn inventory_roundtrip() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut hash_bytes = [0u8; 32];
        rng.fill_bytes(&mut hash_bytes);

        let items = vec![
            InventoryItem {
                kind: InventoryKind::Transaction,
                hash: Hash256::new(hash_bytes),
            },
            InventoryItem {
                kind: InventoryKind::Block,
                hash: Hash256::new(double_sha_rng(&mut rng)),
            },
        ];

        let inv = Message::Inventory(InventoryPayload::new(items.clone()));
        let mut buf = Vec::new();
        inv.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = Message::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, inv);
    }

    #[test]
    fn block_payload_roundtrip() {
        let mut rng = StdRng::seed_from_u64(77);
        let mut hash_bytes = [0u8; 32];
        rng.fill_bytes(&mut hash_bytes);
        let hash = Hash256::new(hash_bytes);
        let data = Bytes::from(vec![1, 2, 3, 4, 5]);
        let msg = Message::Block(PayloadWithData::new(hash, data.clone()));

        let mut buf = Vec::new();
        msg.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = Message::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, msg);

        if let Message::Block(payload) = decoded {
            assert_eq!(payload.data, data);
        } else {
            panic!("expected block message");
        }
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

        if let Message::Address(decoded_payload) = decoded {
            assert_eq!(decoded_payload.entries.len(), 1);
            assert_eq!(decoded_payload.entries[0], payload.entries[0]);
        } else {
            panic!("expected addr message");
        }
    }

    fn double_sha_rng(rng: &mut StdRng) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        double_sha256(bytes)
    }
}
