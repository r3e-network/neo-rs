use super::*;

use crate::{LedgerProviderFactory, StorageLedgerProviderFactory};
use neo_io::Serializable;
use neo_payloads::{Block, Header, Signer, Transaction, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_storage::DataCache;
use std::sync::Arc;

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
fn storage_ledger_provider_factory_creates_latest_provider() {
    let cache = Arc::new(DataCache::new(false));
    let mut header = Header::new();
    header.set_index(11);
    let tx = test_transaction(123);
    let block = Block::from_parts(header, vec![tx]);
    let block_hash = block.header.try_hash().expect("block hash");
    crate::ledger_records::LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    crate::ledger_records::LedgerRecords::write_post_persist_record(&cache, &block_hash, 11)
        .expect("post-persist record");

    let factory = StorageLedgerProviderFactory::new(Arc::clone(&cache));
    let provider = LedgerProviderFactory::latest(&factory).expect("latest provider");

    assert_eq!(
        BlockProvider::block_by_index(&provider, 11)
            .expect("block by index")
            .expect("block present")
            .hash(),
        block.hash()
    );
}
