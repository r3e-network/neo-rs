//! TrackState re-export from neo-storage.
//!
//! The `TrackState` enum is now defined in [`neo_storage`] as the single source of truth.
//! This module re-exports it for backward compatibility.

pub use neo_storage::TrackState;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_state_default_is_none() {
        assert_eq!(TrackState::default(), TrackState::None);
    }

    #[test]
    fn track_state_equality() {
        assert_eq!(TrackState::None, TrackState::None);
        assert_eq!(TrackState::Added, TrackState::Added);
        assert_eq!(TrackState::Changed, TrackState::Changed);
        assert_eq!(TrackState::Deleted, TrackState::Deleted);
        assert_eq!(TrackState::NotFound, TrackState::NotFound);
        assert_ne!(TrackState::None, TrackState::Added);
    }

    #[test]
    fn track_state_repr_values() {
        assert_eq!(TrackState::None as u8, 0);
        assert_eq!(TrackState::Added as u8, 1);
        assert_eq!(TrackState::Changed as u8, 2);
        assert_eq!(TrackState::Deleted as u8, 3);
        assert_eq!(TrackState::NotFound as u8, 4);
    }

    #[test]
    fn track_state_clone() {
        let state = TrackState::Changed;
        let cloned = state;
        assert_eq!(state, cloned);
    }
}
