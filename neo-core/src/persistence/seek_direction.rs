//! SeekDirection re-export from neo-storage.
//!
//! The `SeekDirection` enum is now defined in [`neo_storage`] as the single source of truth.
//! This module re-exports it for backward compatibility.

pub use neo_storage::SeekDirection;

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
