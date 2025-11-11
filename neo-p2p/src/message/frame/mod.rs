mod flags;
mod types;

pub use types::Message;

pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32MB
pub(crate) const COMPRESSION_MIN_SIZE: usize = 128;
pub(crate) const COMPRESSION_THRESHOLD: usize = 64;

pub(crate) use flags::MessageFlags;
