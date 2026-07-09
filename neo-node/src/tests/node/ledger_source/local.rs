use super::LedgerBlockSource;
use neo_network::BlockSource;
use neo_payloads::{Block, Header, Witness};
use neo_primitives::UInt256;
use std::sync::Arc;

fn empty_child_block(parent: &Block, index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(parent.hash());
    header.set_timestamp(parent.header.timestamp() + 15_000);
    header.set_next_consensus(*parent.header.next_consensus());
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    Block::from_parts(header, Vec::new())
}

fn local_source_with_block(index: u32) -> (LedgerBlockSource, Block, UInt256) {
    let settings = neo_config::ProtocolSettings::default();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let resources = neo_blockchain::NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));

    let genesis = Arc::new(neo_blockchain::genesis_block(&settings).expect("genesis block"));
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        &settings,
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist genesis");

    let block = empty_child_block(genesis.as_ref(), index);
    let block_hash = block.hash();
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::new(block.clone()),
        &settings,
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist child");

    let source = LedgerBlockSource::new(
        snapshot,
        Arc::new(neo_blockchain::LedgerContext::default()),
        Arc::new(neo_mempool::MemoryPool::new(&settings)),
    );
    (source, block, block_hash)
}

#[test]
fn local_ledger_block_source_reads_persisted_block_through_provider() {
    let (source, block, block_hash) = local_source_with_block(1);

    assert_eq!(source.block_hash_by_index(1), Some(block_hash));
    assert_eq!(source.block_index_by_hash(&block_hash), Some(1));

    let fetched_block = source.block_by_index(1).expect("block by index");
    assert_eq!(fetched_block.hash(), block_hash);
    assert_eq!(fetched_block.transactions.len(), 0);

    let fetched_by_hash = source.block_by_hash(&block_hash).expect("block by hash");
    assert_eq!(fetched_by_hash.hash(), block_hash);

    let fetched_header = source.header_by_index(1).expect("header by index");
    assert_eq!(fetched_header.hash(), block.header.hash());
}

#[test]
fn local_ledger_block_source_reports_miss_for_unknown_records() {
    let (source, _, _) = local_source_with_block(1);
    let missing = UInt256::from([0xEE; 32]);

    assert!(source.block_by_index(99).is_none());
    assert!(source.header_by_index(99).is_none());
    assert!(source.block_by_hash(&missing).is_none());
    assert!(source.block_index_by_hash(&missing).is_none());
    assert!(!source.contains_transaction(&missing));
}

#[test]
fn local_ledger_block_source_uses_hot_cold_provider_factory_shape() {
    let source = include_str!("../../../node/ledger_source/local.rs");

    assert!(
        source.contains("HotColdLedgerProviderFactory"),
        "local block serving should use the hot/cold ledger provider factory shape"
    );
    assert!(
        source.contains("EmptyLedgerProvider"),
        "local block serving should use the explicit empty cold provider until static files are installed"
    );
    assert!(
        !source.contains("StorageLedgerProviderFactory"),
        "local block serving should not bypass the hot/cold provider boundary"
    );
}

#[test]
fn operational_ledger_tip_reads_stay_behind_local_provider_boundary() {
    let sources = [
        (
            "node composition",
            include_str!("../../../node/composition.rs"),
        ),
        (
            "config validation",
            include_str!("../../../node/config/validation.rs"),
        ),
        (
            "chain.acc driver",
            include_str!("../../../node/chain_acc/driver.rs"),
        ),
        (
            "daemon system context",
            include_str!("../../../node/context/system_context.rs"),
        ),
    ];

    for (name, source) in sources {
        assert!(
            !source.contains("StorageLedgerProviderFactory"),
            "{name} must use the node local-ledger provider boundary for durable tip reads"
        );
    }

    let provider_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/node/ledger_source/tip.rs");
    let provider = std::fs::read_to_string(&provider_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", provider_path.display()));
    assert!(
        provider.contains("HotColdLedgerProviderFactory"),
        "operational ledger-tip reads should use the hot/cold ledger provider factory shape"
    );
    assert!(
        provider.contains("EmptyLedgerProvider"),
        "operational ledger-tip reads should use the explicit empty cold provider until static files are installed"
    );
    assert!(
        !provider.contains("StorageLedgerProviderFactory"),
        "operational ledger-tip reads should not bypass the hot/cold provider boundary"
    );
    assert!(
        provider.contains("StoreCache::new_from_store"),
        "durable store tip reads should be centralized behind the same store-cache snapshot path"
    );
}
