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
mod tests {
    use super::*;

    #[test]
    fn test_track_state_default() {
        assert_eq!(TrackState::default(), TrackState::None);
    }

    #[test]
    fn test_track_state_variants() {
        let states = [
            TrackState::None,
            TrackState::Added,
            TrackState::Changed,
            TrackState::Deleted,
            TrackState::NotFound,
        ];

        for (i, state1) in states.iter().enumerate() {
            for (j, state2) in states.iter().enumerate() {
                if i == j {
                    assert_eq!(state1, state2);
                } else {
                    assert_ne!(state1, state2);
                }
            }
        }
    }

    #[test]
    fn test_track_state_repr_values() {
        assert_eq!(TrackState::None as u8, 0);
        assert_eq!(TrackState::Added as u8, 1);
        assert_eq!(TrackState::Changed as u8, 2);
        assert_eq!(TrackState::Deleted as u8, 3);
        assert_eq!(TrackState::NotFound as u8, 4);
    }

    #[test]
    fn test_track_state_clone() {
        let state1 = TrackState::Changed;
        let state2 = state1;
        assert_eq!(state1, state2);
    }

    #[test]
    fn test_serde_track_state() {
        let state = TrackState::Changed;
        let serialized = serde_json::to_string(&state).unwrap();
        let deserialized: TrackState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }
}
