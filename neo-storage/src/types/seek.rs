use serde::{Deserialize, Serialize};

/// Direction for seeking in storage.
///
/// Matches C# Neo.Persistence.SeekDirection exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(i8)]
pub enum SeekDirection {
    /// Indicates that the search should be performed in ascending order.
    #[default]
    Forward = 1,
    /// Indicates that the search should be performed in descending order.
    Backward = -1,
}

#[cfg(test)]
#[path = "../tests/types/seek.rs"]
mod tests;
