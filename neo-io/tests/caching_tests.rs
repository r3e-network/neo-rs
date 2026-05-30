use neo_io::{
    Cache, FIFOCache, HashSetCache, InventoryHash, IoCache, RelayCache,
};
use std::hash::{Hash, Hasher};

#[test]
fn hash_set_cache_respects_capacity() {
    let mut cache = HashSetCache::new(2);
    assert!(cache.try_add(1));
    assert!(cache.try_add(2));
    assert_eq!(cache.count(), 2);

    assert!(cache.try_add(3));
    assert_eq!(cache.count(), 2);
    assert!(cache.contains(&3));
    assert!(cache.contains(&2));
    assert!(!cache.contains(&1));
}

#[test]
fn hash_set_cache_excepts_with_removes_items() {
    let mut cache = HashSetCache::new(3);
    cache.add(1);
    cache.add(2);
    cache.add(3);
    cache.except_with([1, 3]);

    assert_eq!(cache.count(), 1);
    assert!(cache.contains(&2));
    assert!(!cache.contains(&1));
    assert!(!cache.contains(&3));
}

#[test]
fn hash_set_cache_duplicate_does_not_refresh_fifo_order() {
    let mut cache = HashSetCache::new(2);

    assert!(cache.try_add(1));
    assert!(cache.try_add(2));
    assert!(!cache.try_add(1));
    assert!(cache.try_add(3));

    assert_eq!(cache.count(), 2);
    assert!(!cache.contains(&1));
    assert!(cache.contains(&2));
    assert!(cache.contains(&3));
}

#[test]
fn hash_set_cache_capacity_change_trims_on_next_insert_attempt() {
    let mut cache = HashSetCache::new(3);

    assert!(cache.try_add(1));
    assert!(cache.try_add(2));
    assert!(cache.try_add(3));

    cache.set_capacity(2);
    assert!(cache.contains(&1));
    assert!(cache.contains(&2));
    assert!(cache.contains(&3));

    assert!(cache.try_add(4));
    assert!(!cache.contains(&1));
    assert!(!cache.contains(&2));
    assert!(cache.contains(&3));
    assert!(cache.contains(&4));
}

#[test]
fn hash_set_cache_duplicate_insert_after_capacity_reduction_still_trims() {
    let mut cache = HashSetCache::new(3);

    assert!(cache.try_add(1));
    assert!(cache.try_add(2));
    assert!(cache.try_add(3));
    cache.set_capacity(2);
    assert!(!cache.try_add(1));

    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
    assert!(!cache.contains(&1));
}

#[test]
fn hash_set_cache_set_capacity_zero_defers_trim_until_try_add() {
    let mut cache = HashSetCache::new(3);

    cache.add(1);
    cache.add(2);
    cache.add(3);
    cache.set_capacity(0);

    assert_eq!(cache.count(), 3);
    assert!(cache.contains(&1));
    assert!(cache.contains(&2));
    assert!(cache.contains(&3));
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);

    let mut values = [0; 5];
    cache.copy_to(&mut values, 1).unwrap();
    assert_eq!(values, [0, 1, 2, 3, 0]);

    assert!(cache.try_add(4));
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), Vec::<i32>::new());
}

#[test]
fn hash_set_cache_zero_capacity_duplicate_clears_without_reinserting() {
    let mut cache = HashSetCache::new(2);

    cache.add(1);
    cache.add(2);
    cache.set_capacity(0);

    assert!(!cache.try_add(1));
    assert_eq!(cache.count(), 0);
    assert!(!cache.contains(&1));
    assert!(!cache.contains(&2));
}

#[test]
fn hash_set_cache_contains_does_not_refresh_fifo_order() {
    let mut cache = HashSetCache::new(2);

    cache.add(1);
    cache.add(2);
    assert!(cache.contains(&1));
    assert!(cache.try_add(3));

    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
}

#[test]
fn hash_set_cache_except_with_preserves_remaining_fifo_order() {
    let mut cache = HashSetCache::new(5);

    for value in 1..=5 {
        cache.add(value);
    }
    cache.except_with([2, 4]);

    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 3, 5]);
    cache.add(6);
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 3, 5, 6]);
}

#[test]
fn hash_set_cache_into_iter_preserves_fifo_after_mutations() {
    let mut cache = HashSetCache::new(4);

    cache.add(1);
    cache.add(2);
    cache.add(3);
    cache.add(1);
    assert!(cache.remove(&2));
    cache.add(4);
    cache.except_with([3]);
    cache.add(5);

    assert_eq!(cache.into_iter().collect::<Vec<_>>(), vec![1, 4, 5]);
}

#[derive(Debug)]
struct CacheKey {
    id: u8,
    payload: &'static str,
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CacheKey {}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[test]
fn hash_set_cache_duplicate_equal_value_does_not_replace_stored_item() {
    let mut cache = HashSetCache::new(2);

    assert!(cache.try_add(CacheKey {
        id: 7,
        payload: "original",
    }));
    assert!(!cache.try_add(CacheKey {
        id: 7,
        payload: "replacement",
    }));

    assert_eq!(
        cache
            .iter()
            .map(|item| (item.id, item.payload))
            .collect::<Vec<_>>(),
        vec![(7, "original")]
    );
}

#[test]
fn hash_set_cache_zero_capacity_constructor_uses_default_capacity() {
    let mut cache = HashSetCache::new(0);

    for value in 0..1025 {
        assert!(cache.try_add(value));
    }

    assert_eq!(cache.count(), 1024);
    assert!(!cache.contains(&0));
    assert!(cache.contains(&1));
    assert!(cache.contains(&1024));
}

#[test]
fn hash_set_cache_try_new_rejects_zero_capacity() {
    match HashSetCache::<i32>::try_new(0) {
        Err(error) => assert_eq!(error, "capacity must be greater than zero"),
        Ok(_) => panic!("zero-capacity HashSetCache::try_new should fail"),
    }
}

#[test]
fn hash_set_cache_zero_capacity_after_set_keeps_no_items() {
    let mut cache = HashSetCache::new(1);

    cache.set_capacity(0);
    assert!(cache.try_add(1));
    assert_eq!(cache.count(), 0);
    assert!(!cache.contains(&1));
    assert!(cache.try_add(1));
}

#[test]
fn hash_set_cache_capacity_can_recover_after_zero_capacity() {
    let mut cache = HashSetCache::new(1);

    cache.set_capacity(0);
    assert!(cache.try_add(1));
    cache.set_capacity(2);
    assert!(cache.try_add(2));
    assert!(cache.try_add(3));

    assert_eq!(cache.count(), 2);
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
}

#[test]
fn hash_set_cache_copy_to_preserves_insertion_order() {
    let mut cache = HashSetCache::new(3);
    cache.add(1);
    cache.add(2);
    cache.add(3);

    let mut values = [0; 5];
    cache.copy_to(&mut values, 1).unwrap();

    assert_eq!(values, [0, 1, 2, 3, 0]);
}

#[test]
fn hash_set_cache_iter_and_into_iter_preserve_fifo_order() {
    let mut cache = HashSetCache::new(3);
    cache.add(1);
    cache.add(2);
    cache.add(3);

    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(cache.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn hash_set_cache_remove_preserves_fifo_order() {
    let mut cache = HashSetCache::new(4);
    cache.add(1);
    cache.add(2);
    cache.add(3);
    cache.add(4);

    assert!(cache.remove(&2));
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 3, 4]);
    assert!(cache.try_add(5));
    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![1, 3, 4, 5]);
}

#[test]
fn hash_set_cache_clear_preserves_configured_capacity() {
    let mut cache = HashSetCache::new(2);
    cache.add(1);
    cache.add(2);
    cache.clear();

    assert!(cache.try_add(3));
    assert!(cache.try_add(4));
    assert!(cache.try_add(5));

    assert_eq!(cache.iter().copied().collect::<Vec<_>>(), vec![4, 5]);
}

#[test]
fn io_cache_public_alias_and_fifo_cache_preserve_fifo_order() {
    let alias: Cache<i32, i32> = Cache::new(2, |value| *value);
    alias.add(1);
    alias.add(2);
    alias.add(3);

    assert_eq!(alias.values(), vec![2, 3]);
    assert!(!alias.contains_key(&1));

    let concrete = IoCache::new(2, |value: &i32| *value);
    concrete.add(1);
    concrete.add(2);
    concrete.get(&1);
    concrete.add(3);

    assert_eq!(concrete.values(), vec![2, 3]);
    assert!(!concrete.contains_key(&1));

    let fifo = FIFOCache::new(2, |value: &i32| *value);
    fifo.add(1);
    fifo.add(2);
    fifo.add(3);

    assert_eq!(fifo.values(), vec![2, 3]);
    assert!(!fifo.contains_key(&1));
}

#[test]
fn io_cache_zero_capacity_keeps_no_items() {
    let alias: Cache<i32, i32> = Cache::new(0, |value| *value);
    alias.add(1);
    alias.add_range([2, 3]);
    assert!(alias.is_empty());
    assert_eq!(alias.count(), 0);
    assert!(!alias.contains_key(&1));

    let fifo = FIFOCache::new(0, |value: &i32| *value);
    fifo.add(1);
    assert!(fifo.is_empty());
    assert_eq!(fifo.values(), Vec::<i32>::new());
}

#[test]
fn io_cache_get_and_duplicate_add_do_not_refresh_fifo_order() {
    let cache = IoCache::new(2, |value: &i32| *value);
    cache.add(1);
    cache.add(2);
    assert_eq!(cache.get(&1), Some(1));
    cache.add(1);
    cache.add(3);

    assert_eq!(cache.values(), vec![2, 3]);
    assert!(!cache.contains_key(&1));
}

#[test]
fn io_cache_contains_and_try_get_do_not_refresh_fifo_order() {
    let cache = IoCache::new(2, |value: &i32| *value);
    cache.add(1);
    cache.add(2);

    assert!(cache.contains_key(&1));
    assert_eq!(cache.try_get(&1), Some(1));
    cache.add(3);

    assert_eq!(cache.values(), vec![2, 3]);
    assert!(!cache.contains_key(&1));
}

#[test]
fn io_cache_copy_to_after_eviction_preserves_fifo_order() {
    let cache = IoCache::new(3, |value: &i32| *value);
    cache.add(1);
    cache.add(2);
    cache.add(3);
    cache.add(4);

    let mut values = [0; 5];
    cache.copy_to(&mut values, 1).unwrap();

    assert_eq!(values, [0, 2, 3, 4, 0]);
}

#[test]
fn io_cache_remove_preserves_fifo_order_and_next_eviction() {
    let cache = IoCache::new(3, |value: &i32| *value);
    cache.add(1);
    cache.add(2);
    cache.add(3);

    assert!(cache.remove_key(&2));
    assert_eq!(cache.values(), vec![1, 3]);

    cache.add(4);
    cache.add(5);

    assert_eq!(cache.values(), vec![3, 4, 5]);
    assert!(!cache.contains_key(&1));
}

#[test]
fn fifo_cache_access_paths_do_not_refresh_fifo_order() {
    let cache = FIFOCache::new(2, |value: &i32| *value);
    cache.add(1);
    cache.add(2);

    assert_eq!(cache.get(&1), Some(1));
    assert!(cache.contains_key(&1));
    assert!(cache.contains(&1));
    cache.add(1);
    cache.add(3);

    assert_eq!(cache.values(), vec![2, 3]);
    assert!(!cache.contains_key(&1));
}

#[derive(Clone, Debug)]
struct MockInventory {
    hash: Vec<u8>,
    _payload: &'static str,
}

impl InventoryHash<Vec<u8>> for MockInventory {
    fn inventory_hash(&self) -> &Vec<u8> {
        &self.hash
    }
}

#[test]
fn relay_cache_keys_on_inventory_hash() {
    let cache: RelayCache<Vec<u8>, MockInventory> = RelayCache::new(2);
    cache.add(MockInventory {
        hash: vec![1],
        _payload: "first",
    });
    cache.add(MockInventory {
        hash: vec![2],
        _payload: "second",
    });

    assert!(cache.contains_key(&vec![1]));
    assert!(cache.contains_key(&vec![2]));

    cache.add(MockInventory {
        hash: vec![3],
        _payload: "third",
    });

    assert!(!cache.contains_key(&vec![1]));
    assert!(cache.contains_key(&vec![2]));
    assert!(cache.contains_key(&vec![3]));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VersionedValue {
    key: i32,
    payload: &'static str,
}

#[test]
fn io_cache_duplicate_add_preserves_original_value() {
    let cache = IoCache::new(2, |value: &VersionedValue| value.key);
    let original = VersionedValue {
        key: 1,
        payload: "original",
    };
    let replacement = VersionedValue {
        key: 1,
        payload: "replacement",
    };

    cache.add(original.clone());
    cache.add(replacement);

    assert_eq!(cache.get(&1), Some(original.clone()));
    assert_eq!(cache.values(), vec![original]);
}
