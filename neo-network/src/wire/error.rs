//! Wire-protocol error type.

use thiserror::Error;

/// Result alias for the wire layer.
pub type WireResult<T> = std::result::Result<T, WireError>;

/// Errors raised while encoding or decoding a P2P [`crate::wire::Message`].
#[derive(Debug, Error)]
pub enum WireError {
    /// The message payload was longer than the configured maximum
    /// (matches C# `Neo.Network.P2P.Message.PayloadMaxSize`).
    #[error("payload too large: {0} bytes (max {1})")]
    PayloadTooLarge(usize, usize),

    /// The message payload could not be (de)serialised.
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// The wire data could not be compressed or decompressed.
    #[error("compression error: {0}")]
    Compression(String),

    /// An I/O error occurred while reading or writing the wire data.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// An underlying neo-io error occurred while serialising a payload.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<neo_io::IoError> for WireError {
    fn from(err: neo_io::IoError) -> Self {
        Self::Serialization(err.to_string())
    }
}

neo_error::impl_error_from_struct!(neo_error::CoreError, WireError => Network);
