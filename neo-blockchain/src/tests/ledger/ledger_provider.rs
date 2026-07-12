use super::*;

use std::collections::HashMap;

use neo_error::CoreResult;
use neo_io::Serializable;
use neo_payloads::{Block, Header, Signer, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_storage::DataCache;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{RawReadOnlyStore, Store, StoreCache};
use neo_vm_rs::VmState;

use crate::StaticLedgerArchive;

fn test_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![0x40]); // RET
    tx.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[0x22; 20]).expect("account"),
        WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn transaction_bytes(tx: &Transaction) -> Vec<u8> {
    let mut writer = neo_io::BinaryWriter::new();
    tx.serialize(&mut writer).expect("serialize transaction");
    writer.into_bytes()
}

#[test]
fn storage_ledger_provider_reconstructs_block_and_transaction_from_ledger_records() {
    let cache = DataCache::new(false);
    let mut header = Header::new();
    header.set_index(7);
    let tx = test_transaction(99);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = Block::from_parts(header, vec![tx.clone()]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&cache, &block_hash, 7)
        .expect("post-persist record");
    let provider = StorageLedgerProvider::new(&cache);

    assert_eq!(
        BlockProvider::block_hash_by_index(&provider, 7).expect("block hash"),
        Some(block_hash)
    );
    assert_eq!(
        ChainTipProvider::current_hash(&provider).expect("current hash"),
        block_hash
    );
    assert_eq!(
        ChainTipProvider::current_index(&provider).expect("current index"),
        7
    );
    assert_eq!(
        ChainTipProvider::block_count(&provider).expect("block count"),
        8
    );
    let full_block = BlockProvider::block_by_index(&provider, 7)
        .expect("load block")
        .expect("block present");
    assert_eq!(full_block.hash(), block.hash());
    assert_eq!(full_block.transactions.len(), 1);
    assert_eq!(full_block.transactions[0].hash(), tx_hash);

    let stored_tx = TxProvider::transaction_by_hash(&provider, &tx_hash)
        .expect("load transaction")
        .expect("transaction present");
    assert_eq!(transaction_bytes(&stored_tx), transaction_bytes(&tx));
    let stored_state = provider
        .transaction_state_by_hash(&tx_hash)
        .expect("load transaction state")
        .expect("transaction state present");
    assert_eq!(stored_state.block_index, 7);
    assert_eq!(stored_state.state, VmState::NONE);
    assert_eq!(
        stored_state.transaction.expect("state transaction").hash(),
        tx_hash
    );
    assert!(TxProvider::contains_transaction(&provider, &tx_hash).expect("contains transaction"));
}

#[test]
fn storage_ledger_provider_factory_creates_snapshot_provider() {
    let cache = DataCache::new(false);
    let mut header = Header::new();
    header.set_index(11);
    let block = Block::from_parts(header, vec![]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&cache, &block_hash, 11)
        .expect("post-persist record");

    let factory = StorageLedgerProviderFactory;
    let provider = factory.provider(&cache);

    assert_eq!(
        provider.block_hash_by_index(11).expect("block hash"),
        Some(block_hash)
    );
    assert_eq!(
        provider
            .header_by_index(11)
            .expect("header read")
            .expect("header")
            .index(),
        11
    );
}

#[test]
fn empty_ledger_provider_reports_clean_misses() {
    let cache = DataCache::new(false);
    let provider = EmptyLedgerProviderFactory.provider(&cache);
    let hash = UInt256::from_bytes(&[0xAB; 32]).expect("hash");

    assert_eq!(
        provider
            .block_hash_by_index(42)
            .expect("empty index lookup"),
        None
    );
    assert!(
        provider
            .header_by_hash(&hash)
            .expect("empty header lookup")
            .is_none()
    );
    assert!(
        provider
            .block_by_hash(&hash)
            .expect("empty block lookup")
            .is_none()
    );
    assert!(
        provider
            .transaction_by_hash(&hash)
            .expect("empty tx lookup")
            .is_none()
    );
    assert!(
        !provider
            .contains_transaction(&hash)
            .expect("empty contains lookup")
    );
    assert!(
        provider
            .transaction_state_by_hash(&hash)
            .expect("empty state lookup")
            .is_none()
    );
}

#[test]
fn hot_cold_factory_accepts_empty_cold_provider_without_hiding_hot_records() {
    let hot = DataCache::new(false);
    let mut header = Header::new();
    header.set_index(13);
    let block = Block::from_parts(header, vec![]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &block, &block_hash)
        .expect("hot on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&hot, &block_hash, 13)
        .expect("hot post-persist record");

    let factory = HotColdLedgerProviderFactory::new(EmptyLedgerProvider);
    let provider = factory.provider(&hot);
    let missing = UInt256::from_bytes(&[0xCD; 32]).expect("missing hash");

    assert_eq!(
        provider.block_hash_by_index(13).expect("hot hash"),
        Some(block_hash)
    );
    assert_eq!(
        provider.current_hash().expect("hot current hash"),
        block_hash
    );
    assert_eq!(provider.current_index().expect("hot current index"), 13);
    assert_eq!(
        provider
            .block_by_index(13)
            .expect("hot block")
            .map(|b| b.hash()),
        Some(block.hash())
    );
    assert!(
        provider
            .block_by_hash(&missing)
            .expect("clean cold miss")
            .is_none()
    );
}

#[derive(Clone, Debug, Default)]
struct ColdLedgerProvider {
    hashes: HashMap<u32, UInt256>,
    headers: HashMap<UInt256, Header>,
    blocks: HashMap<UInt256, Block>,
    transactions: HashMap<UInt256, Transaction>,
    states: HashMap<UInt256, neo_payloads::TransactionState>,
}

impl ColdLedgerProvider {
    fn with_block(mut self, block: Block) -> Self {
        let block_hash = block.hash();
        self.hashes.insert(block.index(), block_hash);
        self.headers.insert(block_hash, block.header.clone());
        for transaction in &block.transactions {
            let hash = transaction.hash();
            self.transactions.insert(hash, transaction.clone());
            self.states.insert(
                hash,
                neo_payloads::TransactionState::new(
                    block.index(),
                    Some(transaction.clone()),
                    VmState::NONE,
                ),
            );
        }
        self.blocks.insert(block_hash, block);
        self
    }

    fn with_state(mut self, hash: UInt256, state: neo_payloads::TransactionState) -> Self {
        self.states.insert(hash, state);
        self
    }
}

impl BlockProvider for ColdLedgerProvider {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        Ok(self.hashes.get(&index).copied())
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(self.headers.get(hash).cloned())
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        Ok(self.blocks.get(hash).cloned())
    }
}

impl TxProvider for ColdLedgerProvider {
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(self.transactions.get(hash).cloned())
    }
}

impl TransactionStateProvider for ColdLedgerProvider {
    fn transaction_state_by_hash(
        &self,
        hash: &UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        Ok(self.states.get(hash).cloned())
    }

    fn contains_conflict_hash(
        &self,
        _hash: &UInt256,
        _signers: &[UInt160],
        _max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        Ok(false)
    }
}

#[test]
fn hot_cold_ledger_provider_falls_back_to_cold_records() {
    let hot = DataCache::new(false);
    let mut header = Header::new();
    header.set_index(21);
    let tx = test_transaction(2100);
    let tx_hash = tx.hash();
    let cold_block = Block::from_parts(header, vec![tx.clone()]);
    let cold = ColdLedgerProvider::default().with_block(cold_block.clone());
    let provider = HotColdLedgerProvider::new(StorageLedgerProvider::new(&hot), cold);

    let loaded = provider
        .block_by_index(21)
        .expect("cold block read")
        .expect("cold block");
    assert_eq!(loaded.hash(), cold_block.hash());
    assert_eq!(
        provider
            .transaction_by_hash(&tx_hash)
            .expect("cold tx read")
            .expect("cold tx")
            .hash(),
        tx_hash
    );
}

#[test]
fn hot_cold_ledger_provider_prefers_hot_records() {
    let hot = DataCache::new(false);
    let mut hot_header = Header::new();
    hot_header.set_index(31);
    let hot_tx = test_transaction(3100);
    let hot_block = Block::from_parts(hot_header, vec![hot_tx.clone()]);
    let hot_hash = hot_block.header.try_hash().expect("hot block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &hot_block, &hot_hash)
        .expect("hot on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&hot, &hot_hash, 31)
        .expect("hot post-persist record");

    let mut cold_header = Header::new();
    cold_header.set_index(31);
    let cold_block = Block::from_parts(cold_header, vec![test_transaction(3101)]);
    let cold = ColdLedgerProvider::default().with_block(cold_block);
    let factory = HotColdLedgerProviderFactory::new(cold);
    let provider = factory.provider(&hot);

    let loaded = provider
        .block_by_index(31)
        .expect("hot block read")
        .expect("hot block");
    assert_eq!(loaded.hash(), hot_block.hash());
    assert_eq!(loaded.transactions[0].hash(), hot_tx.hash());
}

#[test]
fn storage_ledger_provider_exposes_conflict_stub_state_without_transaction() {
    let cache = DataCache::new(false);
    let conflict_hash = UInt256::from_bytes(&[0xAB; 32]).expect("conflict hash");
    let signer_account = UInt160::from_bytes(&[0x11; 20]).expect("signer");
    let mut tx = test_transaction(4100);
    tx.set_signers(vec![Signer::new(signer_account, WitnessScope::NONE)]);
    tx.set_attributes(vec![TransactionAttribute::Conflicts(
        neo_payloads::Conflicts::new(conflict_hash),
    )]);
    let mut header = Header::new();
    header.set_index(41);
    let block = Block::from_parts(header, vec![tx]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    let provider = StorageLedgerProvider::new(&cache);

    let state = provider
        .transaction_state_by_hash(&conflict_hash)
        .expect("conflict state")
        .expect("conflict stub");
    assert_eq!(state.block_index, 41);
    assert!(state.transaction.is_none());
    assert!(
        !provider
            .contains_transaction(&conflict_hash)
            .expect("stub is not a full transaction")
    );
}

#[test]
fn hot_cold_transaction_state_prefers_hot_conflict_stub_over_cold_transaction() {
    let hot = DataCache::new(false);
    let conflict_hash = UInt256::from_bytes(&[0xBC; 32]).expect("conflict hash");
    let signer_account = UInt160::from_bytes(&[0x44; 20]).expect("signer");
    let mut hot_tx = test_transaction(4200);
    hot_tx.set_signers(vec![Signer::new(signer_account, WitnessScope::NONE)]);
    hot_tx.set_attributes(vec![TransactionAttribute::Conflicts(
        neo_payloads::Conflicts::new(conflict_hash),
    )]);
    let mut header = Header::new();
    header.set_index(42);
    let hot_block = Block::from_parts(header, vec![hot_tx]);
    let hot_hash = hot_block.header.try_hash().expect("hot block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &hot_block, &hot_hash)
        .expect("hot on-persist records");

    let cold = ColdLedgerProvider::default().with_state(
        conflict_hash,
        neo_payloads::TransactionState::new(7, Some(test_transaction(7)), VmState::NONE),
    );
    let provider = HotColdLedgerProvider::new(StorageLedgerProvider::new(&hot), cold);

    let state = provider
        .transaction_state_by_hash(&conflict_hash)
        .expect("routed state")
        .expect("hot stub wins");
    assert_eq!(state.block_index, 42);
    assert!(state.transaction.is_none());
}

#[test]
fn static_ledger_provider_round_trips_full_and_conflict_records() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let cache = DataCache::new(false);
    let conflict_hash = UInt256::from_bytes(&[0xDA; 32]).expect("conflict hash");
    let signer_account = UInt160::from_bytes(&[0x35; 20]).expect("signer");
    let mut tx = test_transaction(5100);
    tx.set_signers(vec![Signer::new(signer_account, WitnessScope::NONE)]);
    tx.set_attributes(vec![TransactionAttribute::Conflicts(
        neo_payloads::Conflicts::new(conflict_hash),
    )]);
    let tx_hash = tx.hash();
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, vec![tx]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    archive
        .append_block(&cache, &block)
        .expect("append ledger record");
    let provider = archive.provider();

    assert_eq!(
        provider.block_hash_by_index(0).expect("hash"),
        Some(block_hash)
    );
    assert_eq!(
        provider
            .block_by_index(0)
            .expect("block")
            .expect("archived block")
            .transactions[0]
            .hash(),
        tx_hash
    );
    assert_eq!(
        provider
            .transaction_state_by_hash(&tx_hash)
            .expect("state")
            .expect("full state")
            .block_index,
        0
    );
    let stub = provider
        .transaction_state_by_hash(&conflict_hash)
        .expect("stub")
        .expect("conflict stub");
    assert_eq!(stub.block_index, 0);
    assert!(stub.transaction.is_none());
    assert!(
        provider
            .contains_conflict_hash(&conflict_hash, &[signer_account], 100)
            .expect("contains conflict")
    );
}

#[test]
fn optional_static_provider_routes_archived_records_when_hot_storage_is_empty() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let populated = DataCache::new(false);
    let tx = test_transaction(5_200);
    let tx_hash = tx.hash();
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, vec![tx]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&populated, &block, &block_hash)
        .expect("on-persist records");

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    archive
        .append_block(&populated, &block)
        .expect("append ledger record");

    let empty_hot = DataCache::new(false);
    let factory = HotColdLedgerProviderFactory::new(OptionalLedgerProvider::from_option(Some(
        archive.provider(),
    )));
    let provider = factory.provider(&empty_hot);

    assert_eq!(
        provider.block_hash_by_index(0).expect("archived hash"),
        Some(block_hash)
    );
    assert_eq!(
        provider
            .block_by_index(0)
            .expect("archived block")
            .expect("block")
            .transactions[0]
            .hash(),
        tx_hash
    );
    assert_eq!(
        provider
            .transaction_state_by_hash(&tx_hash)
            .expect("archived transaction state")
            .expect("transaction state")
            .block_index,
        0
    );
    assert!(
        provider
            .contains_transaction(&tx_hash)
            .expect("archived transaction presence")
    );
}

#[test]
fn static_archive_reconciles_a_missing_durable_hot_prefix() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let cache = DataCache::new(false);
    let mut blocks = Vec::new();
    for height in 0..=2 {
        let mut header = Header::new();
        header.set_index(height);
        let block = Block::from_parts(header, vec![test_transaction(6_000 + height)]);
        let hash = block.header.try_hash().expect("block hash");
        crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &hash)
            .expect("on-persist records");
        crate::ledger_records::LedgerRecords::write_post_persist_record(&cache, &hash, height)
            .expect("post-persist record");
        blocks.push(block);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    archive
        .append_block(&cache, &blocks[0])
        .expect("seed archive");

    let recovery = archive
        .reconcile(&cache, Some(2), None, 2)
        .expect("reconcile archive");

    assert_eq!(recovery.appended_blocks, 2);
    assert_eq!(archive.tip(), Some(2));
    assert_eq!(
        archive
            .provider()
            .block_by_index(2)
            .expect("block")
            .expect("archived block")
            .hash(),
        blocks[2].hash()
    );
}

#[test]
fn static_archive_reconcile_rolls_back_persistent_row_versions_above_hot_tip() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let hot = DataCache::new(false);
    let archived = DataCache::new(false);
    let mut blocks = Vec::new();
    for height in 0..=2 {
        let mut header = Header::new();
        header.set_index(height);
        let block = Block::from_parts(header, vec![test_transaction(6_100 + height)]);
        let hash = block.header.try_hash().expect("block hash");
        crate::ledger_records::LedgerRecords::write_on_persist_records(&archived, &block, &hash)
            .expect("archived on-persist records");
        crate::ledger_records::LedgerRecords::write_post_persist_record(&archived, &hash, height)
            .expect("archived post-persist record");
        if height <= 1 {
            crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &block, &hash)
                .expect("hot on-persist records");
            crate::ledger_records::LedgerRecords::write_post_persist_record(&hot, &hash, height)
                .expect("hot post-persist record");
        }
        blocks.push(block);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    for block in &blocks {
        archive
            .append_block(&archived, block)
            .expect("append archived block");
    }

    let recovery = archive
        .reconcile(&hot, Some(1), None, 2)
        .expect("truncate ahead archive");

    assert_eq!(recovery.truncated_blocks, 1);
    assert_eq!(recovery.final_tip, Some(1));
    assert_eq!(archive.tip(), Some(1));
    assert!(
        archive
            .provider()
            .block_by_index(2)
            .expect("block lookup")
            .is_none()
    );
    archive.files().scrub().expect("archive/index parity");
}

#[test]
fn static_archive_rejects_a_fork_before_truncating_an_ahead_tail() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let hot = DataCache::new(false);
    let archived = DataCache::new(false);
    let mut archived_blocks = Vec::new();
    for height in 0..=2 {
        let mut hot_header = Header::new();
        hot_header.set_index(height);
        hot_header.set_nonce(u64::from(height));
        let hot_block = Block::from_parts(hot_header, vec![]);
        let hot_hash = hot_block.header.try_hash().expect("hot block hash");
        if height <= 1 {
            crate::ledger_records::LedgerRecords::write_on_persist_records(
                &hot, &hot_block, &hot_hash,
            )
            .expect("hot block records");
            crate::ledger_records::LedgerRecords::write_post_persist_record(
                &hot, &hot_hash, height,
            )
            .expect("hot current block");
        }

        let archived_block = if height == 1 {
            let mut divergent_header = hot_block.header.clone();
            divergent_header.set_nonce(10_001);
            Block::from_parts(divergent_header, vec![])
        } else {
            hot_block.clone()
        };
        let archived_hash = archived_block
            .header
            .try_hash()
            .expect("archived block hash");
        crate::ledger_records::LedgerRecords::write_on_persist_records(
            &archived,
            &archived_block,
            &archived_hash,
        )
        .expect("archived block records");
        crate::ledger_records::LedgerRecords::write_post_persist_record(
            &archived,
            &archived_hash,
            height,
        )
        .expect("archived current block");
        archived_blocks.push(archived_block);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    for block in &archived_blocks {
        archive
            .append_block(&archived, block)
            .expect("append archived block");
    }
    assert_eq!(archive.tip(), Some(2));

    let error = archive
        .reconcile(&hot, Some(1), None, 2)
        .expect_err("interior fork must be rejected");
    assert!(
        error.to_string().contains("height 1"),
        "unexpected error: {error}"
    );
    assert_eq!(
        archive.tip(),
        Some(2),
        "fork validation must precede destructive tail repair"
    );
}

#[test]
fn static_archive_reconciliation_starts_after_the_hot_prune_watermark() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let archived = DataCache::new(false);
    let hot = DataCache::new(false);
    let mut blocks = Vec::new();
    for height in 0..=2 {
        let mut header = Header::new();
        header.set_index(height);
        let block = Block::from_parts(header, vec![test_transaction(6_200 + height)]);
        let hash = block.hash();
        crate::ledger_records::LedgerRecords::write_on_persist_records(&archived, &block, &hash)
            .expect("archived rows");
        crate::ledger_records::LedgerRecords::write_post_persist_record(&archived, &hash, height)
            .expect("archived tip");
        if height == 2 {
            crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &block, &hash)
                .expect("retained hot rows");
            crate::ledger_records::LedgerRecords::write_post_persist_record(&hot, &hash, height)
                .expect("hot tip");
        }
        blocks.push(block);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    for block in &blocks {
        archive
            .append_block(&archived, block)
            .expect("append archived block");
    }

    let recovery = archive
        .reconcile(&hot, Some(2), Some(1), 2)
        .expect("reconcile retained hot suffix");

    assert_eq!(recovery.hot_pruned_through, Some(1));
    assert_eq!(recovery.final_tip, Some(2));
    assert_eq!(recovery.appended_blocks, 0);
    assert_eq!(recovery.truncated_blocks, 0);
}

#[test]
fn static_archive_reconciliation_rejects_archive_lag_below_hot_prune_watermark() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let hot = DataCache::new(false);
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, vec![]);
    let hash = block.hash();
    crate::ledger_records::LedgerRecords::write_on_persist_records(&hot, &block, &hash)
        .expect("hot rows");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&hot, &hash, 2)
        .expect("canonical tip");

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    archive.append_block(&hot, &block).expect("archive block 0");

    let error = archive
        .reconcile(&hot, Some(2), Some(1), 2)
        .expect_err("archive below prune watermark must fail");

    assert!(error.to_string().contains("does not cover"), "{error}");
    assert_eq!(archive.tip(), Some(0));
}

#[test]
fn hot_pruning_keeps_newer_overwrites_and_current_block_until_their_frontier() {
    use std::sync::Arc;

    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_storage::mdbx::MdbxStoreProvider;

    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: temp.path().join("hot"),
            ..Default::default()
        })
        .get_mdbx_store(std::path::Path::new(""))
        .expect("MDBX store"),
    );
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    let conflict_hash = UInt256::from_bytes(&[0xD7; 32]).expect("conflict hash");
    let signer = UInt160::from_bytes(&[0x31; 20]).expect("signer");
    let mut blocks = Vec::new();

    for height in 0..=2 {
        let mut tx = test_transaction(7_000 + height);
        if height != 1 {
            tx.set_signers(vec![Signer::new(signer, WitnessScope::NONE)]);
            tx.set_attributes(vec![TransactionAttribute::Conflicts(
                neo_payloads::Conflicts::new(conflict_hash),
            )]);
        }
        let mut header = Header::new();
        header.set_index(height);
        let block = Block::from_parts(header, vec![tx]);
        let block_hash = block.header.try_hash().expect("block hash");
        let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
        crate::ledger_records::LedgerRecords::write_on_persist_records(
            writer.data_cache(),
            &block,
            &block_hash,
        )
        .expect("on-persist records");
        crate::ledger_records::LedgerRecords::write_post_persist_record(
            writer.data_cache(),
            &block_hash,
            height,
        )
        .expect("post-persist record");
        archive
            .append_block(writer.data_cache(), &block)
            .expect("archive block");
        writer.try_commit_durable().expect("commit hot Ledger");
        blocks.push(block);
    }

    let first_tx_hash = blocks[0].transactions[0].hash();
    let first_block_hash = blocks[0].hash();
    let conflict_key = crate::ledger_records::LedgerRecords::transaction_key(&conflict_hash);
    let first = archive
        .prune_hot_through(store.as_ref(), 0, 64)
        .expect("prune first archived height");

    assert_eq!(first.pruned_through, Some(0));
    assert_eq!(first.processed_frames, 1);
    assert!(
        store
            .try_get_bytes(&crate::ledger_records::LedgerRecords::block_hash_key(0).to_array())
            .is_none()
    );
    assert!(
        store
            .try_get_bytes(
                &crate::ledger_records::LedgerRecords::block_key(&first_block_hash).to_array()
            )
            .is_none()
    );
    assert!(
        store
            .try_get_bytes(
                &crate::ledger_records::LedgerRecords::transaction_key(&first_tx_hash).to_array()
            )
            .is_none()
    );
    assert!(
        store.try_get_bytes(&conflict_key.to_array()).is_some(),
        "a conflict key overwritten at height 2 must survive frontier 0"
    );

    let hot = StoreCache::new_from_store(Arc::clone(&store), true);
    assert_eq!(
        StorageLedgerProvider::new(hot.data_cache())
            .current_index()
            .expect("current block stays hot"),
        2
    );
    let routed = HotColdLedgerProvider::new(
        StorageLedgerProvider::new(hot.data_cache()),
        archive.provider(),
    );
    assert_eq!(
        routed
            .block_by_index(0)
            .expect("cold block lookup")
            .expect("archived block")
            .hash(),
        first_block_hash
    );

    let final_outcome = archive
        .prune_hot_through(store.as_ref(), 2, 64)
        .expect("prune remaining archived heights");
    assert_eq!(final_outcome.previous_watermark, Some(0));
    assert_eq!(final_outcome.pruned_through, Some(2));
    assert!(store.try_get_bytes(&conflict_key.to_array()).is_none());
    assert_eq!(archive.hot_pruned_through(store.as_ref()).unwrap(), Some(2));
    assert_eq!(
        StorageLedgerProvider::new(hot.data_cache())
            .current_index()
            .expect("current block remains after full archive prune"),
        2
    );
}

#[test]
fn hot_pruning_rejects_hot_cold_byte_mismatch_without_advancing_watermark() {
    use std::sync::Arc;

    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_storage::mdbx::MdbxStoreProvider;

    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: temp.path().join("hot-mismatch"),
            ..Default::default()
        })
        .get_mdbx_store(std::path::Path::new(""))
        .expect("MDBX store"),
    );
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("mismatch.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, vec![]);
    let block_hash = block.hash();
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    crate::ledger_records::LedgerRecords::write_on_persist_records(
        writer.data_cache(),
        &block,
        &block_hash,
    )
    .expect("on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(
        writer.data_cache(),
        &block_hash,
        0,
    )
    .expect("post-persist record");
    archive
        .append_block(writer.data_cache(), &block)
        .expect("archive block");
    writer.try_commit_durable().expect("commit hot Ledger");

    let block_key = crate::ledger_records::LedgerRecords::block_key(&block_hash).to_array();
    assert!(
        store
            .try_commit_raw_overlay(&[(block_key, Some(b"corrupt".to_vec()))])
            .expect("corrupt hot row")
    );
    let error = archive
        .prune_hot_through(store.as_ref(), 0, 64)
        .expect_err("mismatch must stop pruning");

    assert!(error.to_string().contains("mismatch"), "{error}");
    assert_eq!(archive.hot_pruned_through(store.as_ref()).unwrap(), None);
}

#[test]
fn hot_prune_watermark_rejects_malformed_typed_value() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_storage::persistence::StoreMaintenanceBatch;
    use neo_storage::persistence::providers::MemoryStore;

    let temp = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("watermark-corruption.static"))
        .expect("archive");
    let archive = StaticLedgerArchive::new(files);
    let store = MemoryStore::new();
    let mut corruption = StoreMaintenanceBatch::new();
    corruption.put_metadata(
        b"neo.ledger.hot-pruned-through.v1".to_vec(),
        vec![0x01, 0x02],
    );
    assert!(
        store
            .try_commit_durable_maintenance(&corruption)
            .expect("inject malformed prune watermark")
    );

    let error = archive
        .hot_pruned_through(&store)
        .expect_err("malformed typed watermark must fail closed");
    assert!(
        error.to_string().contains("HotLedgerPruneWatermark"),
        "typed table identity missing from error: {error}"
    );
    assert!(
        error.to_string().contains("4-byte big-endian u32"),
        "typed codec detail missing from error: {error}"
    );
}
