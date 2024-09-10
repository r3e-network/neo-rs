

/// Represents the direction when searching from the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekDirection {
    /// Indicates that the search should be performed in ascending order.
    Forward = 1,

    /// Indicates that the search should be performed in descending order.
    Backward = -1,
}

impl SeekDirection {
    /// Converts the SeekDirection to its corresponding i8 value.
    pub fn to_i8(&self) -> i8 {
        match self {
            SeekDirection::Forward => 1,
            SeekDirection::Backward => -1,
        }
    }

    /// Creates a SeekDirection from an i8 value.
    pub fn from_i8(value: i8) -> Option<Self> {
        match value {
            1 => Some(SeekDirection::Forward),
            -1 => Some(SeekDirection::Backward),
            _ => None,
        }
    }
}
