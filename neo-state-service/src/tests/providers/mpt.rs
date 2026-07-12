use std::sync::Arc;

use neo_primitives::UInt256;

use super::*;
use crate::{MptChange, MptStore, StateStore, verify_state_proof};

fn storage_key(contract_id: i32, suffix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(std::mem::size_of::<i32>() + suffix.len());
    key.extend_from_slice(&contract_id.to_le_bytes());
    key.extend_from_slice(suffix);
    key
}

fn put(contract_id: i32, suffix: &[u8], value: &[u8]) -> MptChange {
    MptChange::Put {
        key: storage_key(contract_id, suffix),
        value: value.to_vec(),
    }
}

#[test]
fn factory_selects_latest_height_and_historical_root_without_type_erasure() {
    let store = Arc::new(MptStore::new(true));
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0x10, 0x01], b"one")])
        .expect("block 1");
    let root2 = store
        .apply_block_changes(
            2,
            Some(root1),
            &[
                put(5, &[0x10, 0x01], b"two"),
                put(5, &[0x10, 0x02], b"second"),
            ],
        )
        .expect("block 2");
    let factory = MptStateProviderFactory::new(store);

    let mut historical = factory
        .state_at(1)
        .expect("select height")
        .expect("height exists");
    assert_eq!(historical.root_hash(), root1);
    assert_eq!(historical.block_index(), Some(1));
    assert_eq!(
        historical
            .get(&storage_key(5, &[0x10, 0x01]))
            .expect("historical get"),
        Some(b"one".to_vec())
    );

    let mut latest = factory.latest().expect("select latest").expect("latest");
    assert_eq!(latest.root_hash(), root2);
    assert_eq!(latest.block_index(), Some(2));
    assert_eq!(
        latest
            .get(&storage_key(5, &[0x10, 0x01]))
            .expect("latest get"),
        Some(b"two".to_vec())
    );
    assert!(factory.state_at(3).expect("missing height").is_none());
}

#[test]
fn opened_view_remains_frozen_when_pruned_store_advances() {
    let store = Arc::new(MptStore::new(false));
    let key = storage_key(7, &[0x20]);
    let root1 = store
        .apply_block_changes(1, None, &[put(7, &[0x20], b"before")])
        .expect("block 1");
    let factory = MptStateProviderFactory::new(Arc::clone(&store));
    let mut frozen = factory.state_by_root(root1).expect("current root view");

    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(7, &[0x20], b"after")])
        .expect("block 2");

    assert_eq!(
        frozen.get(&key).expect("frozen read"),
        Some(b"before".to_vec()),
        "a view must keep the generation captured before pruning"
    );
    let error = factory
        .state_by_root(root1)
        .expect_err("new historical views are gated in pruning mode");
    assert_eq!(
        error.to_string(),
        format!("fullState:False,current:{root2},rootHash:{root1}")
    );
    assert!(error.is_unsupported_state());
    assert_eq!(
        factory
            .root_at(1)
            .expect("historical root metadata")
            .expect("root record")
            .root_hash(),
        &root1,
        "pruning state nodes must not hide historical StateRoot metadata"
    );
}

#[test]
fn provider_scan_and_proof_share_the_selected_root() {
    let store = Arc::new(MptStore::new(true));
    let root = store
        .apply_block_changes(
            1,
            None,
            &[
                put(9, &[0xAA, 0x01], b"alpha"),
                put(9, &[0xAA, 0x02], b"beta"),
                put(9, &[0xBB, 0x01], b"other"),
            ],
        )
        .expect("block");
    let factory = MptStateProviderFactory::new(store);
    let mut provider = factory.state_by_root(root).expect("root view");
    let prefix = storage_key(9, &[0xAA]);

    let entries = provider.find(&prefix, None, 10).expect("prefix scan");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.key.starts_with(&prefix)));

    let key = storage_key(9, &[0xAA, 0x02]);
    let proof = provider
        .proof(&key)
        .expect("proof query")
        .expect("proof exists");
    assert_eq!(
        verify_state_proof(root, &key, &proof).expect("proof verifies"),
        b"beta".to_vec()
    );
}

#[test]
fn state_store_creates_factory_only_when_mpt_is_enabled() {
    assert!(StateStore::new().state_provider_factory().is_none());

    let state_store = StateStore::with_mpt(true);
    let factory = state_store
        .state_provider_factory()
        .expect("MPT-backed service exposes provider factory");
    assert!(factory.latest().expect("empty latest query").is_none());
}

#[test]
fn provider_contract_uses_associated_concrete_types() {
    let source = include_str!("../../providers/traits.rs");

    assert!(source.contains("type Provider: StateView;"));
    assert!(!source.contains("Box<dyn StateView>"));
    assert!(!source.contains("Arc<dyn StateView>"));
}

#[test]
fn full_history_defers_unknown_root_failure_to_the_read_operation() {
    let store = Arc::new(MptStore::new(true));
    let root = store
        .apply_block_changes(1, None, &[put(5, &[0x01], b"value")])
        .expect("block");
    let unknown = UInt256::from([0x77; 32]);
    assert_ne!(unknown, root);

    let factory = MptStateProviderFactory::new(store);
    let mut provider = factory
        .state_by_root(unknown)
        .expect("full history accepts root selection");
    let error = provider
        .get(&storage_key(5, &[0x01]))
        .expect_err("unknown root cannot resolve");
    assert!(!error.is_unsupported_state());
}
