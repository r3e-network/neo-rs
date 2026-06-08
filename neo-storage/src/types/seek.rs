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
mod tests {
    use super::*;

    #[test]
    fn test_seek_direction_default() {
        assert_eq!(SeekDirection::default(), SeekDirection::Forward);
    }

    #[test]
    fn test_seek_direction_variants() {
        assert_ne!(SeekDirection::Forward, SeekDirection::Backward);
    }

    #[test]
    fn test_seek_direction_repr_values() {
        assert_eq!(SeekDirection::Forward as i8, 1);
        assert_eq!(SeekDirection::Backward as i8, -1);
    }

    #[test]
    fn test_seek_direction_clone() {
        let dir1 = SeekDirection::Forward;
        let dir2 = dir1;
        assert_eq!(dir1, dir2);
    }

    #[test]
    fn test_serde_seek_direction() {
        let dir = SeekDirection::Backward;
        let serialized = serde_json::to_string(&dir).unwrap();
        let deserialized: SeekDirection = serde_json::from_str(&serialized).unwrap();
        assert_eq!(dir, deserialized);
    }
}
