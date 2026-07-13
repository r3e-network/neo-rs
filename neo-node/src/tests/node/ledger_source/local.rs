use super::LedgerBlockSource;
use neo_native_contracts::ledger_contract::storage::{PREFIX_BLOCK, PREFIX_CURRENT_BLOCK};
use neo_network::BlockSource;
use neo_payloads::{Block, Header, Witness};
use neo_primitives::UInt256;
use neo_storage::{StorageItem, StorageKey};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn empty_child_block(parent: &Block, index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(parent.hash());
    header.set_timestamp(parent.header.timestamp() + 15_000);
    header.set_next_consensus(*parent.header.next_consensus());
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    Block::from_parts(header, Vec::new())
}

fn local_source_with_block(
    index: u32,
) -> (
    LedgerBlockSource,
    Block,
    UInt256,
    Arc<neo_storage::DataCache>,
    CancellationToken,
) {
    let settings = neo_config::ProtocolSettings::default();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let resources = neo_blockchain::NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));

    let genesis = Arc::new(neo_blockchain::genesis_block(&settings).expect("genesis block"));
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist genesis");

    let block = empty_child_block(genesis.as_ref(), index);
    let block_hash = block.hash();
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::new(block.clone()),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist child");

    let shutdown = CancellationToken::new();
    let source = LedgerBlockSource::new(
        Arc::clone(&snapshot),
        Arc::new(neo_blockchain::LedgerContext::default()),
        Arc::new(neo_mempool::MemoryPool::new_with_native_contract_provider(
            &settings,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        )),
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            shutdown.clone(),
        )),
    );
    (source, block, block_hash, snapshot, shutdown)
}

#[test]
fn local_ledger_block_source_reads_persisted_block_through_provider() {
    let (source, block, block_hash, _, _) = local_source_with_block(1);

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
    let (source, _, _, _, shutdown) = local_source_with_block(1);
    let missing = UInt256::from([0xEE; 32]);

    assert!(source.block_by_index(99).is_none());
    assert!(source.header_by_index(99).is_none());
    assert!(source.block_by_hash(&missing).is_none());
    assert!(source.block_index_by_hash(&missing).is_none());
    assert!(!source.contains_transaction(&missing));
    assert!(
        !shutdown.is_cancelled(),
        "clean local-ledger misses must not request restart"
    );
}

#[test]
fn local_ledger_block_source_requests_restart_on_hot_provider_corruption() {
    let (source, _, block_hash, snapshot, shutdown) = local_source_with_block(1);
    let block_key = StorageKey::create_with_uint256(
        neo_native_contracts::LedgerContract::ID,
        PREFIX_BLOCK,
        &block_hash,
    );
    snapshot
        .try_update(block_key, StorageItem::from_bytes(vec![0xFF]))
        .expect("corrupt block record");

    assert!(source.block_by_index(1).is_none());
    assert!(
        shutdown.is_cancelled(),
        "local provider corruption must request supervised shutdown"
    );
    assert_eq!(
        source.block_hash_by_index(1),
        None,
        "a failed provider must stop serving even independently readable records"
    );
}

#[test]
fn local_ledger_block_source_requests_restart_on_static_archive_io_error() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let settings = neo_config::ProtocolSettings::default();
    let populated = Arc::new(neo_storage::DataCache::new(false));
    let resources = neo_blockchain::NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));
    let genesis = Arc::new(neo_blockchain::genesis_block(&settings).expect("genesis block"));
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&populated),
        Arc::clone(&genesis),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist genesis");
    let child = empty_child_block(genesis.as_ref(), 1);
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&populated),
        Arc::new(child.clone()),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist child");

    let temp = tempfile::tempdir().expect("tempdir");
    let archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&temp.path().join("ledger.static"))
            .expect("archive"),
    );
    archive
        .append_block(populated.as_ref(), genesis.as_ref())
        .expect("archive genesis");
    archive
        .append_block(populated.as_ref(), &child)
        .expect("archive child");

    let shutdown = CancellationToken::new();
    let source = LedgerBlockSource::new(
        Arc::new(neo_storage::DataCache::new(false)),
        Arc::new(neo_blockchain::LedgerContext::default()),
        Arc::new(neo_mempool::MemoryPool::new_with_native_contract_provider(
            &settings,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        )),
        Some(archive.clone()),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            shutdown.clone(),
        )),
    );

    std::fs::OpenOptions::new()
        .write(true)
        .open(archive.files().path())
        .expect("open archive")
        .set_len(0)
        .expect("truncate archive");

    assert!(source.block_by_index(1).is_none());
    assert!(
        shutdown.is_cancelled(),
        "cold archive I/O failures must request supervised shutdown"
    );
}

#[test]
fn local_ledger_block_source_falls_back_to_configured_static_files() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};

    let settings = neo_config::ProtocolSettings::default();
    let populated = Arc::new(neo_storage::DataCache::new(false));
    let resources = neo_blockchain::NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));
    let genesis = Arc::new(neo_blockchain::genesis_block(&settings).expect("genesis block"));
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&populated),
        Arc::clone(&genesis),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist genesis");
    let child = empty_child_block(genesis.as_ref(), 1);
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&populated),
        Arc::new(child.clone()),
        Arc::new(settings.clone()),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist child");

    let temp = tempfile::tempdir().expect("tempdir");
    let archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&temp.path().join("ledger.static"))
            .expect("archive"),
    );
    archive
        .append_block(populated.as_ref(), genesis.as_ref())
        .expect("archive genesis");
    archive
        .append_block(populated.as_ref(), &child)
        .expect("archive child");

    let shutdown = CancellationToken::new();
    let source = LedgerBlockSource::new(
        Arc::new(neo_storage::DataCache::new(false)),
        Arc::new(neo_blockchain::LedgerContext::default()),
        Arc::new(neo_mempool::MemoryPool::new_with_native_contract_provider(
            &settings,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        )),
        Some(archive),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(None, shutdown)),
    );

    assert_eq!(source.block_hash_by_index(1), Some(child.hash()));
    assert_eq!(
        source.block_by_index(1).expect("static child").hash(),
        child.hash()
    );
}

#[test]
fn local_ledger_block_source_uses_hot_cold_provider_factory_shape() {
    let source = include_str!("../../../node/ledger_source/local.rs");

    assert!(
        source.contains("HotColdLedgerProviderFactory"),
        "local block serving should route hot records and configured static files through one provider"
    );
    assert!(
        source.contains("StaticLedgerProvider"),
        "local block serving should install the production static-file provider when configured"
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
            include_str!("../../../node/lifecycle/composition.rs"),
        ),
        (
            "config validation",
            include_str!("../../../node/config/validation.rs"),
        ),
        (
            "chain.acc driver",
            include_str!("../../../node/chain_acc/driver.rs"),
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

    let system_context_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("neo-system/src/composition/system_context.rs");
    let system_context = std::fs::read_to_string(&system_context_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", system_context_path.display()));
    assert!(
        system_context.contains("HotColdLedgerProviderFactory"),
        "the composition-owned system context should read the canonical tip through the hot/cold provider"
    );
    assert!(
        provider.contains("EmptyLedgerProvider"),
        "current-tip metadata is always hot and should retain an explicit clean-miss cold provider"
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

#[test]
fn durable_tip_read_distinguishes_uninitialized_and_corrupt_current_block() {
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let empty = Arc::new(MemoryStore::new());
    assert_eq!(
        crate::node::ledger_source::tip::store_ledger_index(&empty, false)
            .expect("empty store tip"),
        None
    );

    let corrupt = Arc::new(MemoryStore::new());
    let mut writer = StoreCache::new_from_store(Arc::clone(&corrupt), false);
    writer.add(
        StorageKey::new(
            neo_native_contracts::LedgerContract::ID,
            vec![PREFIX_CURRENT_BLOCK],
        ),
        StorageItem::from_bytes(vec![0xff]),
    );
    writer.try_commit().expect("commit malformed tip");

    let error = crate::node::ledger_source::tip::store_ledger_index(&corrupt, false)
        .expect_err("malformed current block must fail startup");
    assert!(
        error.to_string().contains("persisted ledger tip"),
        "{error}"
    );
}
