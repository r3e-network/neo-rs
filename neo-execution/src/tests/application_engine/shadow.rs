use super::*;
use crate::application_engine::TEST_MODE_GAS;
use crate::contract_state::ContractState;
use crate::diagnostic::{InstructionCounter, NoDiagnostic};
use crate::execution_artifact::ExecutionObservationJournal;
use crate::native_contract_provider::{
    NativeContractProvider, NoNativeContract, NoNativeContractProvider,
};
use crate::specialization::{
    CandidateRouteConfig, FLAMINGO_FACTORY_PAIR_KEY_ENTRY, SpecializationControlConfig,
    SpecializationControlLimits, SpecializationDisableReason, flamingo_pair_key_candidate,
};
use neo_config::ProtocolSettings;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractParameterDefinition, ContractPermission,
    ManifestFeatures, NefFile, WildCardContainer,
};
use neo_payloads::Block;
use neo_primitives::{CallFlags, ContractParameterType, TriggerType, UInt160};
use neo_storage::{DataCache, StorageItem, StorageKey};
use neo_vm::{HardforkTableIdentity, OpCode, Script, Slot, SpecializationMode, StackItem, VmState};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::str::FromStr;

type TestEngine = ApplicationEngine<NoNativeContractProvider>;
type TestPrepared = PreparedShadowEngine<NoNativeContractProvider, NoDiagnostic>;

fn control(strict_replay: bool) -> SpecializationControl {
    let config = SpecializationControlConfig::try_enabled(
        strict_replay,
        SpecializationControlLimits::DEFAULT,
        [CandidateRouteConfig::new(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            SpecializationMode::Shadow,
        )],
    )
    .expect("valid shadow control");
    SpecializationControl::new(config)
}

fn prepare_script(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    script: &[u8],
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
    engine.load_script_bytes(script, CallFlags::ALL, None)?;
    PreparedShadowEngine::new(engine)
}

fn ordinary_script() -> [u8; 2] {
    [OpCode::PUSH1.byte(), OpCode::RET.byte()]
}

fn prepare_exact_candidate(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    outer_fault: bool,
) -> CoreResult<TestPrepared> {
    const PROFILE_HEIGHT: u32 = 1_887_001;
    const CONTRACT_ID: i32 = 27;
    const CONTRACT_UPDATE: u16 = 1;
    const CONTRACT_NEF_CHECKSUM: u32 = 2_962_741_568;

    let candidate = flamingo_pair_key_candidate(HardforkTableIdentity::unconfigured())
        .expect("embedded candidate is valid");
    let script_bytes = candidate.identity().execution().script_bytes().to_vec();
    let contract_hash = UInt160::from_str("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc")
        .expect("known contract hash");
    let mut nef = NefFile::new("shadow-pair-test".to_string(), script_bytes.clone());
    nef.checksum = CONTRACT_NEF_CHECKSUM;
    let mut contract = ContractState::new(
        CONTRACT_ID,
        contract_hash,
        nef,
        ContractManifest::new("FlamingoSwapFactory".to_string()),
    );
    contract.update_counter = CONTRACT_UPDATE;
    let contract = Arc::new(contract);

    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut block = Block::new();
    block.header.set_index(PROFILE_HEIGHT);
    let mut engine = TestEngine::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        Some(Arc::new(block)),
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        native_cache,
        NoDiagnostic,
        Arc::new(NoNativeContractProvider),
    )?;
    observation_binding.bind(&mut engine);

    if outer_fault {
        engine.load_script(
            vec![
                OpCode::DROP.byte(),
                OpCode::DROP.byte(),
                OpCode::PUSHF.byte(),
                OpCode::ASSERT.byte(),
            ],
            CallFlags::ALL,
            None,
        )?;
    }

    let script = Script::new_relaxed(script_bytes);
    let caller_position = script.len();
    let context_contract = Arc::clone(&contract);
    let mut caller = engine.load_script_with_state(script, -1, caller_position, move |state| {
        state.contract = Some(context_contract);
        state.script_hash = Some(contract_hash);
    })?;
    let reference_counter = caller.reference_counter().clone();
    caller.set_static_fields(Some(Slot::new(2, reference_counter)));
    caller
        .store_static_field(1, StackItem::from_buffer(vec![0xFF]))
        .expect("static prefix initializes");
    caller
        .push(StackItem::from_i64(0x1234))
        .expect("sentinel pushes");
    caller
        .push(StackItem::from_byte_string(vec![0x22; 20]))
        .expect("token B pushes");
    caller
        .push(StackItem::from_byte_string(vec![0x11; 20]))
        .expect("token A pushes");

    let callee = caller
        .clone_with_position(FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize)
        .expect("internal CALL clone");
    let attached_here = engine.attach_host();
    let load_result = engine.vm_engine.engine_mut().load_context(callee);
    engine.detach_host(attached_here);
    load_result.map_err(|error| CoreError::invalid_operation(error.to_string()))?;
    PreparedShadowEngine::new(engine)
}

#[test]
fn disabled_control_executes_only_the_ordinary_engine() {
    let base = DataCache::new(false);
    let calls = Cell::new(0usize);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &SpecializationControl::disabled(),
        ExecutionArtifactLimits::DEFAULT,
        b"disabled",
        |_, resources| {
            calls.set(calls.get() + 1);
            prepare_script(resources, &ordinary_script())
        },
    )
    .expect("ordinary execution succeeds");

    assert_eq!(calls.get(), 1);
    assert_eq!(outcome.ordinary_engine().state(), VmState::HALT);
    assert_eq!(
        outcome.status(),
        ShadowReplayStatus::OrdinaryOnly {
            decision: SpecializationRouteDecision::Ordinary {
                reason: SpecializationDisableReason::GloballyDisabled,
            },
        }
    );
    assert!(outcome.candidate_artifact().is_none());
}

#[test]
fn equivalent_twins_use_distinct_overlays_and_native_caches() {
    let base = DataCache::new(false);
    let resource_identities = RefCell::new(Vec::new());
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control(false),
        ExecutionArtifactLimits::DEFAULT,
        b"equivalent",
        |_, resources| {
            let (snapshot, native_cache, observation_binding) = resources.into_parts();
            resource_identities.borrow_mut().push((
                Arc::as_ptr(&snapshot) as usize,
                Arc::as_ptr(&native_cache) as usize,
            ));
            prepare_script(
                ShadowTwinResources {
                    snapshot_cache: snapshot,
                    native_contract_cache: native_cache,
                    observation_binding,
                },
                &ordinary_script(),
            )
        },
    )
    .expect("shadow fallback matches ordinary");

    let identities = resource_identities.into_inner();
    assert_eq!(identities.len(), 2);
    assert_ne!(identities[0].0, identities[1].0);
    assert_ne!(identities[0].1, identities[1].1);
    assert_eq!(outcome.status(), ShadowReplayStatus::CandidateNotApplied);
    assert!(outcome.ordinary_artifact().is_some());
    assert!(outcome.candidate_artifact().is_some());
    assert!(!base.has_pending_changes());
}

#[test]
fn exact_candidate_pair_matches_and_records_one_applied_frame() {
    let base = DataCache::new(false);
    let control = control(false);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control,
        ExecutionArtifactLimits::DEFAULT,
        b"exact-candidate",
        |_, resources| prepare_exact_candidate(resources, false),
    )
    .expect("exact candidate matches ordinary execution");

    assert_eq!(
        outcome.status(),
        ShadowReplayStatus::Matched { applied_frames: 1 }
    );
    assert_eq!(outcome.ordinary_engine().state(), VmState::HALT);
    outcome
        .ordinary_artifact()
        .expect("ordinary artifact")
        .compare(outcome.candidate_artifact().expect("candidate artifact"))
        .expect("complete artifacts match");
    let snapshot = control.snapshot();
    assert_eq!(snapshot.candidates[0].matches, 1);
    assert_eq!(snapshot.candidates[0].mismatches, 0);
}

#[test]
fn exact_candidate_pair_matches_when_an_outer_caller_later_faults() {
    let base = DataCache::new(false);
    let control = control(false);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control,
        ExecutionArtifactLimits::DEFAULT,
        b"exact-candidate-outer-fault",
        |_, resources| prepare_exact_candidate(resources, true),
    )
    .expect("the specialized frame and later fault remain equivalent");

    assert_eq!(
        outcome.status(),
        ShadowReplayStatus::Matched { applied_frames: 1 }
    );
    assert_eq!(outcome.ordinary_engine().state(), VmState::FAULT);
    assert_eq!(
        outcome.ordinary_engine().fault_exception(),
        Some("ASSERT is executed with false result. [ip=3 opcode=ASSERT eval_depth=0]")
    );
    outcome
        .ordinary_artifact()
        .expect("ordinary fault artifact")
        .compare(
            outcome
                .candidate_artifact()
                .expect("candidate fault artifact"),
        )
        .expect("complete fault artifacts match");
    let snapshot = control.snapshot();
    assert_eq!(snapshot.candidates[0].matches, 1);
    assert_eq!(snapshot.candidates[0].mismatches, 0);
}

#[test]
fn candidate_kill_switch_prevents_candidate_construction() {
    let base = DataCache::new(false);
    let control = control(false);
    assert!(control.kill_candidate(
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    ));
    let calls = Cell::new(0usize);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control,
        ExecutionArtifactLimits::DEFAULT,
        b"killed",
        |_, resources| {
            calls.set(calls.get() + 1);
            prepare_script(resources, &ordinary_script())
        },
    )
    .expect("ordinary execution succeeds");

    assert_eq!(calls.get(), 1);
    assert_eq!(
        outcome.status(),
        ShadowReplayStatus::OrdinaryOnly {
            decision: SpecializationRouteDecision::Ordinary {
                reason: SpecializationDisableReason::CandidateKillSwitch,
            },
        }
    );
}

#[test]
fn strict_initial_overlay_divergence_retains_authority_without_latching_candidate() {
    let base = DataCache::new(false);
    let control = control(true);
    let mismatch_key = StorageKey::new(27, b"shadow-only".to_vec());
    let result = run_flamingo_shadow_pair(
        &base,
        &control,
        ExecutionArtifactLimits::DEFAULT,
        b"strict-reproducer",
        |branch, resources| {
            let (snapshot, native_cache, observation_binding) = resources.into_parts();
            if branch == ShadowTwinBranch::Candidate {
                snapshot.add(
                    mismatch_key.clone(),
                    StorageItem::from_bytes(b"candidate".to_vec()),
                );
            }
            prepare_script(
                ShadowTwinResources {
                    snapshot_cache: snapshot,
                    native_contract_cache: native_cache,
                    observation_binding,
                },
                &ordinary_script(),
            )
        },
    );

    let FlamingoShadowRunError::StrictReplay(failure) = result.expect_err("strict mismatch") else {
        panic!("ordinary preparation unexpectedly failed");
    };
    assert_eq!(failure.ordinary_engine().state(), VmState::HALT);
    assert!(failure.ordinary_artifact().is_some());
    let candidate_artifact = failure
        .candidate_artifact()
        .expect("bounded initial candidate artifact is retained");
    assert!(format!("{failure:?}").contains("has_candidate_artifact: true"));
    assert!(matches!(
        failure.kind(),
        ShadowStrictReplayFailureKind::TwinDivergence(mismatch)
            if mismatch.component()
                == crate::execution_artifact::ExecutionArtifactComponent::StorageChanges
    ));
    assert!(base.get(&mismatch_key).is_none());

    let snapshot = control.snapshot();
    assert!(!snapshot.candidates[0].mismatch_latched);
    assert_eq!(snapshot.candidates[0].mismatches, 0);
    assert!(snapshot.reproducers.is_empty());
    assert_ne!(
        artifact_evidence_digest(candidate_artifact),
        artifact_evidence_digest(failure.ordinary_artifact().expect("ordinary artifact"))
    );
}

#[test]
fn candidate_factory_panic_fails_closed_after_ordinary_execution() {
    let base = DataCache::new(false);
    let control = control(false);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control,
        ExecutionArtifactLimits::DEFAULT,
        b"panic",
        |branch, resources| {
            assert_ne!(branch, ShadowTwinBranch::Candidate, "candidate panic");
            prepare_script(resources, &ordinary_script())
        },
    )
    .expect("non-strict replay keeps ordinary authority");

    assert_eq!(outcome.ordinary_engine().state(), VmState::HALT);
    assert_eq!(
        outcome.status(),
        ShadowReplayStatus::CandidateUnavailable {
            stage: ShadowInfrastructureStage::CandidatePreparationPanic,
        }
    );
    assert_eq!(
        control.route(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
        ),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::CandidateKillSwitch,
        }
    );
}

fn prepare_absent_storage_read(
    resources: ShadowTwinResources<EmptyCacheBacking>,
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
    let value = engine.storage_get(StorageContext::read_only(27), b"absent".to_vec())?;
    assert!(value.is_none());
    PreparedShadowEngine::new(engine)
}

#[test]
fn live_absent_storage_reads_are_part_of_the_shadow_artifact() {
    let base = DataCache::new(false);
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control(false),
        ExecutionArtifactLimits::DEFAULT,
        b"observed-storage",
        |_, resources| prepare_absent_storage_read(resources),
    )
    .expect("equivalent observed reads match");

    assert_eq!(outcome.status(), ShadowReplayStatus::CandidateNotApplied);
    let without_live_observations = CanonicalExecutionArtifact::capture(
        outcome.ordinary_engine(),
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("empty comparison artifact");
    assert_eq!(
        outcome
            .ordinary_artifact()
            .expect("ordinary artifact")
            .compare(&without_live_observations)
            .expect_err("the live absent read must be retained")
            .component(),
        crate::execution_artifact::ExecutionArtifactComponent::StorageReads
    );
}

#[derive(Clone, Copy)]
enum ObservedHostSurface {
    Range,
    Witness,
    Context,
    Fee,
}

fn prepare_host_surface(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    surface: ObservedHostSurface,
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
        ObservedHostSurface::Range => {
            let _ = engine.storage_find(
                StorageContext::read_only(27),
                b"prefix".to_vec(),
                neo_primitives::FindOptions::None,
            )?;
        }
        ObservedHostSurface::Witness => {
            assert!(!engine.check_witness_hash(&UInt160::from([0x77; 20]))?);
        }
        ObservedHostSurface::Context => engine.runtime_get_network()?,
        ObservedHostSurface::Fee => engine.charge_execution_fee(7)?,
    }
    PreparedShadowEngine::new(engine)
}

#[test]
fn live_range_witness_context_and_fee_callbacks_are_compared() {
    let cases = [
        (
            ObservedHostSurface::Range,
            crate::execution_artifact::ExecutionArtifactComponent::StorageRanges,
        ),
        (
            ObservedHostSurface::Witness,
            crate::execution_artifact::ExecutionArtifactComponent::Witnesses,
        ),
        (
            ObservedHostSurface::Context,
            crate::execution_artifact::ExecutionArtifactComponent::Contexts,
        ),
        (
            ObservedHostSurface::Fee,
            crate::execution_artifact::ExecutionArtifactComponent::FeeCharges,
        ),
    ];
    for (surface, expected_component) in cases {
        let base = DataCache::new(false);
        if matches!(surface, ObservedHostSurface::Range) {
            base.add(
                StorageKey::new(27, b"prefix-row".to_vec()),
                StorageItem::from_bytes(b"value".to_vec()),
            );
        }
        let outcome = run_flamingo_shadow_pair(
            &base,
            &control(false),
            ExecutionArtifactLimits::DEFAULT,
            b"observed-host-surface",
            |_, resources| prepare_host_surface(resources, surface),
        )
        .expect("equivalent host observations match");
        let without_live_observations = CanonicalExecutionArtifact::capture(
            outcome.ordinary_engine(),
            &ExecutionObservationJournal::new(),
            ExecutionArtifactLimits::DEFAULT,
        )
        .expect("empty comparison artifact");
        assert_eq!(
            outcome
                .ordinary_artifact()
                .expect("ordinary artifact")
                .compare(&without_live_observations)
                .expect_err("live host observation must be retained")
                .component(),
            expected_component
        );
    }
}

#[test]
fn live_observation_bound_failure_fails_strict_replay_without_changing_vm_result() {
    let base = DataCache::new(false);
    let limits = ExecutionArtifactLimits {
        max_storage_reads: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    let result = run_flamingo_shadow_pair(
        &base,
        &control(true),
        limits,
        b"observation-bound",
        |_, resources| prepare_absent_storage_read(resources),
    );

    let FlamingoShadowRunError::StrictReplay(failure) = result.expect_err("strict bound failure")
    else {
        panic!("ordinary preparation unexpectedly failed");
    };
    assert_eq!(failure.ordinary_engine().state(), VmState::HALT);
    assert!(failure.candidate_artifact().is_none());
    assert!(matches!(
        failure.kind(),
        ShadowStrictReplayFailureKind::Infrastructure(
            ShadowInfrastructureStage::InitialOrdinaryArtifact
        )
    ));
    assert!(matches!(
        failure.infrastructure_error(),
        Some(ExecutionArtifactError::LimitExceeded {
            resource: "storage read observations",
            actual: 1,
            maximum: 0,
        })
    ));
}

#[test]
fn overflowed_artifact_capture_falls_back_to_ordinary_when_explicitly_allowed() {
    let base = DataCache::new(false);
    let limits = ExecutionArtifactLimits {
        max_storage_reads: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    let config = SpecializationControlConfig::try_enabled(
        true,
        SpecializationControlLimits::DEFAULT,
        [CandidateRouteConfig::new(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            SpecializationMode::Shadow,
        )],
    )
    .expect("valid shadow control")
    .with_artifact_overflow_fallback(true);
    let control = SpecializationControl::new(config);

    let outcome = run_flamingo_shadow_pair(
        &base,
        &control,
        limits,
        b"observation-bound",
        |_, resources| prepare_absent_storage_read(resources),
    )
    .expect("overflow fallback returns the ordinary-only outcome");

    assert_eq!(outcome.ordinary_engine().state(), VmState::HALT);
    assert!(matches!(
        outcome.status(),
        ShadowReplayStatus::CandidateUnavailable {
            stage: ShadowInfrastructureStage::InitialOrdinaryArtifact
        }
    ));
}

#[test]
fn non_overflow_infrastructure_failure_still_aborts_strict_replay_with_fallback_allowed() {
    let base = DataCache::new(false);
    let limits = ExecutionArtifactLimits {
        max_storage_reads: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    let config = SpecializationControlConfig::try_enabled(
        true,
        SpecializationControlLimits::DEFAULT,
        [CandidateRouteConfig::new(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            SpecializationMode::Shadow,
        )],
    )
    .expect("valid shadow control")
    .with_artifact_overflow_fallback(true);
    let control = SpecializationControl::new(config);
    let mut factory = |_, resources| prepare_script(resources, &ordinary_script());
    let ordinary = prepare_branch(&base, ShadowTwinBranch::Ordinary, limits, &mut factory)
        .expect("ordinary branch prepares");

    let result = unavailable_parts(
        ordinary,
        None,
        None,
        &control,
        ShadowInfrastructureStage::CandidatePreparation,
        None,
    );

    let FlamingoShadowRunError::StrictReplay(failure) =
        result.expect_err("non-overflow infrastructure failure must abort")
    else {
        panic!("ordinary preparation unexpectedly failed");
    };
    assert!(matches!(
        failure.kind(),
        ShadowStrictReplayFailureKind::Infrastructure(
            ShadowInfrastructureStage::CandidatePreparation
        )
    ));
}

#[test]
fn strict_infrastructure_failure_retains_an_available_bounded_candidate_artifact() {
    let base = DataCache::new(false);
    let limits = ExecutionArtifactLimits::DEFAULT;
    let mut factory = |_, resources| prepare_script(resources, &ordinary_script());
    let mut ordinary = prepare_branch(&base, ShadowTwinBranch::Ordinary, limits, &mut factory)
        .expect("ordinary branch prepares");
    ordinary.engine.execute_allow_fault();
    let ordinary_artifact = capture(&ordinary, limits).expect("ordinary artifact is bounded");
    let candidate_artifact = ordinary_artifact.clone();

    let result = unavailable_parts(
        ordinary,
        Some(ordinary_artifact),
        Some(candidate_artifact.clone()),
        &control(true),
        ShadowInfrastructureStage::CandidateIdentity,
        None,
    );
    let FlamingoShadowRunError::StrictReplay(failure) =
        result.expect_err("strict infrastructure failure")
    else {
        panic!("ordinary preparation unexpectedly failed");
    };

    assert_eq!(failure.ordinary_engine().state(), VmState::HALT);
    assert_eq!(failure.candidate_artifact(), Some(&candidate_artifact));
    assert!(format!("{failure:?}").contains("has_candidate_artifact: true"));
    assert!(matches!(
        failure.kind(),
        ShadowStrictReplayFailureKind::Infrastructure(ShadowInfrastructureStage::CandidateIdentity)
    ));
}

fn alias_cycle_contract(hash: UInt160) -> ContractState {
    let parameter =
        ContractParameterDefinition::new("value".to_string(), ContractParameterType::Any)
            .expect("parameter");
    let method = ContractMethodDescriptor::new(
        "echo".to_string(),
        vec![parameter],
        ContractParameterType::Any,
        0,
        true,
    )
    .expect("method");
    let manifest = ContractManifest {
        name: "ObservedEcho".to_string(),
        groups: Vec::new(),
        features: ManifestFeatures::empty(),
        supported_standards: Vec::new(),
        abi: ContractAbi::new(vec![method], Vec::new()),
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::default(),
        extra: None,
    };
    ContractState::new(
        27,
        hash,
        NefFile::new("observed-echo".to_string(), vec![OpCode::RET.byte()]),
        manifest,
    )
}

#[derive(Clone, Copy, Debug)]
struct UnblockedProvider;

impl NativeContractProvider for UnblockedProvider {
    type Contract = NoNativeContract;

    fn policy_is_blocked<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &DataCache<B>,
        _account: &UInt160,
    ) -> CoreResult<bool> {
        Ok(false)
    }

    fn policy_whitelisted_fee<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &DataCache<B>,
        _contract_hash: &UInt160,
        _method: &str,
        _param_count: u32,
    ) -> CoreResult<Option<i64>> {
        Ok(None)
    }
}

fn prepare_alias_cycle_call(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    retained_sources: &RefCell<Vec<StackItem>>,
) -> CoreResult<PreparedShadowEngine<UnblockedProvider, NoDiagnostic>> {
    let contract_hash = UInt160::from([0x52; 20]);
    let contract = alias_cycle_contract(contract_hash);
    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut engine = ApplicationEngine::<UnblockedProvider>::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::from([(contract_hash, contract)]),
        native_cache,
        NoDiagnostic,
        Arc::new(UnblockedProvider),
    )?;
    observation_binding.bind(&mut engine);
    engine.load_script_bytes(
        &ordinary_script(),
        CallFlags::ALL,
        Some(UInt160::from([0x41; 20])),
    )?;

    let shared = StackItem::from_buffer(vec![0x11]);
    let cycle = StackItem::from_array(vec![shared.clone(), shared]);
    let StackItem::Array(array) = &cycle else {
        unreachable!("test array")
    };
    array.push(cycle.clone()).expect("create self-cycle");
    retained_sources.borrow_mut().push(cycle.clone());
    engine.call_contract_dynamic(&contract_hash, "echo", CallFlags::ALL, vec![cycle])?;
    PreparedShadowEngine::new(engine)
}

#[test]
fn live_completed_calls_snapshot_cross_boundary_aliases_and_cycles() {
    let base = DataCache::new(false);
    let retained_sources = RefCell::new(Vec::new());
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control(false),
        ExecutionArtifactLimits::DEFAULT,
        b"observed-call-cycle",
        |_, resources| prepare_alias_cycle_call(resources, &retained_sources),
    )
    .expect("equivalent cyclic calls match");

    let without_live_observations = CanonicalExecutionArtifact::capture(
        outcome.ordinary_engine(),
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("empty comparison artifact");
    assert_eq!(
        outcome
            .ordinary_artifact()
            .expect("ordinary artifact")
            .compare(&without_live_observations)
            .expect_err("the live call must be retained")
            .component(),
        crate::execution_artifact::ExecutionArtifactComponent::Calls
    );
    for source in retained_sources.take() {
        let StackItem::Array(array) = source else {
            unreachable!("test array")
        };
        array.clear().expect("mutate source after capture");
    }
    outcome
        .ordinary_artifact()
        .expect("ordinary artifact")
        .compare(outcome.candidate_artifact().expect("candidate artifact"))
        .expect("immutable live call snapshots remain equal");
}

fn prepare_diagnostic_cycle(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    retained_sources: &RefCell<Vec<StackItem>>,
) -> CoreResult<PreparedShadowEngine<NoNativeContractProvider, InstructionCounter>> {
    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut engine = ApplicationEngine::<NoNativeContractProvider, InstructionCounter>::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        native_cache,
        InstructionCounter::new(),
        Arc::new(NoNativeContractProvider),
    )?;
    observation_binding.bind(&mut engine);
    engine.load_script_bytes(&[OpCode::RET.byte()], CallFlags::ALL, None)?;
    let shared = StackItem::from_buffer(vec![0x91]);
    let cycle = StackItem::from_array(vec![shared.clone(), shared]);
    let StackItem::Array(array) = &cycle else {
        unreachable!("test array")
    };
    array.push(cycle.clone()).expect("create diagnostic cycle");
    retained_sources.borrow_mut().push(cycle.clone());
    engine.push(cycle)?;
    PreparedShadowEngine::new(engine)
}

#[derive(Debug)]
struct ToggleDiagnostic {
    enabled: bool,
}

impl crate::Diagnostic for ToggleDiagnostic {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn initialized(&mut self) {}

    fn disposed(&mut self) {}

    fn context_loaded<B: neo_storage::CacheRead>(
        &mut self,
        _context: &crate::ApplicationExecutionContext<B>,
    ) {
    }

    fn context_unloaded<B: neo_storage::CacheRead>(
        &mut self,
        _context: &crate::ApplicationExecutionContext<B>,
    ) {
    }

    fn pre_execute_instruction(&mut self, _instruction: &neo_vm::Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &neo_vm::Instruction) {}
}

fn prepare_toggle_diagnostic(
    resources: ShadowTwinResources<EmptyCacheBacking>,
    enabled: bool,
) -> CoreResult<PreparedShadowEngine<NoNativeContractProvider, ToggleDiagnostic>> {
    let (snapshot, native_cache, observation_binding) = resources.into_parts();
    let mut engine = ApplicationEngine::<NoNativeContractProvider, ToggleDiagnostic>::new_with_preloaded_native_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        native_cache,
        ToggleDiagnostic { enabled },
        Arc::new(NoNativeContractProvider),
    )?;
    observation_binding.bind(&mut engine);
    engine.load_script_bytes(&[OpCode::RET.byte()], CallFlags::ALL, None)?;
    PreparedShadowEngine::new(engine)
}

#[test]
fn zero_frame_twin_divergence_does_not_latch_the_candidate() {
    let base = DataCache::new(false);
    let limits = ExecutionArtifactLimits::DEFAULT;
    let mut ordinary_factory = |_, resources| prepare_toggle_diagnostic(resources, true);
    let mut candidate_factory = |_, resources| prepare_toggle_diagnostic(resources, false);
    let mut ordinary = prepare_branch(
        &base,
        ShadowTwinBranch::Ordinary,
        limits,
        &mut ordinary_factory,
    )
    .expect("prepare diagnostic ordinary twin");
    let mut candidate = prepare_branch(
        &base,
        ShadowTwinBranch::Candidate,
        limits,
        &mut candidate_factory,
    )
    .expect("prepare diagnostic candidate twin");
    ordinary.engine.execute_allow_fault();
    candidate.engine.execute_allow_fault();
    let ordinary_artifact = capture(&ordinary, limits).expect("capture ordinary artifact");
    let candidate_artifact = capture(&candidate, limits).expect("capture candidate artifact");
    let mismatch = ordinary_artifact
        .compare(&candidate_artifact)
        .expect_err("diagnostic twins intentionally differ");
    assert_eq!(
        mismatch.component(),
        crate::execution_artifact::ExecutionArtifactComponent::Diagnostics
    );

    let control = control(true);
    let result = finish_twin_divergence(
        ordinary,
        Some(ordinary_artifact),
        Some(candidate_artifact),
        &control,
        mismatch,
    );
    let FlamingoShadowRunError::StrictReplay(failure) =
        result.expect_err("strict twin divergence must abort replay")
    else {
        panic!("ordinary preparation unexpectedly failed");
    };
    assert!(matches!(
        failure.kind(),
        ShadowStrictReplayFailureKind::TwinDivergence(observed) if *observed == mismatch
    ));
    let snapshot = control.snapshot();
    assert!(!snapshot.candidates[0].mismatch_latched);
    assert_eq!(snapshot.candidates[0].mismatches, 0);
    assert!(snapshot.reproducers.is_empty());
}

#[test]
fn enabled_diagnostic_callbacks_snapshot_exact_cyclic_stacks() {
    let base = DataCache::new(false);
    let retained_sources = RefCell::new(Vec::new());
    let outcome = run_flamingo_shadow_pair(
        &base,
        &control(false),
        ExecutionArtifactLimits::DEFAULT,
        b"observed-diagnostic-cycle",
        |_, resources| prepare_diagnostic_cycle(resources, &retained_sources),
    )
    .expect("equivalent diagnostic callbacks match");

    let without_live_observations = CanonicalExecutionArtifact::capture(
        outcome.ordinary_engine(),
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("empty comparison artifact");
    assert_eq!(
        outcome
            .ordinary_artifact()
            .expect("ordinary artifact")
            .compare(&without_live_observations)
            .expect_err("enabled diagnostic callbacks must be retained")
            .component(),
        crate::execution_artifact::ExecutionArtifactComponent::Diagnostics
    );
    for source in retained_sources.take() {
        let StackItem::Array(array) = source else {
            unreachable!("test array")
        };
        array.clear().expect("mutate source after capture");
    }
    outcome
        .ordinary_artifact()
        .expect("ordinary artifact")
        .compare(outcome.candidate_artifact().expect("candidate artifact"))
        .expect("diagnostic observations are immutable snapshots");
}

#[path = "shadow_storage_observer.rs"]
mod storage_observer;
