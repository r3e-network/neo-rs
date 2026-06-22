use super::*;
use crate::IndexerError;
use crate::store::{
    ACCOUNT_TRANSACTION_PREFIX, BLOCK_BY_HASH_PREFIX, BLOCK_BY_HEIGHT_PREFIX,
    LEGACY_STORE_SNAPSHOT_KEY, NOTIFICATION_BY_ACCOUNT_PREFIX, NOTIFICATION_BY_BLOCK_PREFIX,
    NOTIFICATION_BY_CHAIN_PREFIX, NOTIFICATION_BY_CONTRACT_PREFIX,
    NOTIFICATION_BY_TRANSACTION_PREFIX, TRANSACTION_BY_CHAIN_PREFIX, TRANSACTION_BY_HASH_PREFIX,
    account_transaction_key, transaction_by_hash_key,
};
use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
use neo_primitives::{TriggerType, UInt160, WitnessScope};
use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::{SeekDirection, StoreProvider};
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;

fn account(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; UInt160::LENGTH]).expect("valid account")
}

fn transaction(nonce: u32, account: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![u8::try_from(nonce % 251).expect("bounded")]);
    tx.set_signers(vec![Signer::new(account, WitnessScope::CALLED_BY_ENTRY)]);
    tx
}

fn block_with_transactions(height: u32, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(height);
    let mut block = Block::from_parts(header, transactions);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn execution(
    tx: Transaction,
    contract: UInt160,
    event_name: &str,
    state: Vec<StackItem>,
) -> ApplicationExecuted {
    let mut execution = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    execution
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            event_name.to_string(),
            state,
        ));
    execution
}

fn execution_with_notifications(
    tx: Transaction,
    notifications: Vec<(UInt160, &'static str, Vec<StackItem>)>,
) -> ApplicationExecuted {
    let mut execution = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    for (contract, event_name, state) in notifications {
        execution
            .notifications
            .push(NotifyEventArgs::new_with_optional_container(
                None,
                contract,
                event_name.to_string(),
                state,
            ));
    }
    execution
}

fn transfer_state(from: UInt160, to: UInt160, amount: i64) -> Vec<StackItem> {
    vec![
        StackItem::from_byte_string(from.to_bytes()),
        StackItem::from_byte_string(to.to_bytes()),
        StackItem::from_int(amount),
    ]
}

fn count_store_rows(store: &Arc<dyn Store>, prefix: &[u8]) -> usize {
    let snapshot = store.snapshot();
    let prefix = prefix.to_vec();
    snapshot.find(Some(&prefix), SeekDirection::Forward).count()
}

fn put_legacy_store_snapshot(store: &Arc<dyn Store>, snapshot: &IndexerSnapshot) {
    let bytes = serde_json::to_vec(snapshot).expect("encode legacy snapshot");
    let mut store_snapshot = store.snapshot();
    let store_snapshot = Arc::get_mut(&mut store_snapshot).expect("unique store snapshot");
    store_snapshot
        .put(LEGACY_STORE_SNAPSHOT_KEY.to_vec(), bytes)
        .expect("put legacy snapshot");
    store_snapshot.try_commit().expect("commit legacy snapshot");
}

#[test]
fn service_indexes_blocks() {
    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());
    let service = IndexerService::new();

    let record = service.index_block(&block).expect("index block");

    assert_eq!(service.status().indexed_blocks, 1);
    assert_eq!(service.block_by_hash(&record.hash), Some(record));
}

#[path = "service/persistence.rs"]
mod persistence;
#[path = "service/store_backed.rs"]
mod store_backed;
