//! Capability-limited AI-assisted diagnostics for Neo N3 operations.
//!
//! Detection and policy validation are deterministic. Models receive only a
//! versioned, redacted evidence bundle and can return recommendations from a
//! fixed action catalog. This crate exposes no signing, wallet, shell, file, or
//! model-selected network tooling.

mod agent;
mod audit;
mod diagnosis;
mod durable_actions;
mod evidence;
mod operations;
mod probe;
mod provider;
mod stall;

pub use agent::{AgentError, DiagnosticAgent, DiagnosticReport, ModelFailure};
pub use audit::{
    AuditError, AuditEvent, AuditEventKind, AuditInput, AuditLog, FileAuditLog, MemoryAuditLog,
    verify_audit_chain,
};
pub use diagnosis::{
    ACTION_CATALOG, ActionId, ActionPolicy, ActionRecommendation, Diagnosis, DiagnosisError,
    DiagnosisValidator,
};
pub use durable_actions::{
    ACTION_LOG_SCHEMA_VERSION, ActionAdmissionOutcome, ActionCatalogEntry,
    ActionResourceReservation, ActionTransition, DurableActionBinding, DurableActionError,
    DurableActionLog, DurableActionSnapshot, DurableActionState, OPERATIONAL_ACTION_CATALOG,
    ReconciliationDecision,
};
pub use evidence::{
    EVIDENCE_SCHEMA_VERSION, EvidenceAuthority, EvidenceBundle, EvidenceError, EvidenceItem,
    EvidenceKind, EvidenceRecord, EvidenceSource,
};
pub use operations::{
    ActionEvidence, EvidenceExportError, EvidenceExportStats, EvidenceExporter, EvidenceReceiver,
    ExportResult, OperationalAction, OperationalActionRequest, OperationalDecision,
    OperationalPolicy, OperationalPolicyConfig, OperationalSafetyState,
};
pub use probe::{
    AuthorizerMetadata, FailClosedProbe, MAX_PROBE_DURATION_MS, MAX_PROBE_OBSERVATIONS,
    MAX_PROBE_OUTPUT_BYTES, MAX_PROBE_TRANSACTION_BYTES, OperationId, PROBE_ACTION_CATALOG,
    PROBE_DETERMINISTIC_RULE_VERSION, PROBE_SCHEMA_VERSION, ProbeAction, ProbeActionSpec,
    ProbeAdmissionReceiver, ProbeAdmissionResult, ProbeAdmissionSender, ProbeAuthorizerMetadata,
    ProbeBudget, ProbeBuild, ProbeContext, ProbeContractError, ProbeDeterministicRule, ProbeError,
    ProbeJob, ProbeOutcome, ProbeOutcomeStatus, ProbeRequest, ProbeResult, ProbeResultValidator,
    ProbeRuleFinding, ProbeRuleFindingKind, ProbeRunId, ProbeTransaction, RestrictedProbe, RunId,
    TrustedLocalDiagnosticsIdentity, ValidatedProbeResult,
};
pub use provider::{
    AnthropicProvider, CircuitBreakerConfig, CredentialEnv, GovernedProvider, HttpProviderConfig,
    ModelProvider, ModelRequest, ModelResponse, OllamaProvider, OpenAiCompatibleProvider,
    ProviderError, ProviderKind, RuntimeLimits,
};
pub use stall::{
    ConsensusObservation, DetectionResult, StallDetector, StallLevel, StallPolicy,
    StallPolicyError, UnknownReason,
};
