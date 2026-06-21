use super::*;

#[test]
fn test_fifo_cache_basic_operations() {
    let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);

    cache.add(1);
    cache.add(2);
    cache.add(3);

    assert_eq!(cache.count(), 3);
    assert!(cache.contains_key(&1));
    assert!(cache.contains_key(&2));
    assert!(cache.contains_key(&3));

    // Adding a 4th item should evict the oldest (1)
    cache.add(4);
    assert_eq!(cache.count(), 3);
    assert!(!cache.contains_key(&1));
    assert!(cache.contains_key(&4));
}

#[test]
fn test_copy_to_success() {
    let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
    cache.add(1);
    cache.add(2);

    let mut dest = vec![0; 5];
    assert!(cache.copy_to(&mut dest, 1).is_ok());
    assert_eq!(dest[1], 1);
    assert_eq!(dest[2], 2);
}

#[test]
fn test_copy_to_bounds_error() {
    let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
    cache.add(1);
    cache.add(2);

    let mut dest = vec![0; 2];
    assert!(cache.copy_to(&mut dest, 1).is_err());
}

#[test]
fn test_clear() {
    let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
    cache.add(1);
    cache.add(2);

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.count(), 0);
}

#[test]
fn test_remove() {
    let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
    cache.add(1);
    cache.add(2);

    assert!(cache.remove_key(&1));
    assert!(!cache.contains_key(&1));
    assert!(!cache.remove_key(&1)); // Already removed
}
