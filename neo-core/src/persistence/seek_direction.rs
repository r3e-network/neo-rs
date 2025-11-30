// Copyright (C) 2015-2025 The Neo Project.
//
// seek_direction.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the direction when searching from the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(i8)]
pub enum SeekDirection {
    /// Indicates that the search should be performed in ascending order.
    #[default]
    Forward = 1,

    /// Indicates that the search should be performed in descending order.
    Backward = -1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_direction_default_is_forward() {
        assert_eq!(SeekDirection::default(), SeekDirection::Forward);
    }

    #[test]
    fn seek_direction_equality() {
        assert_eq!(SeekDirection::Forward, SeekDirection::Forward);
        assert_eq!(SeekDirection::Backward, SeekDirection::Backward);
        assert_ne!(SeekDirection::Forward, SeekDirection::Backward);
    }

    #[test]
    fn seek_direction_repr_values() {
        assert_eq!(SeekDirection::Forward as i8, 1);
        assert_eq!(SeekDirection::Backward as i8, -1);
    }

    #[test]
    fn seek_direction_clone() {
        let dir = SeekDirection::Backward;
        let cloned = dir;
        assert_eq!(dir, cloned);
    }
}
