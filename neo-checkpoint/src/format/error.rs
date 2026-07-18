//! Typed checkpoint format and policy failures.

use thiserror::Error;

/// Result type for checkpoint format and validation operations.
pub type CheckpointResult<T> = Result<T, CheckpointError>;

/// Typed failure vocabulary shared by format, trust, transport, and proof
/// layers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CheckpointError {
    /// A versioned object does not begin with its required format magic.
    #[error("invalid {kind} format magic")]
    InvalidMagic {
        /// Kind of versioned object.
        kind: &'static str,
    },
    /// A versioned object uses an unsupported format.
    #[error("unsupported {kind} format version {actual}; expected {expected}")]
    UnsupportedVersion {
        /// Kind of versioned object.
        kind: &'static str,
        /// Supported version.
        expected: u16,
        /// Encountered version.
        actual: u16,
    },
    /// An untrusted length or count exceeds local policy.
    #[error("{field} is {actual}, exceeding the local maximum {maximum}")]
    LimitExceeded {
        /// Field that exceeded its limit.
        field: &'static str,
        /// Encountered value.
        actual: u64,
        /// Configured maximum.
        maximum: u64,
    },
    /// Checked geometry arithmetic overflowed.
    #[error("{field} arithmetic overflow")]
    ArithmeticOverflow {
        /// Field whose calculation overflowed.
        field: &'static str,
    },
    /// A field violates a canonical or semantic invariant.
    #[error("invalid {field}: {reason}")]
    InvalidField {
        /// Invalid field.
        field: &'static str,
        /// Stable bounded reason.
        reason: &'static str,
    },
    /// A bounded decoder reached the end of an object mid-field.
    #[error("truncated {field}: need {needed} bytes with {remaining} remaining")]
    UnexpectedEof {
        /// Field being decoded.
        field: &'static str,
        /// Bytes required by the field.
        needed: u64,
        /// Bytes remaining in the object.
        remaining: u64,
    },
    /// A canonical object contains bytes after its final field.
    #[error("{kind} contains {trailing} trailing bytes")]
    TrailingBytes {
        /// Kind of versioned object.
        kind: &'static str,
        /// Unconsumed bytes.
        trailing: u64,
    },
    /// A numeric tag does not identify a supported enum variant.
    #[error("invalid {field} tag {value}")]
    InvalidTag {
        /// Tagged field.
        field: &'static str,
        /// Unsupported tag value.
        value: u8,
    },
    /// Computed and declared checkpoint identities differ.
    #[error("checkpoint identity does not match the canonical core")]
    IdentityMismatch,
    /// A checkpoint is below the durable acceptance floor.
    #[error("checkpoint height {offered} is below acceptance floor {minimum}")]
    Rollback {
        /// Offered checkpoint height.
        offered: u32,
        /// Minimum automatically acceptable height.
        minimum: u32,
    },
    /// Two valid identities conflict at one height.
    #[error("conflicting valid checkpoint identities at height {height}")]
    Equivocation {
        /// Conflicting checkpoint height.
        height: u32,
    },
}
