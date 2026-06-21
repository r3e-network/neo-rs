use serde::{Deserialize, Serialize};

/// Track state for cached storage items.
///
/// Matches C# Neo.Persistence.TrackState exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum TrackState {
    /// Indicates that the entry has been loaded from the underlying storage, but has not been modified.
    #[default]
    None = 0,
    /// Indicates that this is a newly added record.
    Added = 1,
    /// Indicates that the entry has been loaded from the underlying storage, and has been modified.
    Changed = 2,
    /// Indicates that the entry should be deleted from the underlying storage when committing.
    Deleted = 3,
    /// Indicates that the entry was not found in the underlying storage.
    NotFound = 4,
}

#[cfg(test)]
#[path = "../tests/types/track.rs"]
mod tests;
