use neo_config::ProtocolSettings;
use neo_payloads::{
    ApplicationExecuted, CommittedHandler, CommittingHandler, Header, NotifyEventArgs, Signer,
    Transaction,
};
use neo_primitives::{TriggerType, UInt160, WitnessScope};
use neo_rpc::application_logs::ApplicationLogsSettings;
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;

use super::*;

fn test_block(height: u32, nonce: u32) -> Block {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![u8::try_from(nonce % 251).expect("bounded")]);

    let mut header = Header::new();
    header.set_index(height);
    header.set_nonce(u64::from(nonce));
    header.set_timestamp(1_700_000_000_000 + u64::from(height));
    let mut block = Block::from_parts(header, vec![tx]);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn indexer_status(
    indexed_height: Option<u32>,
    indexed_hash: Option<UInt256>,
    indexed_blocks: usize,
) -> IndexerStatus {
    IndexerStatus {
        indexed_height,
        indexed_hash,
        indexed_blocks,
        indexed_transactions: 0,
        indexed_accounts: 0,
        indexed_notifications: 0,
        indexed_notification_accounts: 0,
    }
}

fn hash(seed: u8) -> UInt256 {
    UInt256::from_bytes(&[seed; UInt256::LENGTH]).expect("valid hash")
}

fn notification_record(
    block_hash: UInt256,
    block_height: u32,
    notification_index: u32,
    contract_hash: UInt160,
) -> NotificationIndexRecord {
    NotificationIndexRecord {
        block_hash,
        block_height,
        tx_hash: None,
        execution_index: 0,
        notification_index,
        contract_hash,
        event_name: "Transfer".to_string(),
        trigger: "Application".to_string(),
        state_item_count: 0,
        state: Vec::new(),
        accounts: Vec::new(),
    }
}

#[test]
fn live_indexer_start_mode_defers_cold_catchup_until_near_tip() {
    assert_eq!(
        indexer_runtime_start_mode(0, 0),
        IndexerRuntimeStartMode::Deferred,
        "cold service-provider nodes should not start expensive indexer work before sync begins"
    );
    assert_eq!(
        indexer_runtime_start_mode(624_433, 690_000),
        IndexerRuntimeStartMode::Deferred,
        "nodes still far behind the peer tip should keep sync throughput prioritized"
    );
    assert_eq!(
        indexer_runtime_start_mode(680_000, 690_000),
        IndexerRuntimeStartMode::StartNow,
        "once the node is inside the activation window, the indexer should start without restart"
    );
    assert_eq!(
        indexer_runtime_start_mode(1, 0),
        IndexerRuntimeStartMode::StartNow,
        "private or isolated networks with no known peer tip can index once a durable tip exists"
    );
}

#[test]
fn deferred_live_indexer_activation_waits_for_peer_tip_after_cold_start() {
    assert!(
        !indexer_runtime_activation_reached(IndexerRuntimeStartMode::Deferred, 1, 0),
        "a cold-started node should not begin expensive indexer work before observing a peer tip"
    );
    assert!(
        !indexer_runtime_activation_reached(IndexerRuntimeStartMode::Deferred, 624_433, 690_000),
        "a cold-started node should keep indexer deferred while still far behind the peer tip"
    );
    assert!(
        indexer_runtime_activation_reached(IndexerRuntimeStartMode::Deferred, 680_000, 690_000),
        "a cold-started node should activate the indexer once it is near the peer tip"
    );
    assert!(
        indexer_runtime_activation_reached(IndexerRuntimeStartMode::StartNow, 1, 0),
        "warm private or isolated networks keep the immediate-start behavior"
    );
}

#[test]
fn deferred_indexer_activation_forces_backfill_to_avoid_event_gap() {
    assert!(
        indexer_should_backfill_on_activation(false, IndexerRuntimeStartMode::Deferred),
        "a deferred runtime missed earlier import events and must scan the canonical chain once"
    );
    assert!(
        !indexer_should_backfill_on_activation(false, IndexerRuntimeStartMode::StartNow),
        "immediate startup should honor explicit backfill_on_startup=false"
    );
    assert!(
        indexer_should_backfill_on_activation(true, IndexerRuntimeStartMode::StartNow),
        "explicit startup backfill remains honored"
    );
}

#[test]
fn resumable_backfill_starts_after_verified_contiguous_indexed_tip() {
    let tip_hash = hash(1);
    let status = indexer_status(Some(7), Some(tip_hash), 8);

    let start = backfill_start_height_from_status(10, status, Some(tip_hash));

    assert_eq!(start, Some(8));
}

#[test]
fn resumable_backfill_falls_back_to_genesis_for_unsafe_status() {
    let tip_hash = hash(1);

    assert_eq!(
        backfill_start_height_from_status(10, indexer_status(None, None, 0), None),
        Some(0)
    );
    assert_eq!(
        backfill_start_height_from_status(
            10,
            indexer_status(Some(7), Some(tip_hash), 7),
            Some(tip_hash)
        ),
        Some(0),
        "missing historical row means the index is not safely contiguous"
    );
    assert_eq!(
        backfill_start_height_from_status(
            10,
            indexer_status(Some(7), Some(tip_hash), 8),
            Some(hash(2))
        ),
        Some(0),
        "tip hash mismatch means the persisted index may be stale"
    );
    assert_eq!(
        backfill_start_height_from_status(10, indexer_status(Some(11), Some(tip_hash), 12), None),
        Some(0),
        "indexed height beyond canonical height is pruned by a full scan"
    );
}

#[test]
fn resumable_backfill_skips_when_verified_tip_is_max_height() {
    let tip_hash = hash(3);
    let status = indexer_status(
        Some(u32::MAX),
        Some(tip_hash),
        usize::try_from(u32::MAX)
            .expect("u32 fits usize")
            .saturating_add(1),
    );

    let start = backfill_start_height_from_status(u32::MAX, status, Some(tip_hash));

    assert_eq!(start, None);
}

#[test]
fn application_log_recovery_reports_missing_executions_array() {
    let err = parse_application_log_executions(&serde_json::json!({}), None)
        .expect_err("missing executions array");

    assert!(matches!(
        err,
        ApplicationLogRecoveryError::MissingExecutions
    ));
    assert_eq!(err.to_string(), "application log missing executions array");
}

#[test]
fn startup_backfill_prunes_snapshot_records_above_canonical_height() {
    let indexer = IndexerService::new();
    let canonical_block = test_block(3, 30);
    let stale_block = test_block(9, 90);
    let stale_hash = stale_block.try_hash().expect("stale hash");

    indexer
        .index_block(&canonical_block)
        .expect("canonical block");
    indexer.index_block(&stale_block).expect("stale block");
    assert_eq!(indexer.status().indexed_height, Some(9));

    prune_indexer_to_canonical_height(&indexer, 3);

    assert_eq!(indexer.status().indexed_height, Some(3));
    assert!(indexer.block_by_height(3).is_some());
    assert!(indexer.block_by_height(9).is_none());
    assert!(indexer.block_by_hash(&stale_hash).is_none());
}

#[test]
fn backfill_skip_preserves_existing_notifications() {
    let indexer = IndexerService::new();
    let signer = UInt160::from_bytes(&[3; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[4; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(92);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(6);
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
    indexer
        .index_block_with_application_executions(&block, &[executed])
        .expect("index block notifications");

    assert!(block_is_already_indexed(&indexer, &block, 6));
    assert!(
        !should_index_block(&indexer, &block, 6, &[]),
        "runtime should skip an already-indexed block with live notifications"
    );

    assert_eq!(
        indexer.notifications_for_transaction(&tx_hash, 0, 10).len(),
        1
    );
}

#[test]
fn backfill_repairs_partially_indexed_notifications() {
    let indexer = IndexerService::new();
    let block = test_block(8, 80);
    let block_hash = block.try_hash().expect("block hash");
    let first = notification_record(
        block_hash,
        8,
        0,
        UInt160::from_bytes(&[8; UInt160::LENGTH]).expect("first contract"),
    );
    let second = notification_record(
        block_hash,
        8,
        1,
        UInt160::from_bytes(&[9; UInt160::LENGTH]).expect("second contract"),
    );

    indexer
        .index_block_with_notification_records(&block, vec![first.clone()])
        .expect("index partial notifications");

    assert!(block_is_already_indexed(&indexer, &block, 8));
    assert!(
        should_enrich_notifications(&indexer, &block, &[first.clone(), second.clone()]),
        "runtime should re-index a block when ApplicationLogs has more complete notifications"
    );
    assert!(
        should_index_block(&indexer, &block, 8, &[first, second]),
        "backfill should repair partial notification indexes"
    );
}

#[test]
fn application_logs_recover_indexer_notifications_for_backfill() {
    let settings = Arc::new(ProtocolSettings::default());
    let _node = neo_system::Node::new(Arc::clone(&settings), None, None).expect("node");
    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs = ApplicationLogsService::new(logs_settings, Arc::new(MemoryStore::new()));

    let signer = UInt160::from_bytes(&[7; UInt160::LENGTH]).expect("signer");
    let recipient = UInt160::from_bytes(&[8; UInt160::LENGTH]).expect("recipient");
    let contract = UInt160::from_bytes(&[9; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(93);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(7);
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
            vec![
                StackItem::from_byte_string(signer.to_bytes()),
                StackItem::from_byte_string(recipient.to_bytes()),
                StackItem::from_int(100),
            ],
        ));

    logs.blockchain_committing_handler(settings.network, &block, snapshot.as_ref(), &[executed]);
    logs.blockchain_committed_handler(settings.network, &block);

    let records =
        application_log_notification_records(&logs, &block).expect("recover notifications");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tx_hash, Some(tx_hash));
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].trigger, "Application");
    assert_eq!(records[0].state_item_count, 3);

    let indexer = IndexerService::new();
    indexer.index_block(&block).expect("index block only");
    assert!(should_enrich_notifications(&indexer, &block, &records));
    assert!(should_index_block(&indexer, &block, 7, &records));
    index_block_with_available_notifications(&indexer, &block, records)
        .expect("index recovered notifications");
    assert_eq!(
        indexer.notifications_for_account(&signer, 0, 10),
        indexer.notifications_for_transaction(&tx_hash, 0, 10)
    );
    assert_eq!(
        indexer.notifications_for_account(&recipient, 0, 10).len(),
        1
    );
}
