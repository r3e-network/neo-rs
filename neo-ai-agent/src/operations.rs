use crate::evidence::{EvidenceAuthority, EvidenceKind, EvidenceRecord, EvidenceSource};
use crate::probe::PROBE_DETERMINISTIC_RULE_VERSION;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use tokio::sync::mpsc;

/// Result of a non-blocking evidence export attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportResult {
    /// The side-channel accepted the evidence.
    Enqueued,
    /// The bounded queue was full and the side-channel event was dropped.
    DroppedFull,
    /// The diagnostic worker was unavailable and the event was dropped.
    DroppedClosed,
}

#[derive(Debug, Default)]
struct ExportStats {
    enqueued: AtomicU64,
    dropped_full: AtomicU64,
    dropped_closed: AtomicU64,
}

/// Snapshot of evidence side-channel counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvidenceExportStats {
    /// Events accepted by the worker queue.
    pub enqueued: u64,
    /// Events dropped because the queue was full.
    pub dropped_full: u64,
    /// Events dropped because no worker was receiving.
    pub dropped_closed: u64,
}

/// Cloneable, bounded, non-blocking evidence side-channel.
#[derive(Debug, Clone)]
pub struct EvidenceExporter {
    sender: mpsc::Sender<EvidenceRecord>,
    stats: Arc<ExportStats>,
}

/// Single-consumer side of the diagnostic evidence queue.
#[derive(Debug)]
pub struct EvidenceReceiver {
    receiver: mpsc::Receiver<EvidenceRecord>,
}

impl EvidenceExporter {
    /// Creates a bounded side-channel. Capacity must be non-zero.
    pub fn bounded(capacity: usize) -> Result<(Self, EvidenceReceiver), EvidenceExportError> {
        if capacity == 0 {
            return Err(EvidenceExportError::ZeroCapacity);
        }
        let (sender, receiver) = mpsc::channel(capacity);
        Ok((
            Self {
                sender,
                stats: Arc::new(ExportStats::default()),
            },
            EvidenceReceiver { receiver },
        ))
    }

    /// Attempts to export evidence without waiting or applying backpressure.
    #[must_use]
    pub fn try_export(&self, evidence: EvidenceRecord) -> ExportResult {
        match self.sender.try_send(evidence) {
            Ok(()) => {
                self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                ExportResult::Enqueued
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.stats.dropped_full.fetch_add(1, Ordering::Relaxed);
                ExportResult::DroppedFull
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.stats.dropped_closed.fetch_add(1, Ordering::Relaxed);
                ExportResult::DroppedClosed
            }
        }
    }

    /// Returns monotonically increasing side-channel counters.
    #[must_use]
    pub fn stats(&self) -> EvidenceExportStats {
        EvidenceExportStats {
            enqueued: self.stats.enqueued.load(Ordering::Relaxed),
            dropped_full: self.stats.dropped_full.load(Ordering::Relaxed),
            dropped_closed: self.stats.dropped_closed.load(Ordering::Relaxed),
        }
    }
}

impl EvidenceReceiver {
    /// Receives the next diagnostic event. This wait occurs only in the worker.
    pub async fn recv(&mut self) -> Option<EvidenceRecord> {
        self.receiver.recv().await
    }
}

/// Evidence side-channel construction failure.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceExportError {
    /// A bounded queue cannot have zero capacity.
    #[error("evidence queue capacity must be non-zero")]
    ZeroCapacity,
}

/// Fixed node-side operational action catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalAction {
    /// Temporarily increase diagnostic sampling.
    IncreaseSampling,
    /// Ask the restricted probe interface to collect additional evidence.
    StartProbeRequest,
    /// Add independent metadata that affects only local proposal selection.
    QuarantineTransaction,
    /// Remove local proposal-selection metadata.
    ReleaseTransaction,
    /// Restart only the AI diagnostic worker.
    RestartAiWorker,
    /// Restart consensus after explicit enablement and safety checks.
    RestartConsensus,
}

/// Evidence reference used by the operational policy gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEvidence {
    /// Stable evidence identifier.
    pub id: String,
    /// Expected hash of the receiver-validated evidence record.
    pub content_hash: String,
    /// Claimed producer authority. The node runtime must verify this against its registry.
    pub authority: EvidenceAuthority,
}

/// Request evaluated against the fixed operational policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationalActionRequest {
    /// Requested catalog action.
    pub action: OperationalAction,
    /// Transaction hash for transaction-scoped actions.
    pub target: Option<String>,
    /// Evidence supplied to the gate.
    pub evidence: Vec<ActionEvidence>,
    /// Whether a model recommendation was one input to the request.
    pub model_recommended: bool,
}

/// Safe-by-default policy configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationalPolicyConfig {
    /// Enables the policy engine.
    pub enabled: bool,
    /// Immediately disables every action when set.
    pub kill_switch: bool,
    /// Evaluates and audits actions but never executes them.
    pub observe_only: bool,
    /// Explicitly enables bounded transaction quarantine. Disabled by default.
    pub allow_transaction_quarantine: bool,
    /// Explicitly enables the otherwise disabled consensus restart action.
    pub allow_consensus_restart: bool,
}

impl Default for OperationalPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            kill_switch: false,
            observe_only: true,
            allow_transaction_quarantine: false,
            allow_consensus_restart: false,
        }
    }
}

/// Runtime state required for bounded safety checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OperationalSafetyState {
    /// A local Commit exists that has not been durably persisted.
    pub has_unpersisted_commit: bool,
}

/// Auditable policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalDecision {
    /// The fixed policy permits execution.
    Execute,
    /// Observe-only mode recorded the otherwise permitted action.
    ObserveOnly,
    /// The engine is disabled.
    Disabled,
    /// The emergency kill switch is active.
    Killed,
    /// A deterministic evidence attestation is required.
    MissingDeterministicEvidence,
    /// The referenced evidence ID or content hash is not receiver-validated.
    InvalidEvidence,
    /// The referenced evidence is older than the configured policy window.
    StaleEvidence,
    /// Transaction-scoped evidence is not bound to the requested target.
    EvidenceTargetMismatch,
    /// Evidence was produced by a different deterministic rule version.
    EvidenceRuleMismatch,
    /// A transaction-scoped action omitted its target hash.
    MissingTarget,
    /// Transaction quarantine was not explicitly enabled.
    TransactionQuarantineDisabled,
    /// Quarantine was not authorized by the fixed probe rule.
    MissingProbeRuleFinding,
    /// Model participation is forbidden in a quarantine authorization chain.
    ModelInfluencedQuarantine,
    /// Consensus restart was not explicitly enabled.
    ConsensusRestartDisabled,
    /// Consensus restart is unsafe while a Commit is not durable.
    UnpersistedCommit,
}

/// Deterministic operational policy gate. It performs no action itself.
#[derive(Debug, Clone, Copy)]
pub struct OperationalPolicy {
    config: OperationalPolicyConfig,
}

impl OperationalPolicy {
    /// Creates a policy gate from immutable safe-by-default configuration.
    #[must_use]
    pub const fn new(config: OperationalPolicyConfig) -> Self {
        Self { config }
    }

    /// Evaluates one request independently of model output content.
    #[must_use]
    pub fn evaluate(
        &self,
        request: &OperationalActionRequest,
        safety: OperationalSafetyState,
        records: &[EvidenceRecord],
    ) -> OperationalDecision {
        if !self.config.enabled {
            return OperationalDecision::Disabled;
        }
        if self.config.kill_switch {
            return OperationalDecision::Killed;
        }
        if records.len() != request.evidence.len()
            || records
                .iter()
                .zip(&request.evidence)
                .any(|(record, reference)| {
                    record.verify_integrity().is_err()
                        || record.id != reference.id
                        || record.content_hash != reference.content_hash
                        || record.authority != reference.authority
                })
        {
            return OperationalDecision::InvalidEvidence;
        }
        let deterministic = records
            .iter()
            .any(|record| record.authority == EvidenceAuthority::DeterministicRule);
        if !deterministic {
            return OperationalDecision::MissingDeterministicEvidence;
        }
        if request.action != OperationalAction::QuarantineTransaction
            && records
                .iter()
                .any(|record| record.kind == EvidenceKind::ProbeRuleFinding)
        {
            return OperationalDecision::InvalidEvidence;
        }
        if matches!(
            request.action,
            OperationalAction::QuarantineTransaction | OperationalAction::ReleaseTransaction
        ) && request.target.as_deref().is_none_or(str::is_empty)
        {
            return OperationalDecision::MissingTarget;
        }
        if request.action == OperationalAction::QuarantineTransaction {
            if !self.config.allow_transaction_quarantine {
                return OperationalDecision::TransactionQuarantineDisabled;
            }
            if request.model_recommended {
                return OperationalDecision::ModelInfluencedQuarantine;
            }
            let target = request.target.as_deref();
            if !records.iter().any(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.kind == EvidenceKind::ProbeRuleFinding
                    && record.source == EvidenceSource::DeterministicDetector
                    && record.transaction_hash.as_deref() == target
                    && record.rule_version == PROBE_DETERMINISTIC_RULE_VERSION
                    && probe_finding_context_is_bound(record)
                    && record
                        .item
                        .fields()
                        .get("finding")
                        .and_then(serde_json::Value::as_str)
                        == Some("transaction_isolation_candidate")
            }) {
                return OperationalDecision::MissingProbeRuleFinding;
            }
        }
        if request.action == OperationalAction::RestartConsensus {
            if !self.config.allow_consensus_restart {
                return OperationalDecision::ConsensusRestartDisabled;
            }
            if safety.has_unpersisted_commit {
                return OperationalDecision::UnpersistedCommit;
            }
        }
        if self.config.observe_only {
            OperationalDecision::ObserveOnly
        } else {
            OperationalDecision::Execute
        }
    }
}

fn probe_finding_context_is_bound(record: &EvidenceRecord) -> bool {
    let fields = record.item.fields();
    let Some(context_hash) = fields
        .get("context_hash")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    context_hash.len() == 64
        && context_hash != "00".repeat(32)
        && context_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && fields
            .get("snapshot_height")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(record.tip_height))
        && fields
            .get("snapshot_root")
            .and_then(serde_json::Value::as_str)
            == record.tip_hash.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceItem, EvidenceSource};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn evidence(id: &str) -> EvidenceRecord {
        let item = EvidenceItem::new(
            id,
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            1,
            BTreeMap::new(),
        )
        .expect("evidence item");
        EvidenceRecord::new(item, EvidenceAuthority::Observation, 1, None, "test-v1")
            .expect("evidence record")
    }

    fn deterministic_evidence(id: &str) -> EvidenceRecord {
        let item = EvidenceItem::new(
            id,
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            1,
            BTreeMap::from([("rule_version".to_owned(), json!("test-v1"))]),
        )
        .expect("deterministic evidence item");
        EvidenceRecord::new(
            item,
            EvidenceAuthority::DeterministicRule,
            1,
            None,
            "test-v1",
        )
        .expect("deterministic evidence record")
    }

    fn probe_isolation_finding(id: &str, target: &str) -> EvidenceRecord {
        let item = EvidenceItem::new(
            id,
            EvidenceKind::ProbeRuleFinding,
            EvidenceSource::DeterministicDetector,
            1,
            BTreeMap::from([
                ("transaction_hash".to_owned(), json!(target)),
                (
                    "finding".to_owned(),
                    json!("transaction_isolation_candidate"),
                ),
                ("context_hash".to_owned(), json!("cd".repeat(32))),
                ("snapshot_height".to_owned(), json!(1)),
                ("snapshot_root".to_owned(), json!("ef".repeat(32))),
                (
                    "rule_version".to_owned(),
                    json!(PROBE_DETERMINISTIC_RULE_VERSION),
                ),
            ]),
        )
        .unwrap();
        EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::DeterministicRule,
            1,
            Some(target.to_owned()),
            1,
            Some("ef".repeat(32)),
            PROBE_DETERMINISTIC_RULE_VERSION,
        )
        .unwrap()
    }

    fn evidence_reference(record: &EvidenceRecord) -> ActionEvidence {
        ActionEvidence {
            id: record.id.clone(),
            content_hash: record.content_hash.clone(),
            authority: record.authority,
        }
    }

    #[tokio::test]
    async fn full_or_closed_queue_never_blocks_consensus_path() {
        let (exporter, mut receiver) = EvidenceExporter::bounded(1).expect("queue");
        assert_eq!(exporter.try_export(evidence("one")), ExportResult::Enqueued);
        assert_eq!(
            exporter.try_export(evidence("two")),
            ExportResult::DroppedFull
        );
        assert_eq!(receiver.recv().await.expect("first").id, "one");
        drop(receiver);
        assert_eq!(
            exporter.try_export(evidence("three")),
            ExportResult::DroppedClosed
        );
        assert_eq!(
            exporter.stats(),
            EvidenceExportStats {
                enqueued: 1,
                dropped_full: 1,
                dropped_closed: 1,
            }
        );
    }

    #[test]
    fn model_output_alone_cannot_quarantine() {
        let policy = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            ..OperationalPolicyConfig::default()
        });
        let model_item = EvidenceItem::new(
            "model-1",
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            1,
            BTreeMap::new(),
        )
        .unwrap();
        let model =
            EvidenceRecord::new(model_item, EvidenceAuthority::Model, 1, None, "test-v1").unwrap();
        let request = OperationalActionRequest {
            action: OperationalAction::QuarantineTransaction,
            target: Some("hash".to_string()),
            evidence: vec![evidence_reference(&model)],
            model_recommended: true,
        };
        assert_eq!(
            policy.evaluate(&request, OperationalSafetyState::default(), &[model]),
            OperationalDecision::MissingDeterministicEvidence
        );
    }

    #[test]
    fn quarantine_is_disabled_by_default_and_requires_fixed_probe_finding() {
        let record = deterministic_evidence("generic-rule");
        let request = OperationalActionRequest {
            action: OperationalAction::QuarantineTransaction,
            target: Some("hash".to_string()),
            evidence: vec![evidence_reference(&record)],
            model_recommended: false,
        };
        let disabled = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            ..OperationalPolicyConfig::default()
        });
        assert_eq!(
            disabled.evaluate(
                &request,
                OperationalSafetyState::default(),
                &[record.clone()]
            ),
            OperationalDecision::TransactionQuarantineDisabled
        );
        let enabled = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            allow_transaction_quarantine: true,
            ..OperationalPolicyConfig::default()
        });
        assert_eq!(
            enabled.evaluate(&request, OperationalSafetyState::default(), &[record]),
            OperationalDecision::MissingProbeRuleFinding
        );
    }

    #[test]
    fn enabled_quarantine_accepts_only_fixed_target_bound_probe_finding() {
        let target = "ab".repeat(32);
        let record = probe_isolation_finding("fixed-probe-finding", &target);
        let policy = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            allow_transaction_quarantine: true,
            ..OperationalPolicyConfig::default()
        });
        let request = OperationalActionRequest {
            action: OperationalAction::QuarantineTransaction,
            target: Some(target),
            evidence: vec![evidence_reference(&record)],
            model_recommended: false,
        };
        assert_eq!(
            policy.evaluate(&request, OperationalSafetyState::default(), &[record]),
            OperationalDecision::Execute
        );
    }

    #[test]
    fn model_participation_rejects_even_with_fixed_probe_finding() {
        let target = "bc".repeat(32);
        let record = probe_isolation_finding("model-influenced-finding", &target);
        let policy = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            allow_transaction_quarantine: true,
            ..OperationalPolicyConfig::default()
        });
        let request = OperationalActionRequest {
            action: OperationalAction::QuarantineTransaction,
            target: Some(target),
            evidence: vec![evidence_reference(&record)],
            model_recommended: true,
        };
        assert_eq!(
            policy.evaluate(&request, OperationalSafetyState::default(), &[record]),
            OperationalDecision::ModelInfluencedQuarantine
        );
    }

    #[test]
    fn consensus_restart_is_disabled_and_checks_unpersisted_commit() {
        let record = deterministic_evidence("stall-rule-1");
        let request = OperationalActionRequest {
            action: OperationalAction::RestartConsensus,
            target: None,
            evidence: vec![evidence_reference(&record)],
            model_recommended: false,
        };
        let disabled = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            ..OperationalPolicyConfig::default()
        });
        assert_eq!(
            disabled.evaluate(
                &request,
                OperationalSafetyState::default(),
                &[record.clone()]
            ),
            OperationalDecision::ConsensusRestartDisabled
        );
        let enabled = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            allow_consensus_restart: true,
            ..OperationalPolicyConfig::default()
        });
        assert_eq!(
            enabled.evaluate(
                &request,
                OperationalSafetyState {
                    has_unpersisted_commit: true,
                },
                &[record],
            ),
            OperationalDecision::UnpersistedCommit
        );
    }
}
