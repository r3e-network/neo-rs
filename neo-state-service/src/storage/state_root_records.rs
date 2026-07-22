//! Strict codecs and coherent reads for persisted StateService root records.
//!
//! ## Boundary
//!
//! This module owns the byte-level contract between StateService root metadata
//! and storage. Consumers receive a validated [`StateRoot`] and never inspect
//! offsets in the persisted representation.
//!
//! ## Contents
//!
//! - Strict current-index and full state-root record decoders.
//! - One-snapshot lookup of the current local StateService root.

use crate::{CURRENT_VERSION, Keys, StateRoot};
use neo_io::{IoError, MemoryReader};
use neo_storage::StorageError;
use neo_storage::persistence::{RawReadOnlyStore, Store};
use std::error::Error;
use std::fmt;

/// Failure while decoding or coherently reading persisted StateService roots.
#[derive(Debug)]
pub enum StateRootRecordError {
    /// Reading the current-local-root pointer failed.
    ReadCurrentIndex {
        /// Underlying storage failure.
        source: StorageError,
    },
    /// The current-local-root pointer is absent.
    MissingCurrentIndex,
    /// The current-local-root pointer is not exactly one little-endian `u32`.
    InvalidCurrentIndexLength {
        /// Observed byte length.
        actual: usize,
    },
    /// Reading the state-root record selected by the current pointer failed.
    ReadRoot {
        /// Block index selected by the current pointer.
        index: u32,
        /// Underlying storage failure.
        source: StorageError,
    },
    /// The current pointer names a root record that is absent.
    MissingRoot {
        /// Missing state-root index.
        index: u32,
    },
    /// Full Neo state-root decoding failed.
    Decode {
        /// Neo binary codec failure.
        source: IoError,
    },
    /// Bytes remain after one complete state-root record.
    TrailingBytes {
        /// Bytes not consumed by [`StateRoot::deserialize`].
        trailing: usize,
    },
    /// The persisted root uses a StateService version this binary cannot apply.
    UnsupportedVersion {
        /// Persisted record version.
        actual: u8,
        /// Version supported by this binary.
        expected: u8,
    },
    /// The decoded record index differs from the key/current pointer.
    IndexMismatch {
        /// Index selected by the key/current pointer.
        expected: u32,
        /// Index encoded inside the record.
        actual: u32,
    },
}

impl fmt::Display for StateRootRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadCurrentIndex { .. } => {
                formatter.write_str("failed to read the StateService current local root index")
            }
            Self::MissingCurrentIndex => {
                formatter.write_str("StateService current local root index is absent")
            }
            Self::InvalidCurrentIndexLength { actual } => write!(
                formatter,
                "StateService current local root index has {actual} bytes, expected 4"
            ),
            Self::ReadRoot { index, .. } => {
                write!(formatter, "failed to read StateService root record {index}")
            }
            Self::MissingRoot { index } => write!(
                formatter,
                "StateService current local root pointer {index} is dangling"
            ),
            Self::Decode { .. } => formatter.write_str("StateService root record is malformed"),
            Self::TrailingBytes { trailing } => write!(
                formatter,
                "StateService root record has {trailing} trailing bytes"
            ),
            Self::UnsupportedVersion { actual, expected } => write!(
                formatter,
                "StateService root record version {actual:#04x} is unsupported; expected {expected:#04x}"
            ),
            Self::IndexMismatch { expected, actual } => write!(
                formatter,
                "StateService root record index {actual} differs from expected index {expected}"
            ),
        }
    }
}

impl Error for StateRootRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadCurrentIndex { source } | Self::ReadRoot { source, .. } => Some(source),
            Self::Decode { source } => Some(source),
            Self::MissingCurrentIndex
            | Self::InvalidCurrentIndexLength { .. }
            | Self::MissingRoot { .. }
            | Self::TrailingBytes { .. }
            | Self::UnsupportedVersion { .. }
            | Self::IndexMismatch { .. } => None,
        }
    }
}

/// Decodes the exact four-byte little-endian current-local-root pointer.
pub fn decode_current_local_root_index(bytes: &[u8]) -> Result<u32, StateRootRecordError> {
    let bytes: [u8; size_of::<u32>()] =
        bytes
            .try_into()
            .map_err(|_| StateRootRecordError::InvalidCurrentIndexLength {
                actual: bytes.len(),
            })?;
    Ok(u32::from_le_bytes(bytes))
}

/// Decodes exactly one current-version Neo StateService root record.
pub fn decode_state_root_record(bytes: &[u8]) -> Result<StateRoot, StateRootRecordError> {
    let mut reader = MemoryReader::new(bytes);
    let root = StateRoot::deserialize(&mut reader)
        .map_err(|source| StateRootRecordError::Decode { source })?;
    if reader.remaining() != 0 {
        return Err(StateRootRecordError::TrailingBytes {
            trailing: reader.remaining(),
        });
    }
    if root.version() != CURRENT_VERSION {
        return Err(StateRootRecordError::UnsupportedVersion {
            actual: root.version(),
            expected: CURRENT_VERSION,
        });
    }
    Ok(root)
}

/// Decodes a local root record and binds its embedded index to its storage key.
pub fn decode_local_state_root_record(
    expected_index: u32,
    bytes: &[u8],
) -> Result<StateRoot, StateRootRecordError> {
    let root = decode_state_root_record(bytes)?;
    if root.index() != expected_index {
        return Err(StateRootRecordError::IndexMismatch {
            expected: expected_index,
            actual: root.index(),
        });
    }
    Ok(root)
}

/// Reads the current local StateService root through one coherent store snapshot.
///
/// The pointer and selected record are read from the same frozen generation, so
/// a concurrent canonical commit cannot produce a mixed `(index, root)` pair.
pub fn read_current_local_root<S>(store: &S) -> Result<StateRoot, StateRootRecordError>
where
    S: Store,
{
    let snapshot = store.snapshot();
    read_current_local_root_from(snapshot.as_ref())
}

/// Reads the current local root from an already-frozen StateService read view.
pub fn read_current_local_root_from<R>(snapshot: &R) -> Result<StateRoot, StateRootRecordError>
where
    R: RawReadOnlyStore + ?Sized,
{
    let index_bytes = snapshot
        .try_get_bytes_result(Keys::CURRENT_LOCAL_ROOT_INDEX)
        .map_err(|source| StateRootRecordError::ReadCurrentIndex { source })?
        .ok_or(StateRootRecordError::MissingCurrentIndex)?;
    let index = decode_current_local_root_index(&index_bytes)?;
    read_local_state_root(snapshot, index)
}

/// Reads and strictly decodes one local StateService root from a frozen view.
pub fn read_local_state_root<R>(snapshot: &R, index: u32) -> Result<StateRoot, StateRootRecordError>
where
    R: RawReadOnlyStore + ?Sized,
{
    let root_bytes = snapshot
        .try_get_bytes_result(&Keys::state_root(index))
        .map_err(|source| StateRootRecordError::ReadRoot { index, source })?
        .ok_or(StateRootRecordError::MissingRoot { index })?;
    decode_local_state_root_record(index, &root_bytes)
}

#[cfg(test)]
#[path = "../tests/storage/state_root_records.rs"]
mod tests;
