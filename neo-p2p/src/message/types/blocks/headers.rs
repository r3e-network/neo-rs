use neo_base::{
    encoding::{read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    Bytes,
};

use super::super::MAX_HEADERS_COUNT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadersPayload {
    pub headers: Vec<Bytes>,
}

impl HeadersPayload {
    pub fn new(headers: Vec<Bytes>) -> Self {
        Self { headers }
    }
}

impl NeoEncode for HeadersPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let count = self.headers.len() as u64;
        debug_assert!(
            (1..=MAX_HEADERS_COUNT).contains(&count),
            "invalid headers payload size"
        );
        write_varint(writer, count);
        for header in &self.headers {
            header.neo_encode(writer);
        }
    }
}

impl NeoDecode for HeadersPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let count = read_varint(reader)?;
        if count == 0 || count > MAX_HEADERS_COUNT {
            return Err(DecodeError::LengthOutOfRange {
                len: count,
                max: MAX_HEADERS_COUNT,
            });
        }
        let mut headers = Vec::with_capacity(count as usize);
        for _ in 0..count {
            headers.push(Bytes::neo_decode(reader)?);
        }
        Ok(Self { headers })
    }
}
