use super::*;

use crate::{
    BlockProvider, HotColdLedgerProviderFactory, LedgerProviderFactory, LedgerPruningStore,
    PruningReadiness, StorageLedgerProvider, StorageLedgerProviderFactory, TxProvider,
};
use neo_payloads::{Block, Header, Signer, Transaction, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_storage::DataCache;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("neo-rs-{prefix}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![neo_vm_rs::OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn block(index: u32, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    let mut block = Block::from_parts(header, transactions);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn archive_at(path: &Path) -> StaticLedgerArchive {
    StaticLedgerArchive::open(path).expect("open static archive")
}

#[test]
fn static_ledger_archive_reopens_appended_blocks_and_transactions() {
    let temp = TempDir::new("static-ledger-archive");
    let tx = transaction(42);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(7, vec![tx.clone()]);
    let block_hash = block.try_hash().expect("block hash");

    {
        let archive = archive_at(temp.path());
        assert_eq!(
            archive.append_block(&block).expect("append block"),
            block_hash
        );
    }

    let reopened = archive_at(temp.path());
    assert_eq!(
        BlockProvider::block_hash_by_index(&reopened, 7).expect("block hash"),
        Some(block_hash)
    );
    assert_eq!(
        BlockProvider::block_by_hash(&reopened, &block_hash)
            .expect("block by hash")
            .expect("block present")
            .transactions[0]
            .hash(),
        tx_hash
    );
    assert_eq!(
        BlockProvider::block_by_index(&reopened, 7)
            .expect("block by index")
            .expect("block present")
            .hash(),
        block_hash
    );
    assert_eq!(
        TxProvider::transaction_by_hash(&reopened, &tx_hash)
            .expect("tx by hash")
            .expect("tx present")
            .hash(),
        tx_hash
    );
}

#[test]
fn hot_cold_provider_falls_back_to_static_archive_for_cold_history() {
    let temp = TempDir::new("hot-cold-ledger-provider");
    let archive = archive_at(temp.path());
    let tx = transaction(7);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(9, vec![tx.clone()]);
    let block_hash = archive.append_block(&block).expect("append block");

    let hot_cache = DataCache::new(false);
    let hot = StorageLedgerProvider::new(&hot_cache);
    let provider = HotColdLedgerProvider::new(hot, &archive);

    assert_eq!(
        BlockProvider::block_hash_by_index(&provider, 9).expect("block hash"),
        Some(block_hash)
    );
    assert_eq!(
        BlockProvider::block_by_index(&provider, 9)
            .expect("cold block")
            .expect("block present")
            .transactions[0]
            .hash(),
        tx_hash
    );
    assert_eq!(
        TxProvider::transaction_by_hash(&provider, &tx_hash)
            .expect("cold tx")
            .expect("tx present")
            .hash(),
        tx_hash
    );
}

#[test]
fn hot_cold_ledger_provider_factory_composes_latest_providers() {
    let temp = TempDir::new("hot-cold-ledger-provider-factory");
    let archive = archive_at(temp.path());
    let tx = transaction(17);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(12, vec![tx]);
    archive.append_block(&block).expect("append block");

    let hot_cache = Arc::new(DataCache::new(false));
    let hot_factory = StorageLedgerProviderFactory::new(hot_cache);
    let factory = HotColdLedgerProviderFactory::new(hot_factory, archive);
    let provider = LedgerProviderFactory::latest(&factory).expect("latest provider");

    assert_eq!(
        BlockProvider::block_by_index(&provider, 12)
            .expect("block by index")
            .expect("block present")
            .hash(),
        block.hash()
    );
    assert_eq!(
        TxProvider::transaction_by_hash(&provider, &tx_hash)
            .expect("tx by hash")
            .expect("tx present")
            .hash(),
        tx_hash
    );
}

#[test]
fn pruning_acknowledgements_survive_reopen_and_bound_safe_height() {
    let temp = TempDir::new("ledger-pruning-acks");
    let archive = archive_at(temp.path());
    for height in 1..=5 {
        archive
            .append_block(&block(height, Vec::new()))
            .expect("append archive block");
    }

    {
        let store = LedgerPruningStore::open(temp.path()).expect("open pruning store");
        store.acknowledge("indexer", 5).expect("ack indexer height");
        store
            .acknowledge("application-logs", 3)
            .expect("ack application logs height");
    }

    let reopened = LedgerPruningStore::open(temp.path()).expect("reopen pruning store");
    assert_eq!(reopened.acknowledged_height("indexer"), Some(5));
    assert_eq!(reopened.acknowledged_height("application-logs"), Some(3));
    assert_eq!(
        reopened
            .readiness(&archive, 1)
            .expect("read pruning readiness"),
        PruningReadiness::Ready {
            prune_through_height: 2,
            limiting_consumer: "application-logs".to_string(),
            acknowledged_height: 3,
            retention_blocks: 1,
        }
    );
}

#[test]
fn pruning_acknowledgements_do_not_regress_consumer_progress() {
    let temp = TempDir::new("ledger-pruning-monotonic");

    let store = LedgerPruningStore::open(temp.path()).expect("open pruning store");
    store.acknowledge("indexer", 5).expect("ack indexer");
    store
        .acknowledge("indexer", 3)
        .expect("lower ack remains conservative no-op");
    assert_eq!(store.acknowledged_height("indexer"), Some(5));

    let reopened = LedgerPruningStore::open(temp.path()).expect("reopen pruning store");
    assert_eq!(reopened.acknowledged_height("indexer"), Some(5));
}

#[test]
fn pruning_readiness_waits_for_archive_coverage_and_consumers() {
    let temp = TempDir::new("ledger-pruning-coverage");
    let archive = archive_at(temp.path());
    archive
        .append_block(&block(1, Vec::new()))
        .expect("append archive block 1");
    archive
        .append_block(&block(3, Vec::new()))
        .expect("append archive block 3");

    let store = LedgerPruningStore::open(temp.path()).expect("open pruning store");
    assert_eq!(
        store.readiness(&archive, 0).expect("no consumers"),
        PruningReadiness::NoConsumers
    );

    store.acknowledge("indexer", 3).expect("ack indexer");
    assert_eq!(
        store
            .readiness(&archive, 0)
            .expect("missing archive coverage"),
        PruningReadiness::WaitingForArchive {
            prune_through_height: 3,
            missing_height: 2,
        }
    );
}
