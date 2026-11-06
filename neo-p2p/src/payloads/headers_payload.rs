//! Headers payload for `GetHeaders` responses (mirrors `Neo.Network.P2P.Payloads.HeadersPayload`).

use neo_io::serializable::helper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_protocol::payloads::Header;
use serde::{Deserialize, Serialize};

/// Maximum number of headers sent in a single response (matches C# `MaxHeadersCount`).
pub const MAX_HEADERS_COUNT: usize = 2000;

/// Response payload carrying a batch of block headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadersPayload {
    /// The list of headers.
    pub headers: Vec<Header>,
}

impl HeadersPayload {
    /// Creates a new headers payload from a vector of headers.
    pub fn create(headers: Vec<Header>) -> Self {
        Self { headers }
    }

    /// Returns the number of headers in the payload.
    pub fn len(&self) -> usize {
        self.headers.len()
    }

    /// Returns `true` when the payload carries no headers.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

impl Serializable for HeadersPayload {
    fn size(&self) -> usize {
        helper::get_var_size_serializable_slice(&self.headers)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        helper::serialize_array(&self.headers, writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let headers = helper::deserialize_array::<Header>(reader, MAX_HEADERS_COUNT)?;
        if headers.is_empty() {
            return Err(IoError::invalid_data("Empty headers list"));
        }
        Ok(Self { headers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_payload_create_stores_headers() {
        let mut h1 = Header::new();
        h1.set_index(1);
        let mut h2 = Header::new();
        h2.set_index(2);

        let payload = HeadersPayload::create(vec![h1, h2]);
        assert_eq!(payload.len(), 2);
        assert!(!payload.is_empty());
        assert_eq!(payload.headers[0].index(), 1);
        assert_eq!(payload.headers[1].index(), 2);
    }

    #[test]
    fn headers_payload_serialization_roundtrip() {
        let mut h = Header::new();
        h.set_index(42);
        let payload = HeadersPayload::create(vec![h]);

        let mut writer = BinaryWriter::new();
        // Fully qualified call to disambiguate between `serde::Serialize` (derived)
        // and `neo_io::Serializable` (impl'd below); both expose `serialize`.
        Serializable::serialize(&payload, &mut writer).unwrap();
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        let decoded = <HeadersPayload as Serializable>::deserialize(&mut reader).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded.headers[0].index(), 42);
    }

    #[test]
    fn headers_payload_deserialize_rejects_empty() {
        // Serialize an empty var-array (length prefix 0)
        let mut writer = BinaryWriter::new();
        writer.write_var_int(0).unwrap();
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        let result = <HeadersPayload as Serializable>::deserialize(&mut reader);
        assert!(result.is_err());
    }
}
