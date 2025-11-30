use neo_io::{
    ECDsaCache, ECDsaCacheItem, ECPointCache, EncodablePoint, HashSetCache, InventoryHash,
    KeyedCollectionSlim, LRUCache, RelayCache,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestItem {
    id: i32,
    name: &'static str,
}

fn test_collection() -> KeyedCollectionSlim<i32, TestItem> {
    KeyedCollectionSlim::with_selector(0, |item: &TestItem| item.id)
}

#[test]
fn keyed_collection_add_should_add_item() {
    let mut collection = test_collection();
    let item = TestItem {
        id: 1,
        name: "item1",
    };

    assert!(collection.try_add(item.clone()));
    assert_eq!(collection.count(), 1);
    assert_eq!(collection.first_or_default(), Some(&item));
    assert!(collection.contains(&item.id));
}

#[test]
fn keyed_collection_add_rejects_duplicate_key() {
    let mut collection = test_collection();
    let item1 = TestItem {
        id: 1,
        name: "item1",
    };
    let item2 = TestItem {
        id: 1,
        name: "item2",
    };

    assert!(collection.try_add(item1.clone()));
    assert!(!collection.try_add(item2));

    collection.clear();
    assert_eq!(collection.count(), 0);
    assert!(collection.first_or_default().is_none());
}

#[test]
fn keyed_collection_remove_removes_item() {
    let mut collection = test_collection();
    let item = TestItem {
        id: 1,
        name: "item1",
    };
    collection.try_add(item.clone());

    assert!(collection.remove(&item.id));
    assert_eq!(collection.count(), 0);
    assert!(!collection.contains(&item.id));
}

#[test]
fn keyed_collection_remove_first_drops_oldest_item() {
    let mut collection = test_collection();
    let item1 = TestItem {
        id: 1,
        name: "item1",
    };
    let item2 = TestItem {
        id: 2,
        name: "item2",
    };

    collection.try_add(item1.clone());
    collection.try_add(item2.clone());

    assert!(collection.remove_first());
    assert_eq!(collection.count(), 1);
    assert!(!collection.contains(&item1.id));
    assert!(collection.contains(&item2.id));
}

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MockPoint(Vec<u8>);

impl EncodablePoint for MockPoint {
    fn encode_point_compressed(&self) -> Vec<u8> {
        self.0.clone()
    }
}

#[test]
fn ec_point_cache_uses_compressed_encoding_as_key() {
    let cache = ECPointCache::new(2);
    let point1 = MockPoint(vec![1, 2, 3]);
    let point2 = MockPoint(vec![4, 5, 6]);

    cache.add(point1.clone());
    cache.add(point2.clone());

    assert_eq!(cache.count(), 2);
    assert!(cache.contains(&point1));
    assert!(cache.contains(&point2));
    assert_eq!(cache.values().len(), 2);
}

#[derive(Clone, Debug, PartialEq)]
struct MockSigner(&'static str);

#[test]
fn ecdsa_cache_evicts_oldest_entries() {
    let cache: ECDsaCache<MockPoint, MockSigner> = ECDsaCache::new(2);
    cache.add(ECDsaCacheItem::new(MockPoint(vec![1]), MockSigner("A")));
    cache.add(ECDsaCacheItem::new(MockPoint(vec![2]), MockSigner("B")));

    assert!(cache.contains_key(&MockPoint(vec![1])));
    assert!(cache.contains_key(&MockPoint(vec![2])));

    cache.add(ECDsaCacheItem::new(MockPoint(vec![3]), MockSigner("C")));

    assert!(!cache.contains_key(&MockPoint(vec![1])));
    assert!(cache.contains_key(&MockPoint(vec![2])));
    assert!(cache.contains_key(&MockPoint(vec![3])));
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

#[test]
fn lru_cache_eviction_matches_csharp() {
    let cache = LRUCache::new(3, |item: &String| item.parse::<i32>().unwrap());
    assert!(cache.is_empty());

    cache.add("1".to_string());
    cache.add("2".to_string());
    cache.add("3".to_string());
    assert_eq!(cache.count(), 3);
    assert!(cache.contains(&"1".to_string()));
    assert!(cache.contains(&"2".to_string()));
    assert!(cache.contains(&"3".to_string()));
    assert!(!cache.contains(&"4".to_string()));

    let cached = cache.get(&2).unwrap();
    assert_eq!(cached, "2");
    assert_eq!(cache.count(), 3);
    assert!(cache.contains_key(&1));
    assert!(cache.contains_key(&2));
    assert!(cache.contains_key(&3));
    assert!(!cache.contains_key(&4));

    cache.add("4".to_string());
    assert_eq!(cache.count(), 3);
    assert!(cache.contains_key(&3));
    assert!(cache.contains_key(&2));
    assert!(cache.contains_key(&4));
    assert!(!cache.contains_key(&1));

    cache.add("5".to_string());
    assert_eq!(cache.count(), 3);
    assert!(!cache.contains_key(&1));
    assert!(cache.contains_key(&2));
    assert!(!cache.contains_key(&3));
    assert!(cache.contains_key(&4));
    assert!(cache.contains_key(&5));

    cache.add("6".to_string());
    assert_eq!(cache.count(), 3);
    assert!(cache.contains_key(&5));
    assert!(cache.contains_key(&6));
}
