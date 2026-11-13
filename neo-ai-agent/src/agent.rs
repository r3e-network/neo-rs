use serde::Serialize;
use thiserror::Error;

use crate::audit::{AuditError, AuditEventKind, AuditInput, AuditLog};
use crate::diagnosis::{Diagnosis, DiagnosisError, DiagnosisValidator};
use crate::evidence::{EvidenceBundle, EvidenceError};
use crate::provider::{ModelProvider, ModelRequest, ProviderError};
use crate::stall::{ConsensusObservation, DetectionResult, StallDetector};

const SYSTEM_INSTRUCTION: &str = r"Return one bare JSON object only. Use diagnosis schema_version=1 and evidence_schema_version=1. Cite only supplied evidence IDs. observed_height must equal the supplied current height. Allowed actions are: observe_only, alert_operator, capture_consensus_snapshot, request_validator_review, enter_safe_maintenance. Do not invent actions, evidence, commands, URLs, files, wallets, signers, or tools. The response is advisory and cannot execute actions.";

#[derive(Debug)]
pub struct DiagnosticReport {
    pub detection: DetectionResult,
    pub diagnosis: Option<Diagnosis>,
    pub model_failure: Option<ModelFailure>,
}

#[derive(Debug, Error)]
pub enum ModelFailure {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Rejected(#[from] DiagnosisError),
}

pub struct DiagnosticAgent<P, A> {
    detector: StallDetector,
    provider: P,
    audit: A,
    validator: DiagnosisValidator,
    max_output_tokens: u32,
}

impl<P: ModelProvider, A: AuditLog> DiagnosticAgent<P, A> {
    #[must_use]
    pub const fn new(
        detector: StallDetector,
        provider: P,
        audit: A,
        validator: DiagnosisValidator,
        max_output_tokens: u32,
    ) -> Self {
        Self {
            detector,
            provider,
            audit,
            validator,
            max_output_tokens,
        }
    }

    pub async fn analyze(
        &self,
        observation: &ConsensusObservation,
        evidence: &EvidenceBundle,
    ) -> Result<DiagnosticReport, AgentError> {
        evidence.validate()?;
        if evidence.tip_height != observation.local_height {
            return Err(AgentError::EvidenceHeightMismatch {
                evidence: evidence.tip_height,
                observation: observation.local_height,
            });
        }

        let detection = self.detector.detect(observation);
        let evidence_refs = evidence
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let evidence_hashes = evidence
            .items
            .iter()
            .map(super::evidence::EvidenceItem::content_hash)
            .collect::<Result<Vec<_>, _>>()?;
        self.audit
            .append(AuditInput {
                timestamp_unix_ms: observation.observed_at_unix_ms,
                kind: AuditEventKind::DetectionCompleted,
                observed_height: observation.local_height,
                evidence_refs: evidence_refs.clone(),
                evidence_hashes: evidence_hashes.clone(),
                action: None,
                target_hash: None,
                rule_version: None,
                outcome_code: detection_code(&detection).to_owned(),
            })
            .await?;

        let Some(level) = detection.stall_level() else {
            return Ok(DiagnosticReport {
                detection,
                diagnosis: None,
                model_failure: None,
            });
        };

        self.audit
            .append(AuditInput {
                timestamp_unix_ms: observation.observed_at_unix_ms,
                kind: AuditEventKind::ModelRequested,
                observed_height: observation.local_height,
                evidence_refs: evidence_refs.clone(),
                evidence_hashes: evidence_hashes.clone(),
                action: None,
                target_hash: None,
                rule_version: None,
                outcome_code: "requested".to_owned(),
            })
            .await?;

        #[derive(Serialize)]
        struct ModelEnvelope<'a> {
            current_height: u32,
            detection: &'a DetectionResult,
            evidence: &'a EvidenceBundle,
        }
        let evidence_json = serde_json::to_string(&ModelEnvelope {
            current_height: observation.local_height,
            detection: &detection,
            evidence,
        })
        .map_err(|_| AgentError::Serialization)?;
        let response = self
            .provider
            .complete(ModelRequest {
                system_instruction: SYSTEM_INSTRUCTION.to_owned(),
                evidence_json,
                max_output_tokens: self.max_output_tokens,
            })
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.audit
                    .append(AuditInput {
                        timestamp_unix_ms: observation.observed_at_unix_ms,
                        kind: AuditEventKind::ProviderFailed,
                        observed_height: observation.local_height,
                        evidence_hashes,
                        evidence_refs,
                        action: None,
                        target_hash: None,
                        rule_version: None,
                        outcome_code: provider_error_code(&error).to_owned(),
                    })
                    .await?;
                return Ok(DiagnosticReport {
                    detection,
                    diagnosis: None,
                    model_failure: Some(ModelFailure::Provider(error)),
                });
            }
        };

        match self.validator.parse_and_validate(
            &response.content,
            evidence,
            level,
            observation.local_height,
        ) {
            Ok(diagnosis) => {
                self.audit
                    .append(AuditInput {
                        timestamp_unix_ms: observation.observed_at_unix_ms,
                        kind: AuditEventKind::DiagnosisAccepted,
                        observed_height: observation.local_height,
                        evidence_hashes,
                        evidence_refs,
                        action: None,
                        target_hash: None,
                        rule_version: None,
                        outcome_code: "accepted".to_owned(),
                    })
                    .await?;
                Ok(DiagnosticReport {
                    detection,
                    diagnosis: Some(diagnosis),
                    model_failure: None,
                })
            }
            Err(error) => {
                self.audit
                    .append(AuditInput {
                        timestamp_unix_ms: observation.observed_at_unix_ms,
                        kind: AuditEventKind::DiagnosisRejected,
                        observed_height: observation.local_height,
                        evidence_hashes,
                        evidence_refs,
                        action: None,
                        target_hash: None,
                        rule_version: None,
                        outcome_code: "policy_rejected".to_owned(),
                    })
                    .await?;
                Ok(DiagnosticReport {
                    detection,
                    diagnosis: None,
                    model_failure: Some(ModelFailure::Rejected(error)),
                })
            }
        }
    }
}

fn detection_code(detection: &DetectionResult) -> &'static str {
    match detection {
        DetectionResult::Healthy { .. } => "healthy",
        DetectionResult::Stalled { .. } => "stalled",
        DetectionResult::Unknown { .. } => "unknown",
    }
}

fn provider_error_code(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::InvalidConfig(_) => "invalid_config",
        ProviderError::MissingCredential(_) => "missing_credential",
        ProviderError::Timeout => "timeout",
        ProviderError::BudgetExhausted => "budget_exhausted",
        ProviderError::CircuitOpen => "circuit_open",
        ProviderError::Transport(_) => "transport_error",
        ProviderError::Http { .. } => "http_error",
        ProviderError::InvalidResponse(_) => "invalid_response",
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
    #[error(transparent)]
    Audit(#[from] AuditError),
    #[error("evidence tip height {evidence} does not match observation height {observation}")]
    EvidenceHeightMismatch { evidence: u32, observation: u32 },
    #[error("failed to serialize redacted model input")]
    Serialization,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::audit::MemoryAuditLog;
    use crate::diagnosis::DiagnosisValidator;
    use crate::evidence::{EvidenceItem, EvidenceKind, EvidenceSource};
    use crate::provider::{CircuitBreakerConfig, GovernedProvider, ModelResponse, RuntimeLimits};
    use crate::stall::{StallLevel, StallPolicy};

    struct MockProvider {
        response: Result<String, ProviderError>,
        delay: Duration,
        calls: AtomicUsize,
    }

    impl MockProvider {
        fn response(content: String) -> Self {
            Self {
                response: Ok(content),
                delay: Duration::ZERO,
                calls: AtomicUsize::new(0),
            }
        }

        fn failure(error: ProviderError) -> Self {
            Self {
                response: Err(error),
                delay: Duration::ZERO,
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.response.clone().map(|content| ModelResponse {
                content,
                input_tokens: Some(10),
                output_tokens: Some(10),
            })
        }
    }

    fn observation() -> ConsensusObservation {
        ConsensusObservation {
            local_height: 10,
            network_height: Some(12),
            max_peer_height: Some(12),
            last_progress_unix_ms: 1_000,
            observed_at_unix_ms: 401_000,
            view_number: 3,
            connected_peers: 4,
            active_validators: 7,
            expected_validators: 7,
        }
    }

    fn evidence() -> EvidenceBundle {
        let detector = EvidenceItem::new(
            "detector-1",
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            10,
            BTreeMap::from([("no_progress_ms".to_owned(), json!(400_000))]),
        )
        .unwrap();
        let consensus = EvidenceItem::new(
            "consensus-1",
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            10,
            BTreeMap::from([("view".to_owned(), json!(3))]),
        )
        .unwrap();
        EvidenceBundle::new(401_000, 10, vec![detector, consensus]).unwrap()
    }

    fn valid_diagnosis(reference: &str) -> String {
        json!({
            "schema_version": 1,
            "evidence_schema_version": 1,
            "observed_height": 10,
            "summary": "Progress is stalled.",
            "root_cause": "Consensus view is not advancing.",
            "confidence_bps": 8000,
            "evidence_refs": ["detector-1", "consensus-1"],
            "actions": [{
                "action": "alert_operator",
                "rationale": "Escalate the deterministic finding.",
                "evidence_refs": [reference]
            }]
        })
        .to_string()
    }

    fn limits() -> RuntimeLimits {
        RuntimeLimits {
            request_timeout_ms: 50,
            max_concurrency: 1,
            request_budget: 10,
            token_budget: 100_000,
            max_retries: 0,
            retry_backoff_ms: 1,
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 2,
                open_for_ms: 60_000,
            },
        }
    }

    fn agent<P: ModelProvider>(provider: P) -> DiagnosticAgent<P, MemoryAuditLog> {
        DiagnosticAgent::new(
            StallDetector::new(StallPolicy::default()).unwrap(),
            provider,
            MemoryAuditLog::default(),
            DiagnosisValidator::new(2),
            512,
        )
    }

    #[tokio::test]
    async fn timeout_does_not_change_deterministic_detection() {
        let mut mock = MockProvider::response(valid_diagnosis("detector-1"));
        mock.delay = Duration::from_millis(100);
        let mut timeout_limits = limits();
        timeout_limits.request_timeout_ms = 1;
        let governed = GovernedProvider::new(mock, timeout_limits).unwrap();
        let report = agent(governed)
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert_eq!(report.detection.stall_level(), Some(StallLevel::Level3));
        assert!(matches!(
            report.model_failure,
            Some(ModelFailure::Provider(ProviderError::Timeout))
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_action_and_extra_output_fields() {
        let invalid = json!({
            "schema_version": 1,
            "evidence_schema_version": 1,
            "observed_height": 10,
            "summary": "stalled",
            "root_cause": "unknown",
            "confidence_bps": 5000,
            "evidence_refs": ["detector-1"],
            "actions": [{"action": "run_shell", "rationale": "unsafe", "evidence_refs": ["detector-1"]}],
            "tool": "shell"
        })
        .to_string();
        let report = agent(MockProvider::response(invalid))
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            report.model_failure,
            Some(ModelFailure::Rejected(_))
        ));
    }

    #[tokio::test]
    async fn enforces_request_budget() {
        let mut budget_limits = limits();
        budget_limits.request_budget = 1;
        let governed = GovernedProvider::new(
            MockProvider::response(valid_diagnosis("detector-1")),
            budget_limits,
        )
        .unwrap();
        let diagnostic_agent = agent(governed);
        assert!(
            diagnostic_agent
                .analyze(&observation(), &evidence())
                .await
                .unwrap()
                .diagnosis
                .is_some()
        );
        let second = diagnostic_agent
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            second.model_failure,
            Some(ModelFailure::Provider(ProviderError::BudgetExhausted))
        ));
    }

    #[tokio::test]
    async fn enforces_token_budget_before_calling_model() {
        let mut token_limits = limits();
        token_limits.token_budget = 1;
        let governed = GovernedProvider::new(
            MockProvider::response(valid_diagnosis("detector-1")),
            token_limits,
        )
        .unwrap();
        let report = agent(governed)
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            report.model_failure,
            Some(ModelFailure::Provider(ProviderError::BudgetExhausted))
        ));
    }

    #[tokio::test]
    async fn opens_circuit_after_failure_threshold() {
        let mut circuit_limits = limits();
        circuit_limits.circuit_breaker.failure_threshold = 1;
        let governed = GovernedProvider::new(
            MockProvider::failure(ProviderError::Transport("offline".to_owned())),
            circuit_limits,
        )
        .unwrap();
        let diagnostic_agent = agent(governed);
        let first = diagnostic_agent
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            first.model_failure,
            Some(ModelFailure::Provider(ProviderError::Transport(_)))
        ));
        let second = diagnostic_agent
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            second.model_failure,
            Some(ModelFailure::Provider(ProviderError::CircuitOpen))
        ));
    }

    #[tokio::test]
    async fn rejects_missing_or_unknown_evidence_reference() {
        let report = agent(MockProvider::response(valid_diagnosis("invented-evidence")))
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            report.model_failure,
            Some(ModelFailure::Rejected(_))
        ));
    }

    #[tokio::test]
    async fn rejects_missing_evidence_and_stale_height() {
        let missing_evidence = json!({
            "schema_version": 1,
            "evidence_schema_version": 1,
            "observed_height": 10,
            "summary": "stalled",
            "root_cause": "unknown",
            "confidence_bps": 5000,
            "evidence_refs": [],
            "actions": []
        })
        .to_string();
        let missing_report = agent(MockProvider::response(missing_evidence))
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            missing_report.model_failure,
            Some(ModelFailure::Rejected(DiagnosisError::MissingEvidence))
        ));

        let mut stale: serde_json::Value =
            serde_json::from_str(&valid_diagnosis("detector-1")).unwrap();
        stale["observed_height"] = json!(1);
        let stale_report = agent(MockProvider::response(stale.to_string()))
            .analyze(&observation(), &evidence())
            .await
            .unwrap();
        assert!(matches!(
            stale_report.model_failure,
            Some(ModelFailure::Rejected(DiagnosisError::StaleHeight { .. }))
        ));
    }

    #[tokio::test]
    async fn provider_failure_never_masks_detection_result() {
        let report = agent(MockProvider::failure(ProviderError::InvalidResponse(
            "model unavailable".to_owned(),
        )))
        .analyze(&observation(), &evidence())
        .await
        .unwrap();
        assert_eq!(report.detection.stall_level(), Some(StallLevel::Level3));
        assert!(report.diagnosis.is_none());
    }
}
