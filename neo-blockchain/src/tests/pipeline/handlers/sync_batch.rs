use super::*;

#[derive(Clone)]
struct SyncBatchProbe {
    snapshot_calls: Arc<AtomicUsize>,
    commit_calls: Arc<AtomicUsize>,
    flush_calls: Arc<AtomicUsize>,
    artifact_lengths: Arc<parking_lot::Mutex<Vec<usize>>>,
    committing_contexts: Arc<parking_lot::Mutex<Vec<crate::service_context::BlockPersistContext>>>,
    committed_contexts: Arc<parking_lot::Mutex<Vec<crate::service_context::BlockPersistContext>>>,
    committed_heights: Arc<parking_lot::Mutex<Vec<u32>>>,
    commit_counts_at_committed: Arc<parking_lot::Mutex<Vec<usize>>>,
    reverify_calls: Arc<AtomicUsize>,
    block_persisted_calls: Arc<AtomicUsize>,
}

impl Default for SyncBatchProbe {
    fn default() -> Self {
        Self {
            snapshot_calls: Arc::new(AtomicUsize::new(0)),
            commit_calls: Arc::new(AtomicUsize::new(0)),
            flush_calls: Arc::new(AtomicUsize::new(0)),
            artifact_lengths: Arc::new(parking_lot::Mutex::new(Vec::new())),
            committing_contexts: Arc::new(parking_lot::Mutex::new(Vec::new())),
            committed_contexts: Arc::new(parking_lot::Mutex::new(Vec::new())),
            committed_heights: Arc::new(parking_lot::Mutex::new(Vec::new())),
            commit_counts_at_committed: Arc::new(parking_lot::Mutex::new(Vec::new())),
            reverify_calls: Arc::new(AtomicUsize::new(0)),
            block_persisted_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl SyncBatchProbe {
    fn reset(&self) {
        self.snapshot_calls.store(0, Ordering::SeqCst);
        self.commit_calls.store(0, Ordering::SeqCst);
        self.flush_calls.store(0, Ordering::SeqCst);
        self.artifact_lengths.lock().clear();
        self.committing_contexts.lock().clear();
        self.committed_contexts.lock().clear();
        self.committed_heights.lock().clear();
        self.commit_counts_at_committed.lock().clear();
        self.reverify_calls.store(0, Ordering::SeqCst);
        self.block_persisted_calls.store(0, Ordering::SeqCst);
    }
}

struct VerifiedSyncBatchContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    probe: SyncBatchProbe,
    fail_flush: bool,
    sync_batch_policy: crate::SyncBatchCommitPolicy,
}

impl std::fmt::Debug for VerifiedSyncBatchContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedSyncBatchContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for VerifiedSyncBatchContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        self.probe.snapshot_calls.fetch_add(1, Ordering::SeqCst);
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn block_committing_with_context(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache,
        application_executed: &[neo_payloads::ApplicationExecuted],
        context: crate::service_context::BlockPersistContext,
    ) -> bool {
        self.probe
            .artifact_lengths
            .lock()
            .push(application_executed.len());
        self.probe.committing_contexts.lock().push(context);
        true
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.probe.commit_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn abort_store_commit(&self) {
        self.snapshot.reset();
    }

    fn flush_deferred_commit_handlers(&self) -> Result<(), String> {
        self.probe.flush_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_flush {
            Err("state-root worker reported a failed operation".to_string())
        } else {
            Ok(())
        }
    }

    async fn block_finalized(
        &self,
        finalized: crate::FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        assert!(!finalized.context().is_trusted_replay());
        self.probe
            .committed_contexts
            .lock()
            .push(finalized.context());
        self.probe
            .commit_counts_at_committed
            .lock()
            .push(self.probe.commit_calls.load(Ordering::SeqCst));
        self.probe
            .committed_heights
            .lock()
            .push(finalized.block().index());
        Ok(())
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        self.sync_batch_policy
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }
}

struct SyncBatchFixture {
    service: BlockchainService<VerifiedSyncBatchContext, RecordingMempool>,
    handle: BlockchainHandle,
    snapshot: Arc<neo_storage::DataCache>,
    probe: SyncBatchProbe,
}

impl SyncBatchFixture {
    async fn new(
        settings: neo_config::ProtocolSettings,
        sync_batch_policy: crate::SyncBatchCommitPolicy,
        fail_flush: bool,
    ) -> Self {
        let snapshot = Arc::new(neo_storage::DataCache::new(false));
        let probe = SyncBatchProbe::default();
        let system = Arc::new(VerifiedSyncBatchContext {
            snapshot: Arc::clone(&snapshot),
            settings: Arc::new(settings),
            probe: probe.clone(),
            fail_flush,
            sync_batch_policy,
        });
        let mempool = Arc::new(RecordingMempool {
            reverify_calls: Arc::clone(&probe.reverify_calls),
            has_unverified_transactions: true,
            block_persisted_calls: Some(Arc::clone(&probe.block_persisted_calls)),
        });
        let (service, handle) = BlockchainService::with_defaults(
            system,
            Arc::new(LedgerContext::default()),
            Arc::new(HeaderCache::default()),
            mempool,
        );
        service.initialize().await.expect("initialize");
        probe.reset();
        Self {
            service,
            handle,
            snapshot,
            probe,
        }
    }
}

struct SignedSyncBatch {
    settings: neo_config::ProtocolSettings,
    blocks: Vec<Block>,
    hashes: [UInt256; 2],
    timestamps: [u64; 2],
}

fn signed_sync_batch() -> SignedSyncBatch {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let timestamps = [
        genesis.header.timestamp() + 15_000,
        genesis.header.timestamp() + 30_000,
    ];

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(timestamps[0]);
    header1.set_primary_index(0);
    header1.set_next_consensus(*genesis.header.next_consensus());
    header1.witness = sign_header_for_test(&header1, settings.network, &private_key, &verification);
    let block1 = Block::from_parts(header1, Vec::new());
    let block1_hash = block1.try_hash().expect("block 1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(timestamps[1]);
    header2.set_primary_index(0);
    header2.set_next_consensus(*genesis.header.next_consensus());
    header2.witness = sign_header_for_test(&header2, settings.network, &private_key, &verification);
    let block2 = Block::from_parts(header2, Vec::new());
    let block2_hash = block2.try_hash().expect("block 2 hash");

    SignedSyncBatch {
        settings,
        blocks: vec![block1, block2],
        hashes: [block1_hash, block2_hash],
        timestamps,
    }
}

#[tokio::test]
async fn verified_sync_batch_commits_once_and_replays_live_side_effects_after_durability() {
    let batch = signed_sync_batch();
    let fixture = SyncBatchFixture::new(
        batch.settings,
        crate::SyncBatchCommitPolicy::DeferredLive,
        false,
    )
    .await;
    for block in &batch.blocks {
        assert!(fixture.service.header_cache.add(block.header.clone()));
    }
    let mut events = fixture.handle.subscribe();

    let reply = fixture
        .service
        .handle_import(Import {
            blocks: batch.blocks,
            mode: ImportMode::Sync,
        })
        .await;

    assert_eq!(reply.imported, 2);
    assert_eq!(reply.error, None);
    assert_eq!(fixture.service.ledger.current_height(), 2);
    assert_eq!(fixture.probe.commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.flush_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.snapshot_calls.load(Ordering::SeqCst), 2);
    assert!(
        fixture
            .probe
            .artifact_lengths
            .lock()
            .iter()
            .all(|length| *length > 0)
    );
    assert_eq!(
        fixture.probe.committing_contexts.lock().as_slice(),
        &[
            crate::service_context::BlockPersistContext::sync_batch(),
            crate::service_context::BlockPersistContext::sync_batch(),
        ],
        "verified deferred sync must freeze pre-commit observer semantics for the whole batch"
    );
    assert_eq!(fixture.probe.committed_heights.lock().as_slice(), &[1, 2]);
    assert_eq!(
        fixture.probe.commit_counts_at_committed.lock().as_slice(),
        &[1, 1]
    );
    assert_eq!(
        fixture.probe.block_persisted_calls.load(Ordering::SeqCst),
        2
    );
    assert_eq!(fixture.probe.reverify_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.service.header_cache.count(), 0);
    for (position, height) in [1_u32, 2].into_iter().enumerate() {
        assert_eq!(
            events.try_recv().expect("ordered import event"),
            crate::RuntimeEvent::Imported {
                hash: batch.hashes[position],
                height,
                timestamp: batch.timestamps[position],
            }
        );
    }
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&fixture.snapshot)
            .expect("ledger current index"),
        2
    );
}

#[tokio::test]
async fn verified_catch_up_batch_freezes_context_and_replays_live_effects_once() {
    let batch = signed_sync_batch();
    let fixture = SyncBatchFixture::new(
        batch.settings,
        crate::SyncBatchCommitPolicy::DeferredCatchUp,
        false,
    )
    .await;
    let mut events = fixture.handle.subscribe();

    let reply = fixture
        .service
        .handle_import(Import {
            blocks: batch.blocks,
            mode: ImportMode::Sync,
        })
        .await;

    assert_eq!(reply.imported, 2);
    assert_eq!(reply.error, None);
    assert_eq!(fixture.probe.commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.flush_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.probe.committing_contexts.lock().as_slice(),
        &[
            crate::service_context::BlockPersistContext::catch_up(),
            crate::service_context::BlockPersistContext::catch_up(),
        ]
    );
    assert_eq!(
        fixture.probe.committed_contexts.lock().as_slice(),
        &[
            crate::service_context::BlockPersistContext::catch_up(),
            crate::service_context::BlockPersistContext::catch_up(),
        ]
    );
    assert_eq!(
        fixture.probe.block_persisted_calls.load(Ordering::SeqCst),
        2
    );
    assert_eq!(fixture.probe.reverify_calls.load(Ordering::SeqCst), 1);
    for (position, height) in [1_u32, 2].into_iter().enumerate() {
        assert_eq!(
            events.try_recv().expect("ordered catch-up import event"),
            crate::RuntimeEvent::Imported {
                hash: batch.hashes[position],
                height,
                timestamp: batch.timestamps[position],
            }
        );
    }
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn verified_sync_batch_falls_back_to_per_block_durability_when_policy_rejects_batch_commit() {
    let batch = signed_sync_batch();
    let fixture = SyncBatchFixture::new(
        batch.settings,
        crate::SyncBatchCommitPolicy::PerBlock,
        false,
    )
    .await;

    let reply = fixture
        .service
        .handle_import(Import {
            blocks: batch.blocks,
            mode: ImportMode::Sync,
        })
        .await;

    assert_eq!(reply.imported, 2);
    assert_eq!(reply.error, None);
    assert_eq!(fixture.service.ledger.current_height(), 2);
    assert_eq!(fixture.probe.commit_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.probe.flush_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.probe.committed_heights.lock().as_slice(), &[1, 2]);
    assert_eq!(
        fixture.probe.commit_counts_at_committed.lock().as_slice(),
        &[1, 2]
    );
    assert!(
        fixture
            .probe
            .artifact_lengths
            .lock()
            .iter()
            .all(|length| *length > 0)
    );
    assert_eq!(
        fixture.probe.committing_contexts.lock().as_slice(),
        &[
            crate::service_context::BlockPersistContext::live(),
            crate::service_context::BlockPersistContext::live(),
        ],
        "per-block fallback must retain ordinary live observer semantics"
    );
    assert_eq!(
        fixture.probe.block_persisted_calls.load(Ordering::SeqCst),
        2
    );
    assert_eq!(fixture.probe.reverify_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&fixture.snapshot)
            .expect("ledger current index"),
        2
    );
}

#[tokio::test]
async fn verified_sync_batch_finalization_failure_rewinds_tip_without_replaying_live_side_effects()
{
    let batch = signed_sync_batch();
    let fixture = SyncBatchFixture::new(
        batch.settings,
        crate::SyncBatchCommitPolicy::DeferredLive,
        true,
    )
    .await;
    let mut events = fixture.handle.subscribe();

    let reply = fixture
        .service
        .handle_import(Import {
            blocks: batch.blocks,
            mode: ImportMode::Sync,
        })
        .await;

    assert_eq!(reply.imported, 0);
    assert!(
        reply
            .error
            .as_deref()
            .is_some_and(|error| error.contains("state-root worker"))
    );
    assert_eq!(fixture.service.ledger.current_height(), 0);
    assert_eq!(fixture.probe.flush_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.commit_calls.load(Ordering::SeqCst), 0);
    assert!(reply.stats.finalization_commit_handlers_elapsed > std::time::Duration::ZERO);
    assert_eq!(
        reply.stats.finalization_store_commit_elapsed,
        std::time::Duration::ZERO
    );
    assert!(fixture.probe.committed_heights.lock().is_empty());
    assert!(fixture.probe.commit_counts_at_committed.lock().is_empty());
    assert!(
        fixture
            .probe
            .artifact_lengths
            .lock()
            .iter()
            .all(|length| *length > 0)
    );
    assert_eq!(
        fixture.probe.block_persisted_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(fixture.probe.reverify_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}
