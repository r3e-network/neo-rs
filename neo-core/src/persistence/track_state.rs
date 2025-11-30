// Copyright (C) 2015-2025 The Neo Project.
//
// track_state.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the state of a cached entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
