
/// Represents the state of a cached entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackState {
    /// Indicates that the entry has been loaded from the underlying storage, but has not been modified.
    None,

    /// Indicates that this is a newly added record.
    Added,

    /// Indicates that the entry has been loaded from the underlying storage, and has been modified.
    Changed,

    /// Indicates that the entry should be deleted from the underlying storage when committing.
    Deleted,

    /// Indicates that the entry was not found in the underlying storage.
    NotFound,
}

impl From<TrackState> for u8 {
    fn from(state: TrackState) -> Self {
        match state {
            TrackState::None => 0,
            TrackState::Added => 1,
            TrackState::Changed => 2,
            TrackState::Deleted => 3,
            TrackState::NotFound => 4,
        }
    }
}

impl TryFrom<u8> for TrackState {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TrackState::None),
            1 => Ok(TrackState::Added),
            2 => Ok(TrackState::Changed),
            3 => Ok(TrackState::Deleted),
            4 => Ok(TrackState::NotFound),
            _ => Err("Invalid TrackState value"),
        }
    }
}
