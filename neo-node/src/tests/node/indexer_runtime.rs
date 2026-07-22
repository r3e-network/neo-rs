use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
use neo_primitives::{TriggerType, UInt160, UInt256, WitnessScope};
use neo_rpc::application_logs::ApplicationLogsSettings;
use neo_runtime::{CommittedHandler, CommittingHandler};
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_vm::StackItem;
use neo_vm::VmState as VMState;

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
fn application_log_recovery_reports_missing_executions_array() {
    let err = parse_application_log_executions(&serde_json::json!({}), None)
        .expect_err("missing executions array");

    assert!(matches!(
        err,
        ApplicationLogRecoveryError::MissingExecutions
    ));
    assert_eq!(err.to_string(), "application log missing executions array");
}

#[path = "indexer_runtime/stage.rs"]
mod stage;

#[test]
fn application_logs_supply_notifications_to_the_index_stage() {
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("valid MainNet chain spec");
    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = chain_spec.network_magic();
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

    logs.blockchain_committing_handler(
        chain_spec.network_magic(),
        &block,
        snapshot.as_ref(),
        &[executed],
    );
    logs.blockchain_committed_handler(chain_spec.network_magic(), &block);

    let records =
        application_log_notification_records(&logs, &block).expect("recover notifications");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tx_hash, Some(tx_hash));
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].trigger, "Application");
    assert_eq!(records[0].state_item_count, 3);

    let indexer = IndexerService::new();
    indexer
        .index_block_with_notification_records(&block, records)
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
