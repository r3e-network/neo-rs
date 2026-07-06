use super::*;

use std::collections::HashMap;

use neo_io::Serializable;
use neo_payloads::{Block, Header, Signer, Transaction, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_storage::DataCache;

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

#[derive(Clone, Debug, Default)]
struct ColdLedgerProvider {
    hashes: HashMap<u32, UInt256>,
    headers: HashMap<UInt256, Header>,
    blocks: HashMap<UInt256, Block>,
    transactions: HashMap<UInt256, Transaction>,
}

impl ColdLedgerProvider {
    fn with_block(mut self, block: Block) -> Self {
        let block_hash = block.hash();
        self.hashes.insert(block.index(), block_hash);
        self.headers.insert(block_hash, block.header.clone());
        for transaction in &block.transactions {
            self.transactions
                .insert(transaction.hash(), transaction.clone());
        }
        self.blocks.insert(block_hash, block);
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
