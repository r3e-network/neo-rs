// Copyright (C) 2015-2025 The Neo Project.
//
// byte_array_comparer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::cmp::Ordering;

/// Defines methods to support the comparison of two byte arrays.
/// Matches C# ByteArrayComparer class
pub struct ByteArrayComparer {
    direction: i32,
}

impl ByteArrayComparer {
    /// Default comparer
    /// Matches C# Default property
    pub const DEFAULT: ByteArrayComparer = ByteArrayComparer { direction: 1 };

    /// Reverse comparer
    /// Matches C# Reverse property
    pub const REVERSE: ByteArrayComparer = ByteArrayComparer { direction: -1 };

    /// Creates a new ByteArrayComparer
    /// Matches C# constructor
    pub fn new(direction: i32) -> Self {
        Self { direction }
    }

    /// Compares two byte arrays
    /// Matches C# Compare method
    pub fn compare(&self, x: Option<&[u8]>, y: Option<&[u8]>) -> i32 {
        match (x, y) {
            (None, None) => 0,
            (None, Some(y_bytes)) => -(y_bytes.len() as i32) * self.direction,
            (Some(x_bytes), None) => (x_bytes.len() as i32) * self.direction,
            (Some(x_bytes), Some(y_bytes)) => {
                if std::ptr::eq(x_bytes, y_bytes) {
                    return 0;
                }

                match x_bytes.cmp(y_bytes) {
                    Ordering::Equal => 0,
                    Ordering::Less => -self.direction,
                    Ordering::Greater => self.direction,
                }
            }
        }
    }
}

impl PartialEq for ByteArrayComparer {
    fn eq(&self, other: &Self) -> bool {
        self.direction == other.direction
    }
}

impl Eq for ByteArrayComparer {}

impl PartialOrd for ByteArrayComparer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.direction.partial_cmp(&other.direction)
    }
}

impl Ord for ByteArrayComparer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.direction.cmp(&other.direction)
    }
}
