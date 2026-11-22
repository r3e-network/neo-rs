use super::*;

/// Home bucket a key maps to for a fixed bucket count.
fn home_slot(key: &u64, capacity: usize) -> usize {
    fnv1a_hash_u64(hash_key(key)) as usize & (capacity - 1)
}

/// Returns `count` distinct keys whose home bucket is `home`.
fn keys_with_home(home: usize, capacity: usize, count: usize) -> Vec<u64> {
    (0u64..)
        .filter(|candidate| home_slot(candidate, capacity) == home)
        .take(count)
        .collect()
}

/// Returns a key whose home bucket is `home` but that is not in `occupied`.
fn absent_key_with_home(home: usize, capacity: usize, occupied: &[u64]) -> u64 {
    (0u64..)
        .find(|candidate| home_slot(candidate, capacity) == home && !occupied.contains(candidate))
        .expect("a key outside the occupied set always exists")
}

/// Fills a map of `capacity` buckets with distinct keys.
fn full_map(capacity: usize) -> (PooledHashMap<u64, u64>, Vec<u64>) {
    let mut map = PooledHashMap::<u64, u64>::new(capacity);
    let mut keys = Vec::new();
    let mut candidate = 0u64;
    while keys.len() < map.capacity() {
        if map.insert_no_alloc(candidate, candidate).is_none() {
            keys.push(candidate);
        }
        candidate += 1;
    }
    assert_eq!(map.len(), map.capacity());
    (map, keys)
}

#[test]
fn test_pooled_vec_basic() {
    let mut vec = PooledVec::<i32>::with_capacity(10);

    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);

    for i in 0..10 {
        vec.push(i);
    }

    assert_eq!(vec.len(), 10);

    // Verify we can access elements
    for (i, &val) in vec.iter().enumerate() {
        assert_eq!(val, i as i32);
    }
}

#[test]
#[should_panic(expected = "capacity exceeded")]
fn test_pooled_vec_capacity_exceeded() {
    let mut vec = PooledVec::<i32>::with_capacity(5);

    for i in 0..6 {
        vec.push(i);
    }
}

#[test]
fn test_pooled_vec_clear_preserves_capacity() {
    let mut vec = PooledVec::<i32>::with_capacity(100);

    for i in 0..50 {
        vec.push(i);
    }

    let len_before = vec.len();
    vec.clear_keep_capacity();

    assert_eq!(vec.len(), 0);
    // Capacity should remain allocated internally
    assert!(vec.inner.capacity() >= len_before);
}

#[test]
fn test_pooled_map_basic() {
    let mut map = PooledHashMap::<u64, String>::new(64);

    assert!(map.is_empty());

    map.insert_no_alloc(1, "one".to_string());
    map.insert_no_alloc(2, "two".to_string());

    assert_eq!(map.len(), 2);
    assert_eq!(map.get_no_alloc(&1), Some(&"one".to_string()));
    assert_eq!(map.get_no_alloc(&2), Some(&"two".to_string()));
    assert_eq!(map.get_no_alloc(&3), None);
}

#[test]
fn test_pooled_map_collision_handling() {
    // Create a map where hashes collide (same bucket slot)
    let mut map = PooledHashMap::<u64, i32>::new(4);

    // Use keys that hash to same bucket
    map.insert_no_alloc(0x0000_0000_0000_0000, 1);
    map.insert_no_alloc(0x0000_0000_0000_8000, 2); // Likely collides

    assert_eq!(map.len(), 2);
    assert_eq!(map.get_no_alloc(&0x0000_0000_0000_0000), Some(&1));
    assert_eq!(map.get_no_alloc(&0x0000_0000_0000_8000), Some(&2));
}

#[test]
fn test_pooled_map_clear_preserves_buckets() {
    let mut map = PooledHashMap::<u64, u64>::new(256);

    for i in 0..100 {
        map.insert_no_alloc(i, i * 2);
    }

    assert_eq!(map.len(), 100);

    map.clear_no_dealloc();

    assert_eq!(map.len(), 0);
    assert_eq!(map.capacity(), 256);
}

#[test]
fn test_fnv1a_hash_consistency() {
    let hash1 = fnv1a_hash_u64(42);
    let hash2 = fnv1a_hash_u64(42);

    // Same input should always produce same output
    assert_eq!(hash1, hash2);

    // Different inputs should likely produce different outputs
    let hash_different = fnv1a_hash_u64(43);
    assert_ne!(hash1, hash_different);
}

#[test]
fn missing_key_lookup_terminates_when_every_bucket_is_occupied() {
    let (mut map, keys) = full_map(4);
    assert_eq!(map.capacity(), 4);
    assert_eq!(map.load_factor(), 1.0);
    assert_eq!(map.occupied_slots(), vec![0, 1, 2, 3]);

    // Every bucket is taken by another key: the probe must stop after at
    // most `capacity` steps instead of spinning forever.
    for home in 0..map.capacity() {
        let absent = absent_key_with_home(home, map.capacity(), &keys);
        assert_eq!(map.get_no_alloc(&absent), None);
        assert_eq!(map.remove_no_alloc(&absent), None);
    }
    assert_eq!(map.len(), 4);
    assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
}

#[test]
fn deleting_a_collided_entry_keeps_the_rest_of_the_chain_reachable() {
    let mut map = PooledHashMap::<u64, u64>::new(8);
    let chain = keys_with_home(home_slot(&0, 8), 8, 3);
    for key in &chain {
        assert_eq!(map.insert_no_alloc(*key, *key), None);
    }
    assert_eq!(map.len(), 3);
    // The three keys occupy three consecutive buckets: home, home+1, home+2.
    let head = home_slot(&chain[0], 8);
    assert_eq!(
        map.occupied_slots(),
        vec![head, (head + 1) & 7, (head + 2) & 7]
    );

    // Removing the head of the chain must not hide the tail behind it.
    assert_eq!(map.remove_no_alloc(&chain[0]), Some(chain[0]));
    assert_eq!(map.get_no_alloc(&chain[0]), None);
    assert_eq!(map.get_no_alloc(&chain[1]), Some(&chain[1]));
    assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
    assert_eq!(map.len(), 2);
    assert_eq!(map.tombstone_slots(), vec![head]);

    // Removing the new head again leaves the last entry reachable.
    assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
    assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
    assert_eq!(map.len(), 1);

    // The final removal empties the chain, so the trailing tombstones are
    // repaired away instead of lingering.
    assert_eq!(map.remove_no_alloc(&chain[2]), Some(chain[2]));
    assert!(map.is_empty());
    assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
}

#[test]
fn updating_an_existing_key_succeeds_when_every_bucket_is_occupied() {
    let (mut map, keys) = full_map(4);
    assert_eq!(map.load_factor(), 1.0);

    // The existence check runs before the capacity check, so a full table
    // still accepts updates for keys it already holds.
    for key in &keys {
        assert_eq!(map.insert_no_alloc(*key, key + 100), Some(*key));
        assert_eq!(map.get_no_alloc(key), Some(&(key + 100)));
    }
    assert_eq!(map.len(), 4);
}

#[test]
#[should_panic(expected = "PooledHashMap full")]
fn inserting_a_new_key_into_a_full_table_panics() {
    let (mut map, keys) = full_map(4);
    let mut candidate = 1_000_000u64;
    while keys.contains(&candidate) {
        candidate += 1;
    }
    map.insert_no_alloc(candidate, candidate);
}

#[test]
fn repeated_delete_and_reinsert_keeps_every_entry_reachable() {
    let mut map = PooledHashMap::<u64, u64>::new(8);
    let chain = keys_with_home(home_slot(&0, 8), 8, 4);
    for key in &chain {
        map.insert_no_alloc(*key, *key);
    }

    for round in 0..64u64 {
        assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
        assert_eq!(map.len(), 3);
        // The entries behind the removed one stay reachable in every round.
        assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
        assert_eq!(map.get_no_alloc(&chain[3]), Some(&chain[3]));

        // A brand new value proves the reinsert really happened.
        let new_value = chain[1] + round + 1;
        assert_eq!(map.insert_no_alloc(chain[1], new_value), None);
        assert_eq!(map.len(), 4);
        assert_eq!(map.get_no_alloc(&chain[1]), Some(&new_value));
        assert_eq!(map.get_no_alloc(&chain[0]), Some(&chain[0]));
        // The previous value must be gone; restore the stable one.
        assert_ne!(map.get_no_alloc(&chain[1]), Some(&chain[1]));
        map.insert_no_alloc(chain[1], chain[1]);
    }

    assert_eq!(map.len(), 4);
    assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
}

#[test]
fn probing_wraps_around_the_end_of_the_bucket_array() {
    let mut map = PooledHashMap::<u64, u64>::new(4);
    let last = map.capacity() - 1;
    let chain = keys_with_home(last, 4, 3);
    for key in &chain {
        map.insert_no_alloc(*key, *key);
    }

    // The chain starts in the final bucket and overflows into bucket 0.
    assert_eq!(home_slot(&chain[0], 4), last);
    assert_eq!(map.occupied_slots(), vec![0, 1, last]);
    for key in &chain {
        assert_eq!(map.get_no_alloc(key), Some(key));
    }

    // A key homed at the last bucket but absent must still terminate.
    let absent = keys_with_home(last, 4, 4)[3];
    assert_eq!(map.get_no_alloc(&absent), None);

    // Deleting the wrapped entry must keep the slot after the wrap reachable.
    assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
    assert_eq!(map.tombstone_slots(), vec![0]);
    assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
    assert_eq!(map.get_no_alloc(&chain[0]), Some(&chain[0]));

    // Reinsert reuses the tombstone that wrapped around.
    assert_eq!(map.insert_no_alloc(chain[1], chain[1]), None);
    assert_eq!(map.occupied_slots(), vec![0, 1, last]);
    assert_eq!(map.len(), 3);
}
