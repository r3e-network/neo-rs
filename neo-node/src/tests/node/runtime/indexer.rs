//! # neo-node::tests::node::runtime::indexer
//!
//! Indexer pre-commit, continuity, persistence-failure, and recovery tests.
//!
//! ## Boundary
//!
//! These tests assemble daemon hooks around real indexer services but do not
//! define production behavior.
//!
//! ## Contents
//!
//! - Live notification indexing over a contiguous projection prefix.
//! - Gap prevention while the durable Index stage catches up.
//! - Fail-closed MDBX persistence and replay-marker coverage.

use super::*;

#[test]
fn daemon_context_indexes_application_executed_notifications() {
    use neo_blockchain::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, UInt256, WitnessScope};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_vm::VmState as VMState;

    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let mut previous_hash = UInt256::zero();
    for height in 0..5 {
        let mut header = Header::new();
        header.set_index(height);
        header.set_prev_hash(previous_hash);
        let block = Block::from_parts(header, Vec::new());
        previous_hash = block.try_hash().expect("seed block hash");
        indexer
            .index_block(&block)
            .expect("seed contiguous indexer prefix");
    }
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        Some(Arc::clone(&indexer)),
        native_provider(),
        None,
    );

    let signer = UInt160::from_bytes(&[1; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[2; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(91);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(5);
    header.set_prev_hash(previous_hash);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[executed]));

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].block_height, 5);
}

#[test]
fn daemon_context_does_not_create_ahead_of_stage_index_gap() {
    use neo_blockchain::SystemContext;
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        Some(Arc::clone(&indexer)),
        native_provider(),
        None,
    );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing(&block, &snapshot, &[]));
    assert_eq!(indexer.status().indexed_height, None);
    assert_eq!(indexer.status().indexed_blocks, 0);
}

#[test]
fn persistent_indexer_write_failure_rejects_precommit_and_requests_shutdown() {
    use neo_blockchain::SystemContext;
    use neo_payloads::{Block, Header};
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;

    let temp = tempfile::tempdir().expect("temp dir");
    let indexer_path = temp.path().join("indexer");
    let writable = MdbxStoreProvider::new(StorageConfig {
        path: indexer_path.clone(),
        ..StorageConfig::default()
    })
    .get_mdbx_store("")
    .expect("initialize indexer MDBX");
    drop(writable);
    let read_only = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: indexer_path,
            read_only: true,
            ..StorageConfig::default()
        })
        .get_mdbx_store("")
        .expect("open read-only indexer MDBX"),
    );
    let indexer = Arc::new(
        neo_indexer::IndexerService::open_store(read_only).expect("open persistent indexer"),
    );

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        Some(Arc::clone(&indexer)),
        native_provider(),
        None,
    );
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing(&block, &snapshot, &[]),
        "a persistent pre-commit projection cannot fail open"
    );
    assert!(ctx.should_stop_blockchain_service());
    assert!(ctx.shutdown.is_cancelled());
    assert_eq!(indexer.status().indexed_height, None);
    assert_eq!(indexer.status().indexed_blocks, 0);
}

#[test]
fn persistent_indexer_arms_replay_marker_before_precommit_write() {
    use neo_blockchain::BlockPersistContext;
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_system::BlockCommitHooks;

    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let indexer_store = Arc::new(MemoryStore::new());
    let indexer = Arc::new(
        neo_indexer::IndexerService::open_store(indexer_store).expect("persistent indexer service"),
    );
    let hooks: DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    > = DaemonCommitHooks::new(
        ProtocolSettings::default().network,
        None,
        false,
        Some(indexer),
        None,
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            Some(marker.clone()),
            shutdown,
        )),
    );
    let chain_store = Arc::new(MemoryStore::new());
    let snapshot = StoreCache::new_from_store(chain_store, false)
        .data_cache()
        .clone();
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(BlockCommitHooks::block_committing(
        &hooks,
        &block,
        &snapshot,
        &[],
        1,
        BlockPersistContext::live(),
    ));
    assert!(
        marker.exists(),
        "persistent pre-commit indexer writes require write-ahead recovery"
    );
    <DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    > as BlockCommitHooks<MemoryStore>>::fence_precommit_durability(&hooks)
    .expect("persistent indexer durability fence");
    crate::node::recovery::refuse_local_replay_marker(Some(&marker))
        .expect_err("startup must reject an indexer pre-commit without canonical Ledger");
}
