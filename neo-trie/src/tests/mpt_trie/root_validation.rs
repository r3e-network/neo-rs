use super::*;
use crate::{MPT_NODE_PREFIX, PersistedMptGraphLimits, validate_persisted_root_graph};
use neo_primitives::UINT256_SIZE;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingStore {
    inner: MockStore,
    reads: AtomicUsize,
}

impl CountingStore {
    fn new(inner: MockStore) -> Self {
        Self {
            inner,
            reads: AtomicUsize::new(0),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }
}

impl MptStoreSnapshot for CountingStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.try_get(key)
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.inner.put(key, value)
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.inner.delete(key)
    }
}

fn node_key(hash: UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + UINT256_SIZE);
    key.push(MPT_NODE_PREFIX);
    key.extend_from_slice(&hash.to_array());
    key
}

fn persist(store: &MockStore, node: &Node) -> (UInt256, u64) {
    let hash = node.try_hash().expect("hash test node");
    let bytes = node.to_array().expect("serialize test node");
    let byte_len = u64::try_from(bytes.len()).expect("test node length fits u64");
    store.put(node_key(hash), bytes).expect("persist test node");
    (hash, byte_len)
}

fn shared_graph() -> (MockStore, UInt256, PersistedMptGraphReportFixture) {
    let store = MockStore::new();
    let mut leaf = Node::new_leaf(vec![0xAA, 0xBB]);
    leaf.reference = 2;
    let (leaf_hash, leaf_bytes) = persist(&store, &leaf);

    let extension = Node::new_extension(vec![0x01], Node::new_hash(leaf_hash))
        .expect("construct test extension");
    let (extension_hash, extension_bytes) = persist(&store, &extension);

    let mut root = Node::new_branch();
    root.set_child(0, Node::new_hash(leaf_hash));
    root.set_child(1, Node::new_hash(extension_hash));
    let (root_hash, root_bytes) = persist(&store, &root);

    (
        store,
        root_hash,
        PersistedMptGraphReportFixture {
            total_bytes: leaf_bytes + extension_bytes + root_bytes,
            root_bytes,
        },
    )
}

struct PersistedMptGraphReportFixture {
    total_bytes: u64,
    root_bytes: u64,
}

fn permissive_limits(fixture: &PersistedMptGraphReportFixture) -> PersistedMptGraphLimits {
    PersistedMptGraphLimits::new(16, fixture.total_bytes + 1, fixture.total_bytes + 1)
}

#[test]
fn persisted_root_validation_counts_a_shared_graph_once() {
    let (store, root, fixture) = shared_graph();

    let report = validate_persisted_root_graph(&store, root, permissive_limits(&fixture))
        .expect("shared graph must validate");

    assert_eq!(report.unique_nodes, 3);
    assert_eq!(report.total_bytes, fixture.total_bytes);
    assert_eq!(report.branch_nodes, 1);
    assert_eq!(report.extension_nodes, 1);
    assert_eq!(report.leaf_nodes, 1);
}

#[test]
fn persisted_root_validation_rejects_a_missing_child() {
    let store = MockStore::new();
    let missing = UInt256::from([0x7A; UINT256_SIZE]);
    let mut root = Node::new_branch();
    root.set_child(0, Node::new_hash(missing));
    let (root_hash, root_bytes) = persist(&store, &root);
    let limits = PersistedMptGraphLimits::new(2, root_bytes + 1024, root_bytes + 1024);

    let error = validate_persisted_root_graph(&store, root_hash, limits)
        .expect_err("missing reachable rows must fail");

    assert!(error.to_string().contains("missing node"));
    assert!(error.to_string().contains(&missing.to_string()));
}

#[test]
fn persisted_root_validation_rejects_a_row_stored_under_the_wrong_hash() {
    let store = MockStore::new();
    let requested = UInt256::from([0x31; UINT256_SIZE]);
    let leaf = Node::new_leaf(vec![0x55]);
    let bytes = leaf.to_array().expect("serialize mismatched row");
    let byte_len = u64::try_from(bytes.len()).expect("test row length fits u64");
    store
        .put(node_key(requested), bytes)
        .expect("persist mismatched row");
    let limits = PersistedMptGraphLimits::new(1, byte_len, byte_len);

    let error = validate_persisted_root_graph(&store, requested, limits)
        .expect_err("row payload must bind to its requested hash");

    assert!(error.to_string().contains("does not match"));
}

#[test]
fn persisted_root_validation_enforces_node_count_before_loading_another_row() {
    let (store, root, fixture) = shared_graph();
    let store = CountingStore::new(store);
    let limits = PersistedMptGraphLimits::new(2, fixture.total_bytes + 1, fixture.total_bytes + 1);

    let error = validate_persisted_root_graph(&store, root, limits)
        .expect_err("the third distinct node must exceed the node ceiling");

    assert!(error.to_string().contains("max_nodes 2"));
    assert_eq!(store.reads(), 2, "the over-limit node must not be read");
}

#[test]
fn persisted_root_validation_enforces_per_node_and_total_byte_limits() {
    let (store, root, fixture) = shared_graph();
    let node_limit =
        PersistedMptGraphLimits::new(16, fixture.total_bytes + 1, fixture.root_bytes - 1);
    let node_error = validate_persisted_root_graph(&store, root, node_limit)
        .expect_err("root row must exceed the per-node ceiling");
    assert!(node_error.to_string().contains("max_node_bytes"));

    let total_limit = PersistedMptGraphLimits::new(16, fixture.total_bytes - 1, fixture.root_bytes);
    let total_error = validate_persisted_root_graph(&store, root, total_limit)
        .expect_err("complete graph must exceed the total-byte ceiling");
    assert!(total_error.to_string().contains("max_total_bytes"));
}

#[test]
fn persisted_root_validation_rejects_zero_and_inconsistent_limits_before_reading() {
    let store = CountingStore::new(MockStore::new());
    let root = UInt256::from([0x19; UINT256_SIZE]);

    for limits in [
        PersistedMptGraphLimits::new(0, 1, 1),
        PersistedMptGraphLimits::new(1, 0, 1),
        PersistedMptGraphLimits::new(1, 1, 0),
        PersistedMptGraphLimits::new(1, 1, 2),
    ] {
        let error = validate_persisted_root_graph(&store, root, limits)
            .expect_err("invalid limits must fail before reading the root");
        assert!(!error.to_string().contains("missing node"));
    }
    assert_eq!(store.reads(), 0, "invalid limits must not touch storage");
}
