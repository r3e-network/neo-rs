//! Immutable exact-key candidate registry with fail-closed routing.

use super::{
    CandidateAuthority, CandidateContract, CandidateContractError, CandidateContractLimits,
    CandidateId, CandidateVersion,
};
use crate::{ExecutionPlanKey, StackItem};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Hard entry and contract-payload limits for a [`SpecializationRegistry`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecializationRegistryLimits {
    /// Maximum registered candidate count.
    pub max_candidates: usize,
    /// Maximum deterministic candidate contract payload bytes.
    pub max_contract_bytes: usize,
    /// Per-candidate declaration limits, rechecked during registry construction.
    pub candidate: CandidateContractLimits,
}

impl SpecializationRegistryLimits {
    /// Conservative bounds for a small manually audited registry.
    pub const DEFAULT: Self = Self {
        max_candidates: 64,
        max_contract_bytes: 4 * 1024 * 1024,
        candidate: CandidateContractLimits::DEFAULT,
    };
}

impl Default for SpecializationRegistryLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Explicit routing mode. The default always selects ordinary execution.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum SpecializationMode {
    /// Do not select a specialized candidate.
    #[default]
    Disabled,
    /// Select an eligible candidate only for isolated differential shadowing.
    Shadow,
    /// Select an eligible candidate for authoritative execution when promoted.
    Authoritative,
}

/// Exact lookup and eligibility outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecializationSelection<'a> {
    /// Specialization routing is disabled.
    Disabled,
    /// No candidate has the exact execution identity.
    NoExactCandidate,
    /// Exact identity matched but normalized arguments did not.
    IneligibleArguments,
    /// Authoritative routing was requested for a shadow-only candidate.
    AuthorityNotPermitted,
    /// Candidate may proceed to context/dependency auditing in the given mode.
    Selected {
        /// Validated immutable declaration. It contains no execution output.
        candidate: &'a CandidateContract,
        /// Explicit non-disabled routing mode.
        mode: SpecializationMode,
    },
}

/// Point-in-time immutable registry sizing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistrySnapshot {
    /// Registered exact candidate count.
    pub candidates: usize,
    /// Deterministically accounted candidate contract payload bytes.
    pub contract_bytes: usize,
    /// Configured candidate count bound.
    pub max_candidates: usize,
    /// Configured contract payload byte bound.
    pub max_contract_bytes: usize,
}

/// Immutable exact-key registry of validated candidate effect contracts.
///
/// Registry presence never enables a candidate. [`SpecializationMode`] defaults
/// to disabled, shadow and authority require explicit caller configuration, and
/// a shadow-only declaration cannot be selected authoritatively.
#[derive(Debug)]
pub struct SpecializationRegistry {
    limits: SpecializationRegistryLimits,
    candidates: Arc<[CandidateContract]>,
    by_execution: HashMap<ExecutionPlanKey, usize>,
    contract_bytes: usize,
}

impl SpecializationRegistry {
    /// Builds one bounded immutable registry.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid bounds, invalid candidate declarations,
    /// duplicate exact execution identities, duplicate candidate versions, or
    /// entry/byte capacity exhaustion.
    pub fn try_new(
        candidates: impl IntoIterator<Item = CandidateContract>,
        limits: SpecializationRegistryLimits,
    ) -> Result<Self, RegistryBuildError> {
        if limits.max_candidates == 0 {
            return Err(RegistryBuildError::ZeroLimit {
                limit: "max_candidates",
            });
        }
        if limits.max_contract_bytes == 0 {
            return Err(RegistryBuildError::ZeroLimit {
                limit: "max_contract_bytes",
            });
        }

        let mut stored = Vec::new();
        let mut by_execution = HashMap::new();
        let mut versions = HashSet::new();
        let mut contract_bytes = 0usize;

        for candidate in candidates {
            candidate
                .validate_against(limits.candidate)
                .map_err(|source| RegistryBuildError::InvalidCandidate {
                    candidate_id: candidate.identity().candidate_id(),
                    candidate_version: candidate.identity().candidate_version(),
                    source,
                })?;

            if stored.len() == limits.max_candidates {
                return Err(RegistryBuildError::CandidateCapacity {
                    maximum: limits.max_candidates,
                });
            }
            let next_bytes = contract_bytes.saturating_add(candidate.accounted_bytes());
            if next_bytes > limits.max_contract_bytes {
                return Err(RegistryBuildError::ByteCapacity {
                    required: next_bytes,
                    maximum: limits.max_contract_bytes,
                });
            }

            let version_key = (
                candidate.identity().candidate_id(),
                candidate.identity().candidate_version(),
            );
            if !versions.insert(version_key) {
                return Err(RegistryBuildError::DuplicateCandidateVersion {
                    candidate_id: version_key.0,
                    candidate_version: version_key.1,
                });
            }
            let index = stored.len();
            if by_execution
                .insert(candidate.identity().execution().clone(), index)
                .is_some()
            {
                return Err(RegistryBuildError::DuplicateExecutionIdentity);
            }
            contract_bytes = next_bytes;
            stored.push(candidate);
        }

        Ok(Self {
            limits,
            candidates: stored.into(),
            by_execution,
            contract_bytes,
        })
    }

    /// Returns a candidate only for full exact execution-key equality.
    ///
    /// This metadata lookup does not enable or execute the candidate.
    #[must_use]
    pub fn lookup_exact(&self, execution: &ExecutionPlanKey) -> Option<&CandidateContract> {
        self.by_execution
            .get(execution)
            .and_then(|index| self.candidates.get(*index))
    }

    /// Performs fail-closed exact identity, authority, and argument selection.
    ///
    /// A selected candidate must still pass its declared slot, context, state,
    /// and host-access auditing before effects can become visible.
    #[must_use]
    pub fn select<'a>(
        &'a self,
        execution: &ExecutionPlanKey,
        arguments: &[StackItem],
        mode: SpecializationMode,
    ) -> SpecializationSelection<'a> {
        if mode == SpecializationMode::Disabled {
            return SpecializationSelection::Disabled;
        }
        let Some(candidate) = self.lookup_exact(execution) else {
            return SpecializationSelection::NoExactCandidate;
        };
        if !candidate.eligibility().matches_arguments(arguments) {
            return SpecializationSelection::IneligibleArguments;
        }
        if mode == SpecializationMode::Authoritative
            && candidate.authority() != CandidateAuthority::OptInAuthoritative
        {
            return SpecializationSelection::AuthorityNotPermitted;
        }
        SpecializationSelection::Selected { candidate, mode }
    }

    /// Returns immutable registry sizing and bounds.
    #[must_use]
    pub fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot {
            candidates: self.candidates.len(),
            contract_bytes: self.contract_bytes,
            max_candidates: self.limits.max_candidates,
            max_contract_bytes: self.limits.max_contract_bytes,
        }
    }

    /// Returns validated candidates in deterministic registration order.
    #[must_use]
    pub fn candidates(&self) -> &[CandidateContract] {
        &self.candidates
    }
}

/// Registry construction failure.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RegistryBuildError {
    /// A registry hard limit is zero.
    #[error("specialization registry limit `{limit}` must be non-zero")]
    ZeroLimit {
        /// Invalid limit name.
        limit: &'static str,
    },
    /// One candidate violates the registry's per-candidate declaration limits.
    #[error("candidate {candidate_id:?} version {candidate_version:?} is invalid: {source}")]
    InvalidCandidate {
        /// Stable candidate ID.
        candidate_id: CandidateId,
        /// Candidate implementation version.
        candidate_version: CandidateVersion,
        /// Exact declaration error.
        source: CandidateContractError,
    },
    /// Candidate entry bound is exhausted.
    #[error("specialization registry candidate capacity exceeded (maximum {maximum})")]
    CandidateCapacity {
        /// Configured maximum candidate count.
        maximum: usize,
    },
    /// Candidate contract payload byte bound is exhausted.
    #[error("specialization registry requires {required} contract bytes, maximum {maximum}")]
    ByteCapacity {
        /// Required deterministic payload bytes.
        required: usize,
        /// Configured maximum payload bytes.
        maximum: usize,
    },
    /// Two candidates use one stable ID and candidate version.
    #[error("duplicate specialization candidate ID/version")]
    DuplicateCandidateVersion {
        /// Stable candidate ID.
        candidate_id: CandidateId,
        /// Repeated implementation version.
        candidate_version: CandidateVersion,
    },
    /// Two candidates target the same exact execution identity.
    #[error("duplicate exact specialization execution identity")]
    DuplicateExecutionIdentity,
}
