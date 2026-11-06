//! Owned errors for general serialization and compression codecs.

use thiserror::Error;

/// A codec failure with its original operation-specific message.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Encoding or encoded-size estimation failed.
    #[error("{message}")]
    Serialization {
        /// The original encoding failure message.
        message: String,
    },
    /// Decoding serialized data failed.
    #[error("{message}")]
    Deserialization {
        /// The original decoding failure message.
        message: String,
    },
    /// The length-prefixed LZ4 compression codec failed.
    #[error("{message}")]
    Compression {
        /// The original compression failure message.
        message: String,
    },
    /// A compression stream reported an I/O failure.
    #[error("{message}")]
    Io {
        /// The original stream failure message.
        message: String,
    },
}

/// Result alias for general codec operations.
pub type CodecResult<T> = std::result::Result<T, CodecError>;

impl CodecError {
    /// Creates an encoding failure without changing its message.
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Creates a decoding failure without changing its message.
    pub fn deserialization<S: Into<String>>(message: S) -> Self {
        Self::Deserialization {
            message: message.into(),
        }
    }

    /// Creates a compression failure without changing its message.
    pub fn compression<S: Into<String>>(message: S) -> Self {
        Self::Compression {
            message: message.into(),
        }
    }

    /// Creates a stream I/O failure without changing its message.
    pub fn io<S: Into<String>>(message: S) -> Self {
        Self::Io {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_errors_preserve_owned_messages_and_variants() {
        let errors = [
            CodecError::serialization("encode"),
            CodecError::deserialization("decode"),
            CodecError::compression("lz4"),
            CodecError::io("stream"),
        ];
        for (error, message) in errors.iter().zip(["encode", "decode", "lz4", "stream"]) {
            assert_eq!(error.to_string(), message);
            assert_eq!(error.clone(), *error);
        }
        assert!(matches!(errors[0], CodecError::Serialization { .. }));
        assert!(matches!(errors[1], CodecError::Deserialization { .. }));
        assert!(matches!(errors[2], CodecError::Compression { .. }));
        assert!(matches!(errors[3], CodecError::Io { .. }));
    }
}
