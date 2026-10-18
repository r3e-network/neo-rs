//! MPT state-root benchmarks: build/commit and lookup.
//!
//! The node's state root is a Merkle-Patricia Trie over storage changes. This
//! suite measures the trie itself with an in-memory snapshot so changes to the
//! MPT path have a stable, replayable baseline.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_crypto::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MemoryStore {
    data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MptStoreSnapshot for MemoryStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().expect("store").get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.data.lock().expect("store").insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.data.lock().expect("store").remove(&key);
        Ok(())
    }
}

fn entries(count: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
    (0..count)
        .map(|index| {
            let mut key = vec![0u8; 32];
            key[..8].copy_from_slice(&(index as u64).to_le_bytes());
            let mut value = vec![0u8; 32];
            value[..8].copy_from_slice(&((index as u64) * 7).to_le_bytes());
            (key, value)
        })
        .collect()
}

fn build_trie(count: usize) -> Trie<MemoryStore> {
    let store = Arc::new(MemoryStore::default());
    let mut trie = Trie::new(store, None, false);
    for (key, value) in entries(count) {
        trie.put(&key, &value).expect("put");
    }
    trie.commit().expect("commit");
    trie
}

fn bench_mpt_build(c: &mut Criterion) {
    for count in [1_000usize, 10_000] {
        c.bench_function(&format!("mpt_build_{count}"), |b| {
            b.iter(|| {
                let trie = build_trie(black_box(count));
                black_box(trie.root_hash());
            });
        });
    }
}

fn bench_mpt_get(c: &mut Criterion) {
    for count in [1_000usize, 10_000] {
        let mut trie = build_trie(count);
        let keys = entries(count);
        c.bench_function(&format!("mpt_get_{count}"), |b| {
            b.iter(|| {
                for (key, _) in &keys {
                    black_box(trie.get(black_box(key)).expect("get"));
                }
            });
        });
    }
}

criterion_group!(benches, bench_mpt_build, bench_mpt_get);
criterion_main!(benches);
