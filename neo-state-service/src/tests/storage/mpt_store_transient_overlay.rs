use super::write_batch::MPT_NODE_KEY_SIZE;
use super::*;
use std::collections::HashMap;

#[test]
fn write_batch_collapses_only_nodes_absent_from_its_durable_base() {
    let mut durable_key = vec![0xF0; MPT_NODE_KEY_SIZE];
    durable_key[MPT_NODE_KEY_SIZE - 1] = 1;
    let mut transient_key = vec![0xF0; MPT_NODE_KEY_SIZE];
    transient_key[MPT_NODE_KEY_SIZE - 1] = 2;
    let base = Arc::new(HashMap::from([(
        durable_key.clone(),
        Some(b"old-value".to_vec()),
    )]));
    let batch = MptWriteBatch::<MemoryStore>::new(base, None, None, 4);

    assert_eq!(
        batch.try_get(&durable_key).expect("durable lookup"),
        Some(b"old-value".to_vec())
    );
    assert_eq!(
        batch.try_get(&transient_key).expect("transient lookup"),
        None
    );
    batch
        .apply_overlay(vec![
            (transient_key.clone(), Some(b"temporary".to_vec())),
            (durable_key.clone(), None),
        ])
        .expect("stage initial overlay");
    batch
        .apply_overlay(vec![(transient_key.clone(), None)])
        .expect("prune transient node");

    let overlay = batch.overlay.lock();
    assert!(!overlay.contains_key(&transient_key));
    assert_eq!(overlay.get(&durable_key), Some(&None));
}
