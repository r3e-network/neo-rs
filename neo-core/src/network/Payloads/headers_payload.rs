use neo_core::io::{Serializable, MemoryReader, BinaryWriter};
use crate::network::payloads::Header;
use std::io;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::Header;

/// This message is sent to respond to GetHeaders messages.
pub struct HeadersPayload {
    /// The list of headers.
    pub headers: Vec<Header>,
}

impl HeadersPayload {
    /// Indicates the maximum number of headers sent each time.
    pub const MAX_HEADERS_COUNT: usize = 2000;

    /// Creates a new instance of the HeadersPayload struct.
    pub fn new(headers: Vec<Header>) -> Self {
        HeadersPayload { headers }
    }

    /// Returns the size of the payload.
    pub fn size(&self) -> usize {
        self.headers.len() * std::mem::size_of::<Header>()
    }
}

impl ISerializable for HeadersPayload {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), io::Error> {
        writer.write_serializable_list(&self.headers)
    }

    fn deserialize(&mut self, reader: &mut MemoryReader) -> Result<Self, io::Error> {
        let headers = reader.read_serializable_list::<Header>(Self::MAX_HEADERS_COUNT)?;
        if headers.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Empty headers list"));
        }
        Ok(HeadersPayload { headers })
    }
}
