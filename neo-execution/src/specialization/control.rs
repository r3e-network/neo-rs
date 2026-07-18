//! Fail-closed process controls for guarded specialization routing.

use crate::{
    ExecutionArtifactComponent, ExecutionArtifactMismatch, ExecutionArtifactMismatchDetail,
};
use neo_primitives::{UInt160, UInt256};
use neo_vm::{CandidateId, CandidateVersion, SpecializationMode};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Hard process bounds for specialization controls and mismatch evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecializationControlLimits {
    /// Maximum explicitly configured candidate versions.
    pub max_candidates: usize,
    /// Maximum first-mismatch reproducers retained process-wide.
    pub max_reproducers: usize,
    /// Maximum aggregate reproducer payload-prefix bytes.
    pub max_reproducer_bytes: usize,
}

impl SpecializationControlLimits {
    /// Conservative bounds for a small audited candidate set.
    pub const DEFAULT: Self = Self {
        max_candidates: 64,
        max_reproducers: 64,
        max_reproducer_bytes: 1024 * 1024,
    };
}

impl Default for SpecializationControlLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Exact candidate version and explicitly permitted routing mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateRouteConfig {
    candidate_id: CandidateId,
    candidate_version: CandidateVersion,
    mode: SpecializationMode,
}

impl CandidateRouteConfig {
    /// Creates one exact-version route declaration.
    #[must_use]
    pub const fn new(
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
        mode: SpecializationMode,
    ) -> Self {
        Self {
            candidate_id,
            candidate_version,
            mode,
        }
    }

    /// Stable candidate ID.
    #[must_use]
    pub const fn candidate_id(self) -> CandidateId {
        self.candidate_id
    }

    /// Exact configured implementation version.
    #[must_use]
    pub const fn candidate_version(self) -> CandidateVersion {
        self.candidate_version
    }

    /// Explicit candidate-specific mode.
    #[must_use]
    pub const fn mode(self) -> SpecializationMode {
        self.mode
    }
}

/// Immutable disabled-by-default specialization process configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecializationControlConfig {
    enabled: bool,
    strict_replay: bool,
    artifact_overflow_fallback: bool,
    limits: SpecializationControlLimits,
    candidates: Arc<[CandidateRouteConfig]>,
}

impl SpecializationControlConfig {
    /// Validates one explicitly enabled candidate configuration.
    pub fn try_enabled(
        strict_replay: bool,
        limits: SpecializationControlLimits,
        candidates: impl IntoIterator<Item = CandidateRouteConfig>,
    ) -> Result<Self, SpecializationControlConfigError> {
        validate_limits(limits)?;
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.len() > limits.max_candidates {
            return Err(SpecializationControlConfigError::CandidateCapacity {
                actual: candidates.len(),
                maximum: limits.max_candidates,
            });
        }
        let mut ids = HashSet::with_capacity(candidates.len());
        for candidate in &candidates {
            if candidate.candidate_id.value() == 0 || candidate.candidate_version.value() == 0 {
                return Err(SpecializationControlConfigError::InvalidCandidateIdentity);
            }
            if candidate.mode == SpecializationMode::Disabled {
                return Err(SpecializationControlConfigError::DisabledCandidateMode);
            }
            if !ids.insert(candidate.candidate_id) {
                return Err(SpecializationControlConfigError::DuplicateCandidateId {
                    candidate_id: candidate.candidate_id,
                });
            }
        }
        candidates.sort_unstable_by_key(|candidate| candidate.candidate_id.value());
        Ok(Self {
            enabled: true,
            strict_replay,
            artifact_overflow_fallback: false,
            limits,
            candidates: candidates.into(),
        })
    }

    /// Allows strict replay to continue ordinary-only when bounded artifact
    /// capture overflows (a harness memory guard), while still aborting on any
    /// proven mismatch. The ordinary engine is always authoritative, so a
    /// skipped comparison never risks canonical state; the skip is logged and
    /// must be accounted for by the promotion gate.
    #[must_use]
    pub const fn with_artifact_overflow_fallback(mut self, allow: bool) -> Self {
        self.artifact_overflow_fallback = allow;
        self
    }

    /// Returns whether strict replay may skip overflowed artifact captures.
    #[must_use]
    pub const fn artifact_overflow_fallback(&self) -> bool {
        self.artifact_overflow_fallback
    }

    /// Returns whether any specialization routing is globally configured.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns whether the first shadow mismatch must fail replay.
    #[must_use]
    pub const fn strict_replay(&self) -> bool {
        self.strict_replay
    }

    /// Returns immutable process bounds.
    #[must_use]
    pub const fn limits(&self) -> SpecializationControlLimits {
        self.limits
    }

    /// Returns exact candidate declarations in candidate-ID order.
    #[must_use]
    pub fn candidates(&self) -> &[CandidateRouteConfig] {
        &self.candidates
    }
}

impl Default for SpecializationControlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strict_replay: false,
            artifact_overflow_fallback: false,
            limits: SpecializationControlLimits::DEFAULT,
            candidates: Arc::from([]),
        }
    }
}

fn validate_limits(
    limits: SpecializationControlLimits,
) -> Result<(), SpecializationControlConfigError> {
    for (name, value) in [
        ("max_candidates", limits.max_candidates),
        ("max_reproducers", limits.max_reproducers),
        ("max_reproducer_bytes", limits.max_reproducer_bytes),
    ] {
        if value == 0 {
            return Err(SpecializationControlConfigError::ZeroLimit { limit: name });
        }
    }
    Ok(())
}

/// Invalid specialization process configuration.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SpecializationControlConfigError {
    /// A hard bound is zero.
    #[error("specialization control limit `{limit}` must be non-zero")]
    ZeroLimit {
        /// Invalid bound.
        limit: &'static str,
    },
    /// Candidate count exceeds its bound.
    #[error("specialization control has {actual} candidates, maximum {maximum}")]
    CandidateCapacity {
        /// Configured candidate count.
        actual: usize,
        /// Maximum candidate count.
        maximum: usize,
    },
    /// Candidate ID or version is reserved zero.
    #[error("specialization candidate ID and version must be non-zero")]
    InvalidCandidateIdentity,
    /// A configured entry cannot use the disabled mode.
    #[error("configured specialization candidate mode must be shadow or authoritative")]
    DisabledCandidateMode,
    /// More than one version was configured for one stable candidate ID.
    #[error("duplicate specialization candidate ID {candidate_id:?}")]
    DuplicateCandidateId {
        /// Duplicated candidate ID.
        candidate_id: CandidateId,
    },
}

/// Why an exact candidate cannot currently route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecializationDisableReason {
    /// Specialization is disabled in process configuration.
    GloballyDisabled,
    /// The irreversible process-global kill switch was activated.
    GlobalKillSwitch,
    /// This candidate ID is not configured.
    CandidateNotConfigured,
    /// The loaded candidate version differs from configuration.
    CandidateVersionMismatch,
    /// The irreversible operator kill switch was activated for this candidate.
    CandidateKillSwitch,
    /// A prior differential mismatch latched the candidate off.
    MismatchLatched,
}

/// Candidate-specific routing decision before exact registry eligibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecializationRouteDecision {
    /// Use ordinary sequential NeoVM.
    Ordinary {
        /// Fail-closed reason.
        reason: SpecializationDisableReason,
    },
    /// Execute only in an isolated shadow; ordinary output stays authoritative.
    Shadow,
    /// Candidate configuration requests opt-in authoritative execution.
    /// Registry authority and all promotion gates must still permit it.
    Authoritative,
}

struct CandidateRuntimeState {
    candidate_version: CandidateVersion,
    mode: SpecializationMode,
    killed: AtomicBool,
    mismatch_latched: AtomicBool,
    matches: AtomicU64,
    mismatches: AtomicU64,
    overflow_skips: AtomicU64,
}

impl CandidateRuntimeState {
    fn new(config: CandidateRouteConfig) -> Self {
        Self {
            candidate_version: config.candidate_version,
            mode: config.mode,
            killed: AtomicBool::new(false),
            mismatch_latched: AtomicBool::new(false),
            matches: AtomicU64::new(0),
            mismatches: AtomicU64::new(0),
            overflow_skips: AtomicU64::new(0),
        }
    }
}

#[derive(Default)]
struct ReproducerStore {
    retained_payload_bytes: usize,
    entries: Vec<SpecializationMismatchReproducer>,
}

struct SpecializationControlInner {
    config: SpecializationControlConfig,
    global_killed: AtomicBool,
    candidates: HashMap<CandidateId, CandidateRuntimeState>,
    reproducers: Mutex<ReproducerStore>,
}

/// Shareable process-local kill switches, mismatch latches, and bounded evidence.
#[derive(Clone)]
pub struct SpecializationControl {
    inner: Arc<SpecializationControlInner>,
}

impl std::fmt::Debug for SpecializationControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpecializationControl")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl SpecializationControl {
    /// Creates controls from an immutable validated configuration.
    #[must_use]
    pub fn new(config: SpecializationControlConfig) -> Self {
        let candidates = config
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.candidate_id,
                    CandidateRuntimeState::new(*candidate),
                )
            })
            .collect();
        Self {
            inner: Arc::new(SpecializationControlInner {
                config,
                global_killed: AtomicBool::new(false),
                candidates,
                reproducers: Mutex::new(ReproducerStore::default()),
            }),
        }
    }

    /// Returns a disabled control handle with no candidate allocation.
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(SpecializationControlConfig::default())
    }

    /// Selects only the configured exact version unless a kill or mismatch latch applies.
    #[must_use]
    pub fn route(
        &self,
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
    ) -> SpecializationRouteDecision {
        if !self.inner.config.enabled {
            return ordinary(SpecializationDisableReason::GloballyDisabled);
        }
        if self.inner.global_killed.load(Ordering::Acquire) {
            return ordinary(SpecializationDisableReason::GlobalKillSwitch);
        }
        let Some(candidate) = self.inner.candidates.get(&candidate_id) else {
            return ordinary(SpecializationDisableReason::CandidateNotConfigured);
        };
        if candidate.candidate_version != candidate_version {
            return ordinary(SpecializationDisableReason::CandidateVersionMismatch);
        }
        if candidate.mismatch_latched.load(Ordering::Acquire) {
            return ordinary(SpecializationDisableReason::MismatchLatched);
        }
        if candidate.killed.load(Ordering::Acquire) {
            return ordinary(SpecializationDisableReason::CandidateKillSwitch);
        }
        match candidate.mode {
            SpecializationMode::Shadow => SpecializationRouteDecision::Shadow,
            SpecializationMode::Authoritative => SpecializationRouteDecision::Authoritative,
            SpecializationMode::Disabled => {
                ordinary(SpecializationDisableReason::CandidateNotConfigured)
            }
        }
    }

    /// Irreversibly disables all specialization routing for this process handle.
    pub fn kill_global(&self) {
        self.inner.global_killed.store(true, Ordering::Release);
    }

    /// Irreversibly disables one configured exact candidate version.
    pub fn kill_candidate(
        &self,
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
    ) -> bool {
        let Some(candidate) = self.inner.candidates.get(&candidate_id) else {
            return false;
        };
        if candidate.candidate_version != candidate_version {
            return false;
        }
        candidate.killed.store(true, Ordering::Release);
        true
    }

    /// Records one matching differential comparison.
    pub fn record_match(
        &self,
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
    ) -> bool {
        let Some(candidate) = self.exact_candidate(candidate_id, candidate_version) else {
            return false;
        };
        candidate.matches.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Records one strict-replay comparison skipped because bounded artifact
    /// capture overflowed a harness memory guard. The promotion gate uses this
    /// counter to tell "no shadow-eligible transactions" from "comparisons
    /// skipped as unverifiable"; the ordinary engine stayed authoritative.
    pub fn record_overflow_skip(
        &self,
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
    ) -> bool {
        let Some(candidate) = self.exact_candidate(candidate_id, candidate_version) else {
            return false;
        };
        candidate.overflow_skips.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Latches a mismatch before retaining its first bounded reproducer.
    ///
    /// Strict replay always returns an error after latching. Non-strict shadow
    /// replay may continue sequentially, but this candidate remains disabled.
    pub fn record_mismatch(
        &self,
        input: SpecializationMismatchInput<'_>,
    ) -> Result<MismatchRecordOutcome, SpecializationControlError> {
        let candidate = self
            .exact_candidate(input.candidate_id, input.candidate_version)
            .ok_or(SpecializationControlError::UnknownCandidateVersion {
                candidate_id: input.candidate_id,
                candidate_version: input.candidate_version,
            })?;
        candidate.mismatches.fetch_add(1, Ordering::Relaxed);
        let first = candidate
            .mismatch_latched
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if first {
            self.retain_reproducer(input);
        }
        let outcome = if first {
            MismatchRecordOutcome::FirstMismatchLatched
        } else {
            MismatchRecordOutcome::AlreadyLatched
        };
        if self.inner.config.strict_replay {
            return Err(SpecializationControlError::StrictReplayMismatch {
                candidate_id: input.candidate_id,
                candidate_version: input.candidate_version,
                component: input.mismatch.component(),
                detail: input.mismatch.detail(),
            });
        }
        Ok(outcome)
    }

    fn exact_candidate(
        &self,
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
    ) -> Option<&CandidateRuntimeState> {
        self.inner
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.candidate_version == candidate_version)
    }

    fn retain_reproducer(&self, input: SpecializationMismatchInput<'_>) {
        let limits = self.inner.config.limits;
        let mut store = self.inner.reproducers.lock();
        if store.entries.len() >= limits.max_reproducers {
            return;
        }
        let remaining = limits
            .max_reproducer_bytes
            .saturating_sub(store.retained_payload_bytes);
        let retained = input.payload.len().min(remaining);
        let payload_prefix = input.payload[..retained].to_vec();
        store.retained_payload_bytes = store.retained_payload_bytes.saturating_add(retained);
        store.entries.push(SpecializationMismatchReproducer {
            candidate_id: input.candidate_id,
            candidate_version: input.candidate_version,
            component: input.mismatch.component(),
            detail: input.mismatch.detail(),
            block_index: input.block_index,
            transaction_hash: input.transaction_hash,
            script_hash: input.script_hash,
            entry_ip: input.entry_ip,
            ordinary_artifact_digest: input.ordinary_artifact_digest,
            optimized_artifact_digest: input.optimized_artifact_digest,
            payload_digest: Sha256::digest(input.payload).into(),
            original_payload_bytes: input.payload.len(),
            payload_prefix,
            payload_truncated: retained != input.payload.len(),
        });
    }

    /// Returns deterministic counters and cloned bounded mismatch evidence.
    #[must_use]
    pub fn snapshot(&self) -> SpecializationControlSnapshot {
        let mut candidates = self
            .inner
            .candidates
            .iter()
            .map(|(candidate_id, state)| CandidateControlSnapshot {
                candidate_id: *candidate_id,
                candidate_version: state.candidate_version,
                mode: state.mode,
                killed: state.killed.load(Ordering::Acquire),
                mismatch_latched: state.mismatch_latched.load(Ordering::Acquire),
                matches: state.matches.load(Ordering::Relaxed),
                mismatches: state.mismatches.load(Ordering::Relaxed),
                overflow_skips: state.overflow_skips.load(Ordering::Relaxed),
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|candidate| candidate.candidate_id.value());
        let reproducers = self.inner.reproducers.lock();
        SpecializationControlSnapshot {
            enabled: self.inner.config.enabled,
            strict_replay: self.inner.config.strict_replay,
            artifact_overflow_fallback: self.inner.config.artifact_overflow_fallback,
            global_killed: self.inner.global_killed.load(Ordering::Acquire),
            candidates,
            retained_reproducer_bytes: reproducers.retained_payload_bytes,
            reproducers: reproducers.entries.clone(),
        }
    }
}

fn ordinary(reason: SpecializationDisableReason) -> SpecializationRouteDecision {
    SpecializationRouteDecision::Ordinary { reason }
}

impl Default for SpecializationControl {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Caller-owned exact context for the first mismatch reproducer.
#[derive(Clone, Copy, Debug)]
pub struct SpecializationMismatchInput<'a> {
    /// Exact candidate ID.
    pub candidate_id: CandidateId,
    /// Exact candidate implementation version.
    pub candidate_version: CandidateVersion,
    /// First canonical artifact mismatch.
    pub mismatch: ExecutionArtifactMismatch,
    /// Block height, when replay is block-bound.
    pub block_index: Option<u32>,
    /// Transaction hash, when the invocation has a transaction container.
    pub transaction_hash: Option<UInt256>,
    /// Exact script hash selected by the registry.
    pub script_hash: UInt160,
    /// Candidate entry instruction pointer.
    pub entry_ip: u32,
    /// Deterministic digest of the ordinary canonical artifact.
    pub ordinary_artifact_digest: [u8; 32],
    /// Deterministic digest of the optimized canonical artifact.
    pub optimized_artifact_digest: [u8; 32],
    /// Bounded caller-defined replay payload, typically transaction and argument bytes.
    pub payload: &'a [u8],
}

/// Result of recording a non-strict mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MismatchRecordOutcome {
    /// This call won the process latch and attempted to retain evidence.
    FirstMismatchLatched,
    /// A prior caller already latched the candidate.
    AlreadyLatched,
}

/// Mismatch handling failure.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SpecializationControlError {
    /// Mismatch was reported for an unconfigured or stale candidate version.
    #[error("unconfigured specialization candidate {candidate_id:?} version {candidate_version:?}")]
    UnknownCandidateVersion {
        /// Candidate ID.
        candidate_id: CandidateId,
        /// Candidate version.
        candidate_version: CandidateVersion,
    },
    /// Strict shadow replay observed a canonical mismatch.
    #[error(
        "strict specialization replay mismatch for {candidate_id:?} version {candidate_version:?} at {component:?}, detail={detail:?}"
    )]
    StrictReplayMismatch {
        /// Candidate ID.
        candidate_id: CandidateId,
        /// Candidate version.
        candidate_version: CandidateVersion,
        /// First mismatching artifact component.
        component: ExecutionArtifactComponent,
        /// Bounded first differing sequence element, when applicable.
        detail: Option<ExecutionArtifactMismatchDetail>,
    },
}

/// Bounded first mismatch reproducer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecializationMismatchReproducer {
    /// Candidate ID.
    pub candidate_id: CandidateId,
    /// Candidate implementation version.
    pub candidate_version: CandidateVersion,
    /// First mismatching artifact component.
    pub component: ExecutionArtifactComponent,
    /// Bounded first differing sequence element, when applicable.
    pub detail: Option<ExecutionArtifactMismatchDetail>,
    /// Optional block height.
    pub block_index: Option<u32>,
    /// Optional transaction hash.
    pub transaction_hash: Option<UInt256>,
    /// Exact script hash.
    pub script_hash: UInt160,
    /// Candidate entry IP.
    pub entry_ip: u32,
    /// Ordinary artifact digest.
    pub ordinary_artifact_digest: [u8; 32],
    /// Optimized artifact digest.
    pub optimized_artifact_digest: [u8; 32],
    /// SHA-256 of the complete supplied payload.
    pub payload_digest: [u8; 32],
    /// Complete payload length before bounding.
    pub original_payload_bytes: usize,
    /// Retained deterministic prefix.
    pub payload_prefix: Vec<u8>,
    /// Whether the retained prefix is incomplete.
    pub payload_truncated: bool,
}

/// Candidate control counters at one point in time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateControlSnapshot {
    /// Candidate ID.
    pub candidate_id: CandidateId,
    /// Configured version.
    pub candidate_version: CandidateVersion,
    /// Configured mode.
    pub mode: SpecializationMode,
    /// Manual candidate kill switch state.
    pub killed: bool,
    /// Differential mismatch latch state.
    pub mismatch_latched: bool,
    /// Matching shadow comparison count.
    pub matches: u64,
    /// Mismatch observation count.
    pub mismatches: u64,
    /// Strict-replay comparisons skipped on artifact memory-guard overflow.
    pub overflow_skips: u64,
}

/// Deterministic process control snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecializationControlSnapshot {
    /// Global configuration enabled state.
    pub enabled: bool,
    /// Strict replay setting.
    pub strict_replay: bool,
    /// Strict replay may skip comparisons whose artifact capture overflows.
    pub artifact_overflow_fallback: bool,
    /// Global kill switch state.
    pub global_killed: bool,
    /// Candidate snapshots in ID order.
    pub candidates: Vec<CandidateControlSnapshot>,
    /// Aggregate retained reproducer prefix bytes.
    pub retained_reproducer_bytes: usize,
    /// First mismatch evidence in latch order.
    pub reproducers: Vec<SpecializationMismatchReproducer>,
}

#[cfg(test)]
#[path = "../tests/specialization/control.rs"]
mod tests;
