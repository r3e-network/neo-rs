use super::*;

#[test]
fn test_storage_key_creation() {
    let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02, 0x03]);
}

#[test]
fn test_storage_key_create() {
    let key = StorageKey::create(-4, 0x05);
    assert_eq!(key.id(), -4);
    assert_eq!(key.key(), &[0x05]);
}

#[test]
fn test_storage_key_create_with_byte() {
    let key = StorageKey::create_with_byte(-1, 0x10, 0x42);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x10, 0x42]);
}

#[test]
fn test_storage_key_create_with_uint160() {
    let hash = UInt160::zero();
    let key = StorageKey::create_with_uint160(-1, 0x14, &hash);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 21);
    assert_eq!(key.key()[0], 0x14);
}

#[test]
fn test_storage_key_create_with_uint256() {
    let hash = UInt256::zero();
    let key = StorageKey::create_with_uint256(-2, 0x15, &hash);
    assert_eq!(key.id(), -2);
    assert_eq!(key.key().len(), 33);
    assert_eq!(key.key()[0], 0x15);
}

#[test]
fn test_storage_key_create_with_int32() {
    let key = StorageKey::create_with_int32(-1, 0x20, 0x12345678);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 5);
    assert_eq!(key.key()[0], 0x20);
    assert_eq!(&key.key()[1..], &[0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn test_storage_key_create_with_int64() {
    let key = StorageKey::create_with_int64(-1, 0x21, 0x123456789ABCDEF0u64 as i64);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 9);
    assert_eq!(key.key()[0], 0x21);
}

#[test]
fn test_storage_key_create_with_bytes() {
    let content = vec![0xAA, 0xBB, 0xCC];
    let key = StorageKey::create_with_bytes(-1, 0x30, &content);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x30, 0xAA, 0xBB, 0xCC]);
}

#[test]
fn test_storage_key_create_search_prefix() {
    let prefix = StorageKey::create_search_prefix(-1, &[0x14]);
    assert_eq!(prefix.len(), 5);
    assert_eq!(&prefix[..4], &(-1i32).to_le_bytes());
    assert_eq!(prefix[4], 0x14);
}

#[test]
fn test_storage_key_ordering_matches_serialized_bytes() {
    let key1 = StorageKey::new(-1, vec![0x01]);
    let key2 = StorageKey::new(-1, vec![0x02]);
    let key3 = StorageKey::new(0, vec![0x01]);

    assert!(key1 < key2);
    assert!(
        key3 < key1,
        "C# DataCache orders StorageKey.ToArray() with ByteArrayComparer, so little-endian id bytes drive cross-contract ordering"
    );
}

#[test]
fn test_storage_key_ordering_same_id() {
    let key1 = StorageKey::new(5, vec![0x01]);
    let key2 = StorageKey::new(5, vec![0x02]);
    let key3 = StorageKey::new(5, vec![0x01]);

    assert!(key1 < key2);
    assert_eq!(key1, key3);
    assert!(key2 > key1);
}

#[test]
fn test_storage_key_ordering_different_id() {
    let key1 = StorageKey::new(-5, vec![0xFF]);
    let key2 = StorageKey::new(10, vec![0x00]);

    assert!(key2 < key1);
}

#[test]
fn storage_key_ord_matches_csharp_v310_byte_array_comparer() {
    let mut keys = [
        StorageKey::new(-5, vec![0x01]),
        StorageKey::new(10, vec![0x01]),
        StorageKey::new(0, vec![0xFF]),
        StorageKey::new(-1, vec![0x00]),
    ];

    let mut expected: Vec<_> = keys.iter().map(StorageKey::to_array).collect();
    expected.sort();

    keys.sort();

    assert_eq!(
        keys.iter().map(StorageKey::to_array).collect::<Vec<_>>(),
        expected,
        "C# v3.10 DataCache.Seek orders p.Key.ToArray() using ByteArrayComparer.SequenceCompareTo"
    );
}

#[test]
fn test_storage_key_to_array() {
    let key = StorageKey::new(-1, vec![0xAA, 0xBB]);
    let array = key.to_array();
    assert_eq!(&array[..4], &(-1i32).to_le_bytes());
    assert_eq!(&array[4..], &[0xAA, 0xBB]);
}

#[test]
fn test_storage_key_from_bytes() {
    let bytes = vec![0x01, 0x02, 0x03, 0x04, 0xAA, 0xBB];
    let key = StorageKey::from_bytes(&bytes);
    let expected_id = i32::from_le_bytes([0x01, 0x02, 0x03, 0x04]);
    assert_eq!(key.id(), expected_id);
    assert_eq!(key.key(), &[0xAA, 0xBB]);
}

#[test]
fn test_storage_key_equality_and_hash_ignore_cached_bytes() {
    use std::collections::HashSet;

    let constructed = StorageKey::new(-1, vec![0xAA, 0xBB]);
    let roundtrip = StorageKey::from_bytes(&constructed.to_array());

    assert_eq!(constructed, roundtrip);

    let mut keys = HashSet::new();
    keys.insert(constructed);
    assert!(keys.contains(&roundtrip));
}

#[test]
fn test_storage_key_suffix() {
    let key = StorageKey::new(-1, vec![0x01, 0x02]);
    assert_eq!(key.suffix(), key.key());
}

#[test]
fn test_storage_key_length() {
    let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
    // C# `StorageKey.Length` returns `Build().Length`, and `Build()` allocates
    // `sizeof(int) + Key.Length`. The suffix carries its own prefix byte, so a
    // 3-byte suffix is 4 + 3 = 7, not `PREFIX_LENGTH + 3`.
    assert_eq!(key.length(), 7);
    assert_eq!(key.length(), key.as_bytes().len());
}

#[test]
fn test_storage_key_clone() {
    let key1 = StorageKey::new(-1, vec![0x01, 0x02]);
    let key2 = key1.clone();
    assert_eq!(key1, key2);
}

#[test]
fn test_storage_key_hash_set() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    let key1 = StorageKey::new(-1, vec![0x01]);
    let key2 = StorageKey::new(-1, vec![0x01]);
    let key3 = StorageKey::new(-1, vec![0x02]);

    set.insert(key1.clone());
    assert!(set.contains(&key2));
    assert!(!set.contains(&key3));
}

#[test]
fn test_storage_key_get_hash_code() {
    let key = StorageKey::new(-1, vec![0x14, 0xAA, 0xBB]);
    let hash1 = key.hash_code();
    let hash2 = key.hash_code();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_storage_key_display_empty() {
    let key = StorageKey::new(-1, vec![]);
    let display = format!("{}", key);
    assert_eq!(display, "StorageKey{Id=-1}");
}

#[test]
fn test_storage_key_display_with_prefix() {
    let key = StorageKey::new(-1, vec![0x14, 0xAA, 0xBB]);
    let display = format!("{}", key);
    assert_eq!(display, "StorageKey{Id=-1,Key=14aabb}");
}

#[test]
fn storage_key_display_matches_csharp_v3101_to_string() {
    assert_eq!(
        StorageKey::new(0, vec![0x12]).to_string(),
        "StorageKey{Id=0,Key=12}"
    );
    assert_eq!(
        StorageKey::new(0, Vec::new()).to_string(),
        "StorageKey{Id=0}"
    );
}

#[test]
fn test_storage_key_debug() {
    let key = StorageKey::new(-1, vec![0x01]);
    let debug_str = format!("{:?}", key);
    assert!(debug_str.contains("StorageKey"));
}

#[test]
fn test_storage_key_from_vec() {
    let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02];
    let key: StorageKey = bytes.into();
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02]);
}

#[test]
fn test_storage_key_from_slice() {
    let bytes: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02];
    let key: StorageKey = bytes.into();
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02]);
}

#[test]
fn test_serde_storage_key() {
    let key = StorageKey::new(-1, vec![0x01, 0x02]);
    let serialized = serde_json::to_string(&key).unwrap();
    let deserialized: StorageKey = serde_json::from_str(&serialized).unwrap();
    assert_eq!(key.id, deserialized.id);
    assert_eq!(key.key, deserialized.key);
}

/// `Ord` is allocation-free but must stay byte-identical to comparing the
/// materialized full keys, which is what C# `DataCache.Seek` does via
/// `ByteArrayComparer.SequenceCompareTo` over `p.Key.ToArray()`.
///
/// The corpus deliberately mixes both construction paths so cached keys
/// (`from_bytes`, taken by MDBX/`StoreCache` reads) are compared against
/// uncached keys (`new`/`create_*`, produced by `Storage.Put` and native
/// writes). `change_set` holds both shapes in one `BTreeSet`.
#[test]
fn storage_key_ord_is_byte_identical_to_materialized_compare() {
    fn materialized_cmp(a: &StorageKey, b: &StorageKey) -> std::cmp::Ordering {
        a.as_bytes().as_ref().cmp(b.as_bytes().as_ref())
    }

    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    let ids = [0, 1, 2, 77, 255, 256, -1, -2, -4, i32::MAX, i32::MIN];
    let mut keys = Vec::new();
    for id in ids {
        for len in 0_usize..10 {
            let suffix: Vec<u8> = (0..len)
                .map(|i| ((next() >> (i % 8)) & 0xFF) as u8)
                .collect();
            // Uncached: the write-path shape.
            keys.push(StorageKey::new(id, suffix.clone()));
            keys.push(StorageKey::create_with_bytes(id, 0x0B, &suffix));
            // Cached: the read-path shape (`from_bytes` populates `cache`).
            let mut raw = id.to_le_bytes().to_vec();
            raw.extend_from_slice(&suffix);
            keys.push(StorageKey::from_bytes(&raw));
        }
    }

    for left in &keys {
        for right in &keys {
            assert_eq!(
                left.cmp(right),
                materialized_cmp(left, right),
                "ordering diverged for {:?} vs {:?}",
                left.as_bytes(),
                right.as_bytes()
            );
        }
    }

    let mut fast = keys.clone();
    fast.sort();
    let mut materialized = keys;
    materialized.sort_by(materialized_cmp);
    assert_eq!(
        fast.iter().map(StorageKey::to_array).collect::<Vec<_>>(),
        materialized
            .iter()
            .map(StorageKey::to_array)
            .collect::<Vec<_>>(),
        "sorted change-set order must not depend on which keys cache full bytes"
    );
}

/// `length()` must equal the materialized key length regardless of whether the
/// key caches its full bytes, matching C# `StorageKey.Length` (which always
/// builds `id ++ key`). The suffix already contains any leading prefix byte.
#[test]
fn storage_key_length_matches_materialized_bytes() {
    let cases = [
        StorageKey::new(0, vec![]),
        StorageKey::new(-1, vec![0x01, 0x02]),
        StorageKey::create(77, 0x0B),
        StorageKey::create_with_byte(77, 0x0B, 0x01),
        StorageKey::create_with_uint160(-5, 0x14, &neo_primitives::UInt160::zero()),
        StorageKey::from_bytes(&[77, 0, 0, 0, 0x0B, 0x01]),
    ];

    for key in &cases {
        assert_eq!(
            key.length(),
            key.as_bytes().len(),
            "length() diverged from materialized bytes for {key}"
        );
        assert_eq!(
            key.length(),
            StorageKey::new(key.id(), key.key().to_vec()).length(),
            "length() must not depend on whether full bytes are cached for {key}"
        );
    }
}

/// `Ord` must agree with `Eq`: whenever two keys are equal they must compare
/// `Equal`, or a `BTreeSet` can silently lose or duplicate entries.
///
/// `from_bytes` accepts fewer than four bytes and then caches the input verbatim
/// with no contract-ID prefix, so a comparator that ordered by the cached bytes
/// disagreed with `Eq` for those keys. Ordering by `(id, key)` — the same fields
/// `Eq` and `Hash` use — makes the contract structural.
#[test]
fn storage_key_ord_agrees_with_eq_for_short_from_bytes_keys() {
    for raw in [
        [].as_slice(),
        [0x01].as_slice(),
        [0x01, 0x02].as_slice(),
        [0x01, 0x02, 0x03].as_slice(),
    ] {
        let from_raw = StorageKey::from_bytes(raw);
        let constructed = StorageKey::new(0, raw.to_vec());
        assert_eq!(
            from_raw, constructed,
            "from_bytes({raw:?}) and new(0, {raw:?}) must be equal"
        );
        assert_eq!(
            from_raw.cmp(&constructed),
            std::cmp::Ordering::Equal,
            "equal keys must compare Equal for raw {raw:?}"
        );
        assert_eq!(constructed.cmp(&from_raw), std::cmp::Ordering::Equal);

        // The Eq/Ord agreement must hold through a BTreeSet, which is where the
        // inconsistency would actually corrupt the change set.
        let mut set = std::collections::BTreeSet::new();
        set.insert(from_raw.clone());
        set.insert(constructed.clone());
        assert_eq!(set.len(), 1, "equal keys must dedupe in a BTreeSet");
        assert!(set.contains(&from_raw));
        assert!(set.contains(&constructed));
    }
}

/// `Ord` must be antisymmetric and transitive for `BTreeSet`/`BTreeMap` to
/// behave, since `change_set` ordering feeds MPT insertion and therefore the
/// state root.
#[test]
fn storage_key_ord_is_a_total_order() {
    let keys = [
        StorageKey::new(0, vec![]),
        StorageKey::new(0, vec![0x00]),
        StorageKey::new(0, vec![0xFF]),
        StorageKey::new(1, vec![0x00]),
        StorageKey::new(-1, vec![0x00]),
        StorageKey::new(i32::MIN, vec![0x01]),
        StorageKey::new(i32::MAX, vec![0x01]),
        StorageKey::create_with_byte(77, 0x0B, 0x01),
        StorageKey::from_bytes(&[77, 0, 0, 0, 0x0B, 0x01]),
    ];

    for left in &keys {
        for right in &keys {
            assert_eq!(
                left.cmp(right).reverse(),
                right.cmp(left),
                "antisymmetry violated"
            );
            if left == right {
                assert_eq!(
                    left.cmp(right),
                    std::cmp::Ordering::Equal,
                    "equal keys must compare Equal"
                );
            }
            for mid in &keys {
                if left.cmp(mid) == std::cmp::Ordering::Less
                    && mid.cmp(right) == std::cmp::Ordering::Less
                {
                    assert_eq!(
                        left.cmp(right),
                        std::cmp::Ordering::Less,
                        "transitivity violated"
                    );
                }
            }
        }
    }
}
