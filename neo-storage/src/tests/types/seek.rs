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
