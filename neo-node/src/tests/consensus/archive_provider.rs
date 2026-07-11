//! Consensus reads through the configured static Ledger fallback.

use std::sync::Arc;

use neo_blockchain::{
    HotColdLedgerProviderFactory, NativePersistOptions, NativePersistResources,
    OptionalStaticLedgerProvider, StaticLedgerArchiveFactory, genesis_block,
    persist_block_natives_with_resources,
};
use neo_native_contracts::LedgerContract;
use neo_native_contracts::ledger_contract::storage::{PREFIX_BLOCK, PREFIX_TRANSACTION};
use neo_payloads::{Block, Header};
use neo_storage::{DataCache, StorageKey};

use super::super::native_provider::{ConsensusNativeProvider, NativeConsensusProvider};
use super::*;

#[test]
fn consensus_reads_tip_header_and_transaction_from_configured_archive() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let native_contract_provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());
    let resources = NativePersistResources::from_provider(Arc::clone(&native_contract_provider));
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        &settings,
        NativePersistOptions::default(),
        &resources,
    )
    .expect("persist genesis");

    let transaction = signed_zero_fee_tx(&settings, 0xA1);
    let transaction_hash = transaction.hash();
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(123);
    header.set_next_consensus(*genesis.header.next_consensus());
    let mut block = Block::from_parts(header, vec![transaction]);
    block.try_rebuild_merkle_root().expect("merkle root");
    let block = Arc::new(block);
    let block_hash = block.hash();
    persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&block),
        &settings,
        NativePersistOptions::default(),
        &resources,
    )
    .expect("persist child block");

    let directory = tempfile::tempdir().expect("archive directory");
    let archive = StaticLedgerArchiveFactory::default()
        .open(directory.path().join("ledger.static"))
        .expect("open Ledger archive");
    archive
        .append_block(snapshot.as_ref(), genesis.as_ref())
        .expect("archive genesis");
    archive
        .append_block(snapshot.as_ref(), block.as_ref())
        .expect("archive child block");

    snapshot.delete(&StorageKey::create_with_uint256(
        LedgerContract::ID,
        PREFIX_BLOCK,
        &block_hash,
    ));
    snapshot.delete(&StorageKey::create_with_uint256(
        LedgerContract::ID,
        PREFIX_TRANSACTION,
        &transaction_hash,
    ));

    let cold = OptionalStaticLedgerProvider::from_option(Some(archive.provider()));
    let provider = NativeConsensusProvider::new(
        native_contract_provider,
        HotColdLedgerProviderFactory::new(cold),
    );
    let tip = provider.ledger_tip(snapshot.as_ref());

    assert_eq!(tip.next_block_index, 2);
    assert_eq!(tip.prev_hash, block_hash);
    assert_eq!(tip.prev_timestamp, 123);
    assert!(
        provider
            .contains_transaction(snapshot.as_ref(), &transaction_hash)
            .expect("archived transaction lookup")
    );
}
