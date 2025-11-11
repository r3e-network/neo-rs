mod error;
mod primitives;
mod reader;
mod traits;
mod varint;
mod writer;

#[cfg(test)]
mod tests;

pub use error::DecodeError;
pub use reader::SliceReader;
pub use traits::{NeoDecode, NeoEncode, NeoRead, NeoWrite};
pub use varint::{read_varint, to_varint_le, write_varint};
