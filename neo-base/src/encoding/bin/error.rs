/// Error returned when a value cannot be decoded from the Neo binary wire format.
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("neo-bin: unexpected end of input (needed {needed}, remaining {remaining})")]
    UnexpectedEof { needed: usize, remaining: usize },

    #[error("neo-bin: invalid varint prefix 0x{0:02X}")]
    InvalidVarIntTag(u8),

    #[error("neo-bin: encoded length {len} exceeds maximum {max}")]
    LengthOutOfRange { len: u64, max: u64 },

    #[error("neo-bin: invalid utf-8 in string field")]
    InvalidUtf8,

    #[error("neo-bin: invalid value for {0}")]
    InvalidValue(&'static str),
}
