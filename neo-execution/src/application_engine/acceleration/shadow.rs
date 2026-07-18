//! Isolated ordinary-authoritative orchestration for guarded shadow execution.

use super::super::*;
use super::specializations::prepared_flamingo_candidate;
use crate::execution_artifact::{
    CanonicalExecutionArtifact, ExecutionArtifactError, ExecutionArtifactLimits,
    ExecutionArtifactMismatch,
};
use crate::specialization::{
    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    SpecializationControl, SpecializationControlError, SpecializationMismatchInput,
    SpecializationRouteDecision,
};
use neo_primitives::Verifiable;
use neo_storage::{DataCacheReadObserver, SeekDirection, StorageItem, StorageKey};
use neo_vm::CandidateContract;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Identifies which independently prepared engine a factory is constructing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowTwinBranch {
    /// Sequential NeoVM oracle whose output remains authoritative.
    Ordinary,
    /// Isolated engine on which the guarded candidate may run.
    Candidate,
}

/// Runner-owned resources for one isolated shadow branch.
///
/// Factories must bind all handles to the returned engine. The runner verifies
/// their `Arc` identities before execution and fails closed if either handle is
/// replaced or shared with another branch.
pub struct ShadowTwinResources<B: neo_storage::CacheRead> {
    snapshot_cache: Arc<DataCache<B>>,
    native_contract_cache: Arc<Mutex<NativeContractsCache>>,
    observation_binding: ShadowObservationBinding,
}

impl<B: neo_storage::CacheRead> ShadowTwinResources<B> {
    /// Consumes the resources so they can be passed directly to an engine constructor.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        Arc<DataCache<B>>,
        Arc<Mutex<NativeContractsCache>>,
        ShadowObservationBinding,
    ) {
        (
            self.snapshot_cache,
            self.native_contract_cache,
            self.observation_binding,
        )
    }
}

/// Opaque runner-owned binding for one twin's bounded live observations.
#[derive(Clone)]
pub struct ShadowObservationBinding {
    observations: Arc<Mutex<ExecutionObservationState>>,
}

struct ShadowStorageReadObserver {
    observations: Arc<Mutex<ExecutionObservationState>>,
}

impl DataCacheReadObserver for ShadowStorageReadObserver {
    fn observe_point_read(&self, key: &StorageKey, value: Option<&StorageItem>) {
        let mut observations = self.observations.lock();
        observations.record(|journal| journal.record_storage_read_borrowed(key, value));
    }

    fn observe_range_read(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
        rows: &[(StorageKey, StorageItem)],
    ) {
        let mut observations = self.observations.lock();
        observations.record(|journal| {
            let row_count =
                u32::try_from(rows.len()).map_err(|_| ExecutionArtifactError::NumericOverflow {
                    field: "storage range row count",
                })?;
            let direction = match direction {
                SeekDirection::Forward => neo_vm::RangeDirection::Forward,
                SeekDirection::Backward => neo_vm::RangeDirection::Reverse,
            };
            let access = prefix.map_or_else(
                || crate::host_access_audit::StorageRangeAccess::whole_store(direction, row_count),
                |prefix| {
                    crate::host_access_audit::StorageRangeAccess::prefix(
                        prefix.id(),
                        prefix.suffix().to_vec(),
                        direction,
                        neo_primitives::FindOptions::None,
                        row_count,
                    )
                },
            );
            journal.record_storage_range_borrowed(&access, rows)
        });
    }
}

impl ShadowObservationBinding {
    /// Binds live observations before the factory loads the transaction script.
    pub fn bind<P, D, B>(&self, engine: &mut ApplicationEngine<P, D, B>)
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        engine.bind_execution_observations(Arc::clone(&self.observations));
    }
}

/// One freshly prepared engine and its non-final observation journal.
pub struct PreparedShadowEngine<P, D, B = EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    engine: ApplicationEngine<P, D, B>,
    observations: Arc<Mutex<ExecutionObservationState>>,
}

impl<P, D, B> PreparedShadowEngine<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Wraps an engine previously bound through [`ShadowObservationBinding`].
    pub fn new(engine: ApplicationEngine<P, D, B>) -> CoreResult<Self> {
        let observations = engine.execution_observation_handle().ok_or_else(|| {
            CoreError::invalid_operation(
                "prepared shadow engine is missing its runner-owned observation binding",
            )
        })?;
        Ok(Self {
            engine,
            observations,
        })
    }

    fn into_engine(self) -> ApplicationEngine<P, D, B> {
        self.engine
            .original_snapshot_cache_handle()
            .disable_read_observation();
        self.engine
    }
}

/// Candidate-side stage that could not be proven safe for shadow comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowInfrastructureStage {
    /// The exact candidate contract could not be constructed.
    CandidateIdentity,
    /// The candidate factory rejected runner-owned isolated resources.
    CandidatePreparation,
    /// The candidate factory panicked before returning an engine.
    CandidatePreparationPanic,
    /// The ordinary initial artifact exceeded a bound or could not be captured.
    InitialOrdinaryArtifact,
    /// The candidate initial artifact exceeded a bound or could not be captured.
    InitialCandidateArtifact,
    /// The ordinary final artifact exceeded a bound or could not be captured.
    FinalOrdinaryArtifact,
    /// The candidate execution panicked inside its isolated branch.
    CandidateExecutionPanic,
    /// The candidate final artifact exceeded a bound or could not be captured.
    FinalCandidateArtifact,
    /// Isolated twins diverged before any specialized frame was applied.
    TwinDivergenceWithoutCandidate,
}

/// Result of one ordinary-authoritative shadow attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowReplayStatus {
    /// Controls selected ordinary execution before candidate construction.
    OrdinaryOnly {
        /// Exact fail-closed routing decision.
        decision: SpecializationRouteDecision,
    },
    /// Shadow infrastructure could not prove a comparison; ordinary still completed.
    CandidateUnavailable {
        /// Stage at which candidate work was abandoned.
        stage: ShadowInfrastructureStage,
    },
    /// Both engines completed equally, but no specialized frame was applied.
    CandidateNotApplied,
    /// Complete final artifacts matched after candidate application.
    Matched {
        /// Number of exact candidate frames applied in the isolated engine.
        applied_frames: u64,
    },
    /// Complete artifacts differed and the candidate was latched off.
    MismatchLatched {
        /// First canonical component that differed.
        mismatch: ExecutionArtifactMismatch,
        /// Number of candidate frames applied before final comparison.
        applied_frames: u64,
    },
}

/// Completed shadow attempt. The ordinary engine is always the authoritative result.
pub struct FlamingoShadowOutcome<P, D, B = EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    ordinary_engine: ApplicationEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    status: ShadowReplayStatus,
}

impl<P, D, B> std::fmt::Debug for FlamingoShadowOutcome<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FlamingoShadowOutcome")
            .field("status", &self.status)
            .field("has_ordinary_artifact", &self.ordinary_artifact.is_some())
            .field("has_candidate_artifact", &self.candidate_artifact.is_some())
            .finish_non_exhaustive()
    }
}

impl<P, D, B> FlamingoShadowOutcome<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    /// Returns the authoritative sequential engine.
    #[must_use]
    pub const fn ordinary_engine(&self) -> &ApplicationEngine<P, D, B> {
        &self.ordinary_engine
    }

    /// Returns the isolated transaction-root overlay owned by the ordinary branch.
    ///
    /// A persistence caller may commit this overlay only after the ordinary VM
    /// state is `HALT`; candidate overlays are never exposed by this API.
    #[must_use]
    pub fn ordinary_snapshot_cache(&self) -> Arc<DataCache<B>>
    where
        B: neo_storage::CacheRead,
    {
        self.ordinary_engine.original_snapshot_cache_handle()
    }

    /// Returns the final ordinary artifact when bounded capture succeeded.
    #[must_use]
    pub const fn ordinary_artifact(&self) -> Option<&CanonicalExecutionArtifact> {
        self.ordinary_artifact.as_ref()
    }

    /// Returns the final candidate artifact when candidate execution completed.
    #[must_use]
    pub const fn candidate_artifact(&self) -> Option<&CanonicalExecutionArtifact> {
        self.candidate_artifact.as_ref()
    }

    /// Returns the deterministic routing/comparison status.
    #[must_use]
    pub const fn status(&self) -> ShadowReplayStatus {
        self.status
    }

    /// Consumes the outcome and returns the authoritative sequential engine.
    #[must_use]
    pub fn into_ordinary_engine(self) -> ApplicationEngine<P, D, B> {
        self.ordinary_engine
    }
}

/// Why strict shadow replay could not produce an accepted comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShadowStrictReplayFailureKind {
    /// Artifact capture, preparation, or isolated candidate execution failed.
    Infrastructure(ShadowInfrastructureStage),
    /// Complete initial or final artifacts diverged.
    Mismatch(SpecializationControlError),
    /// Twin setup or process-global state diverged before candidate work.
    TwinDivergence(ExecutionArtifactMismatch),
}

/// Strict replay failure that retains the already-completed ordinary authority.
pub struct ShadowStrictReplayFailure<P, D, B = EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    ordinary_engine: ApplicationEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    kind: ShadowStrictReplayFailureKind,
    infrastructure_error: Option<ExecutionArtifactError>,
}

impl<P, D, B> std::fmt::Debug for ShadowStrictReplayFailure<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShadowStrictReplayFailure")
            .field("kind", &self.kind)
            .field("has_ordinary_artifact", &self.ordinary_artifact.is_some())
            .field("has_candidate_artifact", &self.candidate_artifact.is_some())
            .field("infrastructure_error", &self.infrastructure_error)
            .finish_non_exhaustive()
    }
}

impl<P, D, B> ShadowStrictReplayFailure<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    /// Returns why strict replay failed.
    #[must_use]
    pub const fn kind(&self) -> &ShadowStrictReplayFailureKind {
        &self.kind
    }

    /// Returns the exact bounded capture error for infrastructure failures.
    #[must_use]
    pub const fn infrastructure_error(&self) -> Option<&ExecutionArtifactError> {
        self.infrastructure_error.as_ref()
    }

    /// Returns the completed ordinary engine retained as the semantic oracle.
    #[must_use]
    pub const fn ordinary_engine(&self) -> &ApplicationEngine<P, D, B> {
        &self.ordinary_engine
    }

    /// Returns the final ordinary artifact when bounded capture succeeded.
    #[must_use]
    pub const fn ordinary_artifact(&self) -> Option<&CanonicalExecutionArtifact> {
        self.ordinary_artifact.as_ref()
    }

    /// Returns the isolated candidate artifact when bounded capture succeeded.
    #[must_use]
    pub const fn candidate_artifact(&self) -> Option<&CanonicalExecutionArtifact> {
        self.candidate_artifact.as_ref()
    }

    /// Consumes the failure and returns the completed ordinary engine.
    #[must_use]
    pub fn into_ordinary_engine(self) -> ApplicationEngine<P, D, B> {
        self.ordinary_engine
    }
}

/// Failure before an ordinary result is available, or a strict replay failure
/// that retains the authoritative ordinary engine.
pub enum FlamingoShadowRunError<P, D, B = EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    /// The caller could not construct the ordinary engine.
    OrdinaryPreparation(CoreError),
    /// Strict replay rejected an incomplete or divergent comparison.
    StrictReplay(ShadowStrictReplayFailure<P, D, B>),
}

impl<P, D, B> std::fmt::Debug for FlamingoShadowRunError<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OrdinaryPreparation(error) => formatter
                .debug_tuple("OrdinaryPreparation")
                .field(error)
                .finish(),
            Self::StrictReplay(failure) => formatter
                .debug_tuple("StrictReplay")
                .field(failure)
                .finish(),
        }
    }
}

/// Runs the exact Flamingo candidate against a separate engine and overlay.
///
/// `prepare` is invoked once for the ordinary branch and only when routing is
/// `Shadow` for the candidate branch. Each invocation receives a fresh child
/// overlay over `snapshot` and a distinct native method-metadata cache. The
/// runner verifies that the returned engine retained those exact resources,
/// compares initial artifacts before either engine runs, executes ordinary
/// NeoVM first, and never returns candidate state as authority.
///
/// `replay_payload` is used only for bounded first-mismatch evidence. It should
/// contain the caller's deterministic transaction or invocation encoding.
pub fn run_flamingo_shadow_pair<P, D, B, F>(
    snapshot: &DataCache<B>,
    control: &SpecializationControl,
    limits: ExecutionArtifactLimits,
    replay_payload: &[u8],
    mut prepare: F,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
    F: FnMut(ShadowTwinBranch, ShadowTwinResources<B>) -> CoreResult<PreparedShadowEngine<P, D, B>>,
{
    let mut ordinary = prepare_branch(snapshot, ShadowTwinBranch::Ordinary, limits, &mut prepare)
        .map_err(FlamingoShadowRunError::OrdinaryPreparation)?;
    let decision = control.route(
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    );
    if decision != SpecializationRouteDecision::Shadow {
        ordinary.engine.execute_allow_fault();
        let artifact = capture(&ordinary, limits).ok();
        return Ok(outcome(
            ordinary,
            artifact,
            None,
            ShadowReplayStatus::OrdinaryOnly { decision },
        ));
    }

    let hardforks = {
        let snapshot = ordinary.engine.original_snapshot_cache_handle();
        let _pause = snapshot.pause_read_observation();
        ordinary.engine.hardfork_plan_identity()
    };
    let Some(prepared_candidate) = prepared_flamingo_candidate(hardforks) else {
        ordinary.engine.execute_allow_fault();
        return unavailable(
            ordinary,
            None,
            control,
            limits,
            ShadowInfrastructureStage::CandidateIdentity,
            None,
        );
    };

    let candidate_result = catch_unwind(AssertUnwindSafe(|| {
        prepare_branch(snapshot, ShadowTwinBranch::Candidate, limits, &mut prepare)
    }));
    let candidate = match candidate_result {
        Ok(Ok(candidate)) => candidate,
        Ok(Err(_)) => {
            ordinary.engine.execute_allow_fault();
            return unavailable(
                ordinary,
                None,
                control,
                limits,
                ShadowInfrastructureStage::CandidatePreparation,
                None,
            );
        }
        Err(_) => {
            control.kill_candidate(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            );
            ordinary.engine.execute_allow_fault();
            return unavailable(
                ordinary,
                None,
                control,
                limits,
                ShadowInfrastructureStage::CandidatePreparationPanic,
                None,
            );
        }
    };

    let initial_ordinary = match capture(&ordinary, limits) {
        Ok(artifact) => artifact,
        Err(error) => {
            ordinary.engine.execute_allow_fault();
            return unavailable(
                ordinary,
                None,
                control,
                limits,
                ShadowInfrastructureStage::InitialOrdinaryArtifact,
                Some(error),
            );
        }
    };
    let initial_candidate = match capture(&candidate, limits) {
        Ok(artifact) => artifact,
        Err(error) => {
            ordinary.engine.execute_allow_fault();
            return unavailable(
                ordinary,
                None,
                control,
                limits,
                ShadowInfrastructureStage::InitialCandidateArtifact,
                Some(error),
            );
        }
    };
    if let Err(mismatch) = initial_ordinary.compare(&initial_candidate) {
        ordinary.engine.execute_allow_fault();
        let ordinary_artifact = capture(&ordinary, limits).ok();
        return finish_twin_divergence(
            ordinary,
            ordinary_artifact,
            Some(initial_candidate),
            control,
            mismatch,
        );
    }

    ordinary.engine.execute_allow_fault();
    let ordinary_artifact = match capture(&ordinary, limits) {
        Ok(artifact) => artifact,
        Err(error) => {
            return unavailable(
                ordinary,
                None,
                control,
                limits,
                ShadowInfrastructureStage::FinalOrdinaryArtifact,
                Some(error),
            );
        }
    };

    let mut candidate = candidate;
    let candidate_result = catch_unwind(AssertUnwindSafe(|| {
        candidate.engine.execute_prepared_flamingo_shadow_candidate(
            control,
            &prepared_candidate.candidate,
            &prepared_candidate.policy,
        )
    }));
    let applied_frames = match candidate_result {
        Ok(result) => result.applied_frames,
        Err(_) => {
            control.kill_candidate(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            );
            return unavailable_with_artifact(
                ordinary,
                ordinary_artifact,
                None,
                control,
                ShadowInfrastructureStage::CandidateExecutionPanic,
                None,
            );
        }
    };
    let candidate_artifact = match capture(&candidate, limits) {
        Ok(artifact) => artifact,
        Err(error) => {
            // A harness memory-guard overflow is not candidate evidence: keep
            // the candidate alive so later transactions are still compared
            // (the `InitialCandidateArtifact` arm above already does this).
            // Only non-overflow failures kill, matching the candidate-panic
            // arms that indicate genuinely unsafe candidate behavior.
            if !matches!(error, ExecutionArtifactError::LimitExceeded { .. }) {
                control.kill_candidate(
                    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
                );
            }
            return unavailable_with_artifact(
                ordinary,
                ordinary_artifact,
                None,
                control,
                ShadowInfrastructureStage::FinalCandidateArtifact,
                Some(error),
            );
        }
    };

    match ordinary_artifact.compare(&candidate_artifact) {
        Ok(()) if applied_frames == 0 => Ok(outcome(
            ordinary,
            Some(ordinary_artifact),
            Some(candidate_artifact),
            ShadowReplayStatus::CandidateNotApplied,
        )),
        Ok(()) => {
            if !control.record_match(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            ) {
                return unavailable_with_artifact(
                    ordinary,
                    ordinary_artifact,
                    Some(candidate_artifact),
                    control,
                    ShadowInfrastructureStage::CandidateIdentity,
                    None,
                );
            }
            Ok(outcome(
                ordinary,
                Some(ordinary_artifact),
                Some(candidate_artifact),
                ShadowReplayStatus::Matched { applied_frames },
            ))
        }
        Err(mismatch) if applied_frames == 0 => finish_twin_divergence(
            ordinary,
            Some(ordinary_artifact),
            Some(candidate_artifact),
            control,
            mismatch,
        ),
        Err(mismatch) => {
            let mismatch_result = record_mismatch(
                control,
                &prepared_candidate.candidate,
                &ordinary.engine,
                mismatch,
                &ordinary_artifact,
                &candidate_artifact,
                replay_payload,
            );
            finish_mismatch(
                ordinary,
                Some(ordinary_artifact),
                Some(candidate_artifact),
                mismatch_result,
                mismatch,
                applied_frames,
            )
        }
    }
}

fn finish_twin_divergence<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    control: &SpecializationControl,
    mismatch: ExecutionArtifactMismatch,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    tracing::warn!(
        target: "neo::specialization",
        error = %mismatch,
        "shadow twins diverged before specialized execution; candidate remains enabled"
    );
    if control.snapshot().strict_replay {
        return Err(FlamingoShadowRunError::StrictReplay(
            ShadowStrictReplayFailure {
                ordinary_engine: ordinary.into_engine(),
                ordinary_artifact,
                candidate_artifact,
                kind: ShadowStrictReplayFailureKind::TwinDivergence(mismatch),
                infrastructure_error: None,
            },
        ));
    }
    Ok(outcome(
        ordinary,
        ordinary_artifact,
        candidate_artifact,
        ShadowReplayStatus::CandidateUnavailable {
            stage: ShadowInfrastructureStage::TwinDivergenceWithoutCandidate,
        },
    ))
}

fn prepare_branch<P, D, B, F>(
    snapshot: &DataCache<B>,
    branch: ShadowTwinBranch,
    limits: ExecutionArtifactLimits,
    prepare: &mut F,
) -> CoreResult<PreparedShadowEngine<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
    F: FnMut(ShadowTwinBranch, ShadowTwinResources<B>) -> CoreResult<PreparedShadowEngine<P, D, B>>,
{
    let branch_native_cache = Arc::new(Mutex::new(NativeContractsCache::default()));
    let branch_observations = Arc::new(Mutex::new(ExecutionObservationState::new(limits)));
    let storage_observer: Arc<dyn DataCacheReadObserver> = Arc::new(ShadowStorageReadObserver {
        observations: Arc::clone(&branch_observations),
    });
    let branch_snapshot = Arc::new(snapshot.clone_cache().with_read_observer(storage_observer));
    let expected_snapshot = Arc::clone(&branch_snapshot);
    let expected_native_cache = Arc::clone(&branch_native_cache);
    let expected_observations = Arc::clone(&branch_observations);
    let prepared = prepare(
        branch,
        ShadowTwinResources {
            snapshot_cache: branch_snapshot,
            native_contract_cache: branch_native_cache,
            observation_binding: ShadowObservationBinding {
                observations: branch_observations,
            },
        },
    )?;
    if !Arc::ptr_eq(
        &prepared.engine.original_snapshot_cache_handle(),
        &expected_snapshot,
    ) {
        return Err(CoreError::invalid_operation(
            "shadow factory did not retain the runner-owned snapshot overlay",
        ));
    }
    if !Arc::ptr_eq(&prepared.observations, &expected_observations) {
        return Err(CoreError::invalid_operation(
            "shadow factory did not retain the runner-owned observation journal",
        ));
    }
    if !Arc::ptr_eq(
        &prepared.engine.native_contract_cache_handle(),
        &expected_native_cache,
    ) {
        return Err(CoreError::invalid_operation(
            "shadow factory did not retain the runner-owned native cache",
        ));
    }
    Ok(prepared)
}

fn capture<P, D, B>(
    prepared: &PreparedShadowEngine<P, D, B>,
    limits: ExecutionArtifactLimits,
) -> Result<CanonicalExecutionArtifact, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let snapshot = prepared.engine.original_snapshot_cache_handle();
    let _pause = snapshot.pause_read_observation();
    let observations = prepared.observations.lock();
    CanonicalExecutionArtifact::capture(&prepared.engine, observations.journal()?, limits)
}

fn unavailable<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    control: &SpecializationControl,
    limits: ExecutionArtifactLimits,
    stage: ShadowInfrastructureStage,
    infrastructure_error: Option<ExecutionArtifactError>,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let ordinary_artifact = capture(&ordinary, limits).ok();
    unavailable_parts(
        ordinary,
        ordinary_artifact,
        candidate_artifact,
        control,
        stage,
        infrastructure_error,
    )
}

fn unavailable_with_artifact<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    ordinary_artifact: CanonicalExecutionArtifact,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    control: &SpecializationControl,
    stage: ShadowInfrastructureStage,
    infrastructure_error: Option<ExecutionArtifactError>,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    unavailable_parts(
        ordinary,
        Some(ordinary_artifact),
        candidate_artifact,
        control,
        stage,
        infrastructure_error,
    )
}

fn unavailable_parts<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    control: &SpecializationControl,
    stage: ShadowInfrastructureStage,
    infrastructure_error: Option<ExecutionArtifactError>,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    if let Some(error) = &infrastructure_error {
        tracing::warn!(
            target: "neo::specialization",
            ?stage,
            error = %error,
            "specialization shadow artifact capture failed"
        );
    }
    let snapshot = control.snapshot();
    if snapshot.strict_replay {
        // An artifact capture that overflows the configured memory guard is
        // categorically different from a proven mismatch: the candidate was
        // never evaluated, and the ordinary engine remains authoritative.
        // When the campaign explicitly opts in, skip the comparison loudly
        // instead of aborting the replay on pathological transactions.
        let overflowed = matches!(
            infrastructure_error,
            Some(ExecutionArtifactError::LimitExceeded { .. })
        );
        if overflowed && snapshot.artifact_overflow_fallback {
            control.record_overflow_skip(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            );
            tracing::warn!(
                target: "neo::specialization",
                ?stage,
                error = %infrastructure_error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                "strict replay skipping overflowed artifact comparison; ordinary execution remains authoritative"
            );
        } else {
            return Err(FlamingoShadowRunError::StrictReplay(
                ShadowStrictReplayFailure {
                    ordinary_engine: ordinary.into_engine(),
                    ordinary_artifact,
                    candidate_artifact,
                    kind: ShadowStrictReplayFailureKind::Infrastructure(stage),
                    infrastructure_error,
                },
            ));
        }
    }
    Ok(outcome(
        ordinary,
        ordinary_artifact,
        candidate_artifact,
        ShadowReplayStatus::CandidateUnavailable { stage },
    ))
}

fn finish_mismatch<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    mismatch_result: Result<
        crate::specialization::MismatchRecordOutcome,
        SpecializationControlError,
    >,
    mismatch: ExecutionArtifactMismatch,
    applied_frames: u64,
) -> Result<FlamingoShadowOutcome<P, D, B>, FlamingoShadowRunError<P, D, B>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    if let Err(error) = mismatch_result {
        return Err(FlamingoShadowRunError::StrictReplay(
            ShadowStrictReplayFailure {
                ordinary_engine: ordinary.into_engine(),
                ordinary_artifact,
                candidate_artifact,
                kind: ShadowStrictReplayFailureKind::Mismatch(error),
                infrastructure_error: None,
            },
        ));
    }
    Ok(outcome(
        ordinary,
        ordinary_artifact,
        candidate_artifact,
        ShadowReplayStatus::MismatchLatched {
            mismatch,
            applied_frames,
        },
    ))
}

fn outcome<P, D, B>(
    ordinary: PreparedShadowEngine<P, D, B>,
    ordinary_artifact: Option<CanonicalExecutionArtifact>,
    candidate_artifact: Option<CanonicalExecutionArtifact>,
    status: ShadowReplayStatus,
) -> FlamingoShadowOutcome<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    FlamingoShadowOutcome {
        ordinary_engine: ordinary.into_engine(),
        ordinary_artifact,
        candidate_artifact,
        status,
    }
}

fn record_mismatch<P, D, B>(
    control: &SpecializationControl,
    candidate: &CandidateContract,
    ordinary_engine: &ApplicationEngine<P, D, B>,
    mismatch: ExecutionArtifactMismatch,
    ordinary_artifact: &CanonicalExecutionArtifact,
    candidate_artifact: &CanonicalExecutionArtifact,
    replay_payload: &[u8],
) -> Result<crate::specialization::MismatchRecordOutcome, SpecializationControlError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let identity = candidate.identity();
    let execution = identity.execution();
    control.record_mismatch(SpecializationMismatchInput {
        candidate_id: identity.candidate_id(),
        candidate_version: identity.candidate_version(),
        mismatch,
        block_index: ordinary_engine
            .persisting_block()
            .map(|block| block.header.index()),
        transaction_hash: ordinary_engine
            .script_container()
            .and_then(|container| container.hash().ok()),
        script_hash: UInt160::from(*execution.script_hash()),
        entry_ip: execution.entry_ip(),
        ordinary_artifact_digest: artifact_evidence_digest(ordinary_artifact),
        optimized_artifact_digest: artifact_evidence_digest(candidate_artifact),
        payload: replay_payload,
    })
}

/// Hashes the deterministic debug document without allocating a second artifact.
/// The digest is reproducer evidence only; it is never a consensus input.
fn artifact_evidence_digest(artifact: &CanonicalExecutionArtifact) -> [u8; 32] {
    struct DigestWriter(Sha256);

    impl std::fmt::Write for DigestWriter {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0.update(value.as_bytes());
            Ok(())
        }
    }

    let mut writer = DigestWriter(Sha256::new());
    write!(&mut writer, "{artifact:?}").expect("SHA-256 formatter is infallible");
    writer.0.finalize().into()
}

#[cfg(test)]
#[path = "../../tests/application_engine/shadow.rs"]
mod tests;
