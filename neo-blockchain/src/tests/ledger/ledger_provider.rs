use super::*;

use std::collections::HashMap;

use neo_error::CoreResult;
use neo_io::Serializable;
use neo_payloads::{Block, Header, Signer, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_storage::DataCache;
use neo_vm_rs::VmState;

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
