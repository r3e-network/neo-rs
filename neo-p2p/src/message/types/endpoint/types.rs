use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

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
