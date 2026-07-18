use super::*;
use neo_primitives::FindOptions;

#[derive(Clone)]
struct SnapshotReadingProvider {
    present: StorageKey,
    absent: StorageKey,
}

impl NativeContractProvider for SnapshotReadingProvider {
    type Contract = NoNativeContract;

    fn current_block_index<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<u32> {
        let _ = snapshot.get(&self.present);
        Ok(0)
    }

    fn exec_fee_factor_raw<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<u32> {
        let _ = snapshot.get(&self.present);
        Ok(30)
    }

    fn storage_price<B: neo_storage::CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        let _ = snapshot.get(&self.absent);
        Ok(100_000)
    }
}

type ProviderPrepared = PreparedShadowEngine<SnapshotReadingProvider, NoDiagnostic>;

fn prepare_provider_reads(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    provider: &SnapshotReadingProvider,
) -> CoreResult<ProviderPrepared> {
    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut engine = ApplicationEngine::<SnapshotReadingProvider>::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        native_cache,
        NoDiagnostic,
        Arc::new(provider.clone()),
    )?;
    observation_binding.bind(&mut engine);
    engine.load_script_bytes(&ordinary_script(), CallFlags::ALL, None)?;
    PreparedShadowEngine::new(engine)
}

fn expected_provider_journal(
    present: &StorageKey,
    absent: &StorageKey,
) -> ExecutionObservationJournal {
    let mut journal = ExecutionObservationJournal::new();
    for _ in 0..3 {
        journal
            .record_storage_read(present.clone(), Some(vec![0x2A]))
            .expect("expected present provider read");
    }
    journal
        .record_storage_read(absent.clone(), None)
        .expect("expected absent provider read");
    journal
}

#[test]
fn constructor_provider_reads_are_captured_before_engine_binding() {
    let present = StorageKey::new(-7, b"present-policy".to_vec());
    let absent = StorageKey::new(-7, b"absent-policy".to_vec());
    let provider = SnapshotReadingProvider {
        present: present.clone(),
        absent: absent.clone(),
    };
    let base = DataCache::new(false);
    base.add(present.clone(), StorageItem::from_bytes(vec![0x2A]));

    let outcome = run_flamingo_shadow_pair(
        &base,
        &control(false),
        ExecutionArtifactLimits::DEFAULT,
        b"constructor-provider-reads",
        |_, resources| prepare_provider_reads(resources, &provider),
    )
    .expect("equivalent provider reads match");
    assert_eq!(outcome.status(), ShadowReplayStatus::CandidateNotApplied);

    let expected = CanonicalExecutionArtifact::capture(
        outcome.ordinary_engine(),
        &expected_provider_journal(&present, &absent),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("expected provider artifact");
    outcome
        .ordinary_artifact()
        .expect("ordinary artifact")
        .compare(&expected)
        .expect("constructor provider reads retain exact order and values");
}

#[test]
fn artifact_capture_pauses_cache_observation() {
    let present = StorageKey::new(-7, b"present-policy".to_vec());
    let absent = StorageKey::new(-7, b"absent-policy".to_vec());
    let provider = SnapshotReadingProvider {
        present: present.clone(),
        absent,
    };
    let base = DataCache::new(false);
    base.add(present, StorageItem::from_bytes(vec![0x2A]));
    let limits = ExecutionArtifactLimits::DEFAULT;
    let mut factory = |_, resources| prepare_provider_reads(resources, &provider);
    let mut prepared = prepare_branch(&base, ShadowTwinBranch::Ordinary, limits, &mut factory)
        .expect("provider branch prepares");
    prepared.engine.execute_allow_fault();

    let first = capture(&prepared, limits).expect("first capture");
    let second = capture(&prepared, limits).expect("second capture");
    first
        .compare(&second)
        .expect("artifact capture must not append cache reads to its own journal");
}

#[derive(Clone, Copy)]
enum DirectCacheSurface {
    Point,
    PrefixRange,
    SemanticPrefixRange,
    WholeStoreRange,
    Delete,
}

fn direct_key() -> StorageKey {
    StorageKey::new(27, b"prefix-row".to_vec())
}

fn prepare_direct_cache_surface(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    surface: DirectCacheSurface,
) -> CoreResult<TestPrepared> {
    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut engine = TestEngine::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        native_cache,
        NoDiagnostic,
        Arc::new(NoNativeContractProvider),
    )?;
    observation_binding.bind(&mut engine);
    engine.load_script_bytes(&ordinary_script(), CallFlags::ALL, None)?;
    match surface {
        DirectCacheSurface::Point => {
            assert!(engine.snapshot_cache().get(&direct_key()).is_some());
        }
        DirectCacheSurface::PrefixRange => {
            let prefix = StorageKey::new(27, b"prefix".to_vec());
            let _ = engine
                .snapshot_cache()
                .find(Some(&prefix), SeekDirection::Forward);
        }
        DirectCacheSurface::SemanticPrefixRange => {
            let _ = engine.find_storage_entries(
                &StorageContext::read_only(27),
                b"prefix",
                FindOptions::KeysOnly | FindOptions::RemovePrefix,
            )?;
        }
        DirectCacheSurface::WholeStoreRange => {
            let _ = engine.snapshot_cache().find(None, SeekDirection::Forward);
        }
        DirectCacheSurface::Delete => engine.snapshot_cache().delete(&direct_key()),
    }
    PreparedShadowEngine::new(engine)
}

fn expected_direct_journal(surface: DirectCacheSurface) -> ExecutionObservationJournal {
    let key = direct_key();
    let value = b"value".to_vec();
    let mut journal = ExecutionObservationJournal::new();
    match surface {
        DirectCacheSurface::Point | DirectCacheSurface::Delete => journal
            .record_storage_read(key, Some(value))
            .expect("expected direct point read"),
        DirectCacheSurface::PrefixRange | DirectCacheSurface::SemanticPrefixRange => journal
            .record_storage_range(
                crate::host_access_audit::StorageRangeAccess::prefix(
                    27,
                    b"prefix".to_vec(),
                    neo_vm::RangeDirection::Forward,
                    if matches!(surface, DirectCacheSurface::SemanticPrefixRange) {
                        FindOptions::KeysOnly | FindOptions::RemovePrefix
                    } else {
                        FindOptions::None
                    },
                    1,
                ),
                vec![(key, value)],
            )
            .expect("expected prefix range"),
        DirectCacheSurface::WholeStoreRange => journal
            .record_storage_range(
                crate::host_access_audit::StorageRangeAccess::whole_store(
                    neo_vm::RangeDirection::Forward,
                    1,
                ),
                vec![(key, value)],
            )
            .expect("expected whole-store range"),
    }
    journal
}

#[test]
fn direct_cache_reads_ranges_and_deletes_are_observed_exactly_once() {
    for surface in [
        DirectCacheSurface::Point,
        DirectCacheSurface::PrefixRange,
        DirectCacheSurface::SemanticPrefixRange,
        DirectCacheSurface::WholeStoreRange,
        DirectCacheSurface::Delete,
    ] {
        let base = DataCache::new(false);
        base.add(direct_key(), StorageItem::from_bytes(b"value".to_vec()));
        let outcome = run_flamingo_shadow_pair(
            &base,
            &control(false),
            ExecutionArtifactLimits::DEFAULT,
            b"direct-cache-surface",
            |_, resources| prepare_direct_cache_surface(resources, surface),
        )
        .expect("equivalent direct cache observations match");
        let expected = CanonicalExecutionArtifact::capture(
            outcome.ordinary_engine(),
            &expected_direct_journal(surface),
            ExecutionArtifactLimits::DEFAULT,
        )
        .expect("expected direct-cache artifact");
        outcome
            .ordinary_artifact()
            .expect("ordinary artifact")
            .compare(&expected)
            .expect("direct cache surface must be retained exactly once");
    }
}

#[test]
fn direct_cache_observation_bounds_fail_closed_after_ordinary_execution() {
    let cases = [
        (
            DirectCacheSurface::Point,
            ExecutionArtifactLimits {
                max_storage_reads: 0,
                ..ExecutionArtifactLimits::DEFAULT
            },
        ),
        (
            DirectCacheSurface::PrefixRange,
            ExecutionArtifactLimits {
                max_storage_ranges: 0,
                ..ExecutionArtifactLimits::DEFAULT
            },
        ),
        (
            DirectCacheSurface::PrefixRange,
            ExecutionArtifactLimits {
                max_storage_range_rows: 0,
                ..ExecutionArtifactLimits::DEFAULT
            },
        ),
    ];
    for (surface, limits) in cases {
        let base = DataCache::new(false);
        base.add(direct_key(), StorageItem::from_bytes(b"value".to_vec()));
        let result = run_flamingo_shadow_pair(
            &base,
            &control(true),
            limits,
            b"direct-cache-bound",
            |_, resources| prepare_direct_cache_surface(resources, surface),
        );
        let FlamingoShadowRunError::StrictReplay(failure) =
            result.expect_err("direct cache bound must fail strict replay")
        else {
            panic!("ordinary preparation unexpectedly failed");
        };
        assert_eq!(failure.ordinary_engine().state(), VmState::HALT);
        assert!(matches!(
            failure.kind(),
            ShadowStrictReplayFailureKind::Infrastructure(
                ShadowInfrastructureStage::InitialOrdinaryArtifact
            )
        ));
    }
}
