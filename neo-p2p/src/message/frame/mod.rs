mod flags;
mod types;

/// Message helpers implement the same framing used by Neo C#: each payload begins
/// with a flags byte, a command byte, and a var-bytes payload length prefix.
pub use types::Message;

pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32MB
pub(crate) const COMPRESSION_MIN_SIZE: usize = 128;
pub(crate) const COMPRESSION_THRESHOLD: usize = 64;

pub(crate) use flags::MessageFlags;
