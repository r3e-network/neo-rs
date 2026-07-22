use anyhow::Context;
use neo_execution::ExecutionArtifactLimits;
use neo_execution::specialization::{
    CandidateRouteConfig, FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION, SpecializationControl,
    SpecializationControlConfig, SpecializationControlLimits,
};
use neo_vm::SpecializationMode;
use serde::Deserialize;
use std::sync::Arc;

/// `[execution]`: explicitly gated execution experiments.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::node) struct ExecutionSection {
    /// Ordinary-authoritative differential execution for audited candidates.
    #[serde(default)]
    pub(in crate::node) specialization_shadow: SpecializationShadowSection,
    /// Bounded header-witness preverification. Disabled unless explicitly
    /// enabled because it is an execution-policy experiment.
    #[serde(default)]
    pub(in crate::node) optimistic_signature_verification: OptimisticSignatureVerificationSection,
}

/// `[execution.optimistic_signature_verification]`: overlap header-witness
/// signature verification with ordered import. Workers cache only exact P-256
/// outcomes; canonical NeoVM verification and publication remain synchronous.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::node) struct OptimisticSignatureVerificationSection {
    /// Process-wide opt-in. The ordinary synchronous path remains default.
    #[serde(default)]
    enabled: bool,
    /// Number of dedicated state-free ECDSA preverification workers.
    #[serde(default = "default_signature_workers")]
    workers: usize,
    /// Bounded queued jobs behind the workers.
    #[serde(default = "default_signature_queue_capacity")]
    queue_capacity: usize,
}

impl Default for OptimisticSignatureVerificationSection {
    fn default() -> Self {
        Self {
            enabled: false,
            workers: default_signature_workers(),
            queue_capacity: default_signature_queue_capacity(),
        }
    }
}

const fn default_signature_workers() -> usize {
    4
}

const fn default_signature_queue_capacity() -> usize {
    32
}

/// Process resources created only for explicit optimistic verification.
pub(in crate::node) struct OptimisticSignatureVerificationRuntime {
    pub(in crate::node) pool:
        Arc<neo_blockchain::pipeline::signature_verification::SignatureVerificationPool>,
}

impl OptimisticSignatureVerificationSection {
    pub(in crate::node) fn validate(&self, local_execution: bool) -> anyhow::Result<()> {
        let config =
            neo_blockchain::pipeline::signature_verification::SignatureVerificationPoolConfig {
                workers: self.workers,
                queue_capacity: self.queue_capacity,
            };
        config.validate().map_err(|error| {
            anyhow::anyhow!("invalid [execution.optimistic_signature_verification]: {error}")
        })?;
        if self.enabled && !local_execution {
            anyhow::bail!(
                "[execution.optimistic_signature_verification] requires local ledger execution"
            );
        }
        Ok(())
    }

    pub(in crate::node) fn build_runtime(
        &self,
        local_execution: bool,
    ) -> anyhow::Result<Option<OptimisticSignatureVerificationRuntime>> {
        self.validate(local_execution)?;
        if !self.enabled {
            return Ok(None);
        }
        let config =
            neo_blockchain::pipeline::signature_verification::SignatureVerificationPoolConfig {
                workers: self.workers,
                queue_capacity: self.queue_capacity,
            };
        let pool =
            neo_blockchain::pipeline::signature_verification::SignatureVerificationPool::new(
                config,
            )
            .map_err(|error| {
                anyhow::anyhow!("failed to start optimistic signature verification: {error}")
            })?;
        Ok(Some(OptimisticSignatureVerificationRuntime {
            pool: Arc::new(pool),
        }))
    }
}

/// One exact specialization candidate exposed by the node configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SpecializationShadowCandidate {
    FlamingoFactoryPairKeyV1,
}

/// `[execution.specialization_shadow]`: bounded, ordinary-authoritative shadowing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::node) struct SpecializationShadowSection {
    /// Global process opt-in. The default path constructs no control object.
    #[serde(default)]
    enabled: bool,
    /// Abort replay before publication when comparison cannot prove equality.
    #[serde(default)]
    strict_replay: bool,
    /// With `strict_replay`, continue ordinary-only when bounded artifact
    /// capture overflows a memory guard. Proven mismatches still abort.
    #[serde(default)]
    allow_artifact_overflow: bool,
    /// Exact candidate versions to shadow. No implicit candidate is selected.
    #[serde(default)]
    candidates: Vec<SpecializationShadowCandidate>,
    /// Maximum retained first-mismatch reproducers.
    #[serde(default = "default_max_reproducers")]
    max_reproducers: usize,
    /// Maximum aggregate retained reproducer payload-prefix bytes.
    #[serde(default = "default_max_reproducer_bytes")]
    max_reproducer_bytes: usize,
    /// Maximum aggregate dynamic bytes retained by one comparison artifact.
    #[serde(default = "default_max_artifact_bytes")]
    max_artifact_bytes: usize,
    /// Optional override for `ExecutionArtifactLimits::max_stack_roots`.
    #[serde(default)]
    max_stack_roots: Option<usize>,
    /// Optional override for `ExecutionArtifactLimits::max_stack_nodes`.
    #[serde(default)]
    max_stack_nodes: Option<usize>,
    /// Optional override for `ExecutionArtifactLimits::max_stack_edges`.
    #[serde(default)]
    max_stack_edges: Option<usize>,
    /// Optional override for `ExecutionArtifactLimits::max_storage_reads`.
    #[serde(default)]
    max_storage_reads: Option<usize>,
    /// Optional override for `ExecutionArtifactLimits::max_events`.
    #[serde(default)]
    max_events: Option<usize>,
}

impl Default for SpecializationShadowSection {
    fn default() -> Self {
        Self {
            enabled: false,
            strict_replay: false,
            allow_artifact_overflow: false,
            candidates: Vec::new(),
            max_reproducers: default_max_reproducers(),
            max_reproducer_bytes: default_max_reproducer_bytes(),
            max_artifact_bytes: default_max_artifact_bytes(),
            max_stack_roots: None,
            max_stack_nodes: None,
            max_stack_edges: None,
            max_storage_reads: None,
            max_events: None,
        }
    }
}

const fn default_max_reproducers() -> usize {
    SpecializationControlLimits::DEFAULT.max_reproducers
}

const fn default_max_reproducer_bytes() -> usize {
    SpecializationControlLimits::DEFAULT.max_reproducer_bytes
}

const fn default_max_artifact_bytes() -> usize {
    ExecutionArtifactLimits::DEFAULT.max_retained_bytes
}

/// Process resources created only for an explicitly enabled shadow campaign.
pub(in crate::node) struct SpecializationShadowRuntime {
    pub(in crate::node) control: SpecializationControl,
    pub(in crate::node) artifact_limits: ExecutionArtifactLimits,
}

impl SpecializationShadowSection {
    pub(in crate::node) fn validate(&self, local_execution: bool) -> anyhow::Result<()> {
        validate_nonzero_bounded(
            self.max_reproducers,
            SpecializationControlLimits::DEFAULT.max_reproducers,
            "max_reproducers",
        )?;
        validate_nonzero_bounded(
            self.max_reproducer_bytes,
            SpecializationControlLimits::DEFAULT.max_reproducer_bytes,
            "max_reproducer_bytes",
        )?;
        validate_nonzero_bounded(
            self.max_artifact_bytes,
            ExecutionArtifactLimits::DEFAULT.max_retained_bytes,
            "max_artifact_bytes",
        )?;
        // Optional harness-guard overrides must be nonzero so a misconfigured
        // zero cannot disable the bound they replace.
        for (override_value, key) in [
            (self.max_stack_roots, "max_stack_roots"),
            (self.max_stack_nodes, "max_stack_nodes"),
            (self.max_stack_edges, "max_stack_edges"),
            (self.max_storage_reads, "max_storage_reads"),
            (self.max_events, "max_events"),
        ] {
            if matches!(override_value, Some(0)) {
                anyhow::bail!("[execution.specialization_shadow].{key} must be greater than zero");
            }
        }

        if !self.enabled {
            return Ok(());
        }
        if !local_execution {
            anyhow::bail!("[execution.specialization_shadow] requires local ledger execution");
        }
        if self.candidates.is_empty() {
            anyhow::bail!(
                "[execution.specialization_shadow].enabled requires at least one exact candidate"
            );
        }
        if self.candidates.len() > 1 {
            anyhow::bail!(
                "[execution.specialization_shadow].candidates contains a duplicate or unsupported candidate"
            );
        }
        Ok(())
    }

    pub(in crate::node) fn build_runtime(
        &self,
        local_execution: bool,
    ) -> anyhow::Result<Option<SpecializationShadowRuntime>> {
        self.validate(local_execution)?;
        if !self.enabled {
            return Ok(None);
        }

        let routes = self.candidates.iter().map(|candidate| match candidate {
            SpecializationShadowCandidate::FlamingoFactoryPairKeyV1 => CandidateRouteConfig::new(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
                SpecializationMode::Shadow,
            ),
        });
        let control_limits = SpecializationControlLimits {
            max_candidates: 1,
            max_reproducers: self.max_reproducers,
            max_reproducer_bytes: self.max_reproducer_bytes,
        };
        let control_config =
            SpecializationControlConfig::try_enabled(self.strict_replay, control_limits, routes)
                .context("invalid [execution.specialization_shadow] control configuration")?
                .with_artifact_overflow_fallback(self.allow_artifact_overflow);
        let mut artifact_limits = ExecutionArtifactLimits {
            max_retained_bytes: self.max_artifact_bytes,
            ..ExecutionArtifactLimits::DEFAULT
        };
        // Expert overrides for harness memory guards that real MainNet
        // transactions have exceeded (never protocol limits).
        for (override_value, field) in [
            (self.max_stack_roots, &mut artifact_limits.max_stack_roots),
            (self.max_stack_nodes, &mut artifact_limits.max_stack_nodes),
            (self.max_stack_edges, &mut artifact_limits.max_stack_edges),
            (
                self.max_storage_reads,
                &mut artifact_limits.max_storage_reads,
            ),
            (self.max_events, &mut artifact_limits.max_events),
        ] {
            if let Some(value) = override_value {
                *field = value;
            }
        }
        Ok(Some(SpecializationShadowRuntime {
            control: SpecializationControl::new(control_config),
            artifact_limits,
        }))
    }
}

fn validate_nonzero_bounded(value: usize, maximum: usize, key: &str) -> anyhow::Result<()> {
    if value == 0 {
        anyhow::bail!("[execution.specialization_shadow].{key} must be greater than zero");
    }
    if value > maximum {
        anyhow::bail!(
            "[execution.specialization_shadow].{key} must not exceed the hard limit {maximum}"
        );
    }
    Ok(())
}
