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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TrackState {
    /// Indicates that the entry has been loaded from the underlying storage, but has not been modified.
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

impl Default for TrackState {
    fn default() -> Self {
        Self::None
    }
}
