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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i8)]
pub enum SeekDirection {
    /// Indicates that the search should be performed in ascending order.
    Forward = 1,

    /// Indicates that the search should be performed in descending order.
    Backward = -1,
}

impl Default for SeekDirection {
    fn default() -> Self {
        Self::Forward
    }
}
