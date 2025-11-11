use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::Endpoint;

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
