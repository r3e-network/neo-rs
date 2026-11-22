//! Capability-limited diagnostics runtime with reverse-injected host capabilities.

use crate::store_files::{unix_time_ms, valid_hash};
use crate::{
    ActionBinding, ActionState, AdmitOutcome, AnthropicProvider, AuditEventKind, AuditInput,
    AuditLog, AuthorizerMetadata, DiagnosisValidator, DiagnosticAgent, DiagnosticsSection,
    DurableActionStore, EvidenceAuthority, EvidenceBundle, EvidenceExporter, EvidenceItem,
    EvidenceKind, EvidenceReceiver, EvidenceRecord, EvidenceSource, ExportResult, FileAuditLog,
    HttpProviderConfig, MemoryAuditLog, ModelProvider, ModelProviderSection, ModelRequest,
    ModelResponse, OllamaProvider, OpenAiCompatibleProvider, OperationId, OperationalAction,
    OperationalActionRequest, OperationalDecision, OperationalPolicy, OperationalPolicyConfig,
    OperationalSafetyState, PROBE_ACTION_CATALOG, PROBE_DETERMINISTIC_RULE_VERSION, ProbeAction,
    ProbeBudget, ProbeBuild, ProbeContext, ProbeDeterministicRule, ProbeJob, ProbeResult,
    ProbeRuleFindingKind, ProbeTransaction, ProviderError, ProviderKind, RunId, StallDetector,
    StallPolicy,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

pub const TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION: u16 = 1;
pub const TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION: &str =
    "canonical-transaction-execution-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBlockHash {
    pub display: String,
    pub canonical: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerObservation {
    pub connected_peers: u64,
    pub max_peer_height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedTransaction {
    pub bytes: Vec<u8>,
    pub hash_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionObservation {
    pub transaction_hash: String,
    pub block_height: u32,
    pub block_hash: String,
    pub execution_stage: String,
    pub vm_state: String,
    pub exception_category: String,
    pub gas_consumed: i64,
    pub schema_version: u16,
    pub rule_version: &'static str,
    pub observed_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProbeSessionConfig {
    pub replay_timeout_ms: u64,
    pub frame_limit_bytes: usize,
    pub worker_binary_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeCompletion {
    pub operation_id: OperationId,
    pub run_id: RunId,
    pub result: Option<ProbeResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeSubmissionError;

pub trait ProbeControl: Send + Sync {
    fn try_submit(&self, job: ProbeJob) -> Result<(), ProbeSubmissionError>;
}

pub struct ProbeSession {
    pub control: Arc<dyn ProbeControl>,
    pub config: ProbeSessionConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSubmitResult {
    Disabled,
    Enqueued,
    DroppedFull,
    DroppedClosed,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResult {
    Executed,
    Observed,
    Rejected(OperationalDecision),
    Unsupported,
    InvalidTarget,
    Failed,
}

#[derive(Clone)]
pub struct AiAgentRuntime {
    #[doc(hidden)]
    pub inner: Arc<RuntimeInner>,
}

#[doc(hidden)]
pub fn advance_last_progress(last_progress: &AtomicU64, observed_at_unix_ms: u64) {
    last_progress.fetch_max(observed_at_unix_ms, Ordering::AcqRel);
}

async fn probe_completion_recv(
    completions: &mut Option<mpsc::Receiver<ProbeCompletion>>,
) -> Option<ProbeCompletion> {
    match completions.as_mut() {
        Some(receiver) => receiver.recv().await,
        // A permanently pending future keeps this select arm dormant when no
        // restricted probe controller was started.
        None => std::future::pending().await,
    }
}

#[doc(hidden)]
pub async fn receive_worker_event(
    receiver: &mut EvidenceReceiver,
    execution_receiver: &mut mpsc::Receiver<ExecutionObservation>,
    probe_completions: &mut Option<mpsc::Receiver<ProbeCompletion>>,
    restart: &Notify,
    prefer_evidence: bool,
) -> WorkerEvent {
    if prefer_evidence {
        tokio::select! {
            biased;
            () = restart.notified() => WorkerEvent::Restart,
            probe = probe_completion_recv(probe_completions) => match probe {
                Some(result) => WorkerEvent::Input(WorkerInput::ProbeCompletion(result)),
                None => WorkerEvent::ProbeCompletionsClosed,
            },
            record = receiver.recv() => record
                .map(WorkerInput::Evidence)
                .map_or(WorkerEvent::Closed, WorkerEvent::Input),
            observation = execution_receiver.recv() => observation
                .map(WorkerInput::Execution)
                .map_or(WorkerEvent::Closed, WorkerEvent::Input),
        }
    } else {
        tokio::select! {
            biased;
            () = restart.notified() => WorkerEvent::Restart,
            probe = probe_completion_recv(probe_completions) => match probe {
                Some(result) => WorkerEvent::Input(WorkerInput::ProbeCompletion(result)),
                None => WorkerEvent::ProbeCompletionsClosed,
            },
            observation = execution_receiver.recv() => observation
                .map(WorkerInput::Execution)
                .map_or(WorkerEvent::Closed, WorkerEvent::Input),
            record = receiver.recv() => record
                .map(WorkerInput::Evidence)
                .map_or(WorkerEvent::Closed, WorkerEvent::Input),
        }
    }
}

#[doc(hidden)]
pub fn action_request(
    action: OperationalAction,
    target: Option<String>,
    record: &EvidenceRecord,
) -> OperationalActionRequest {
    OperationalActionRequest {
        action,
        target,
        evidence: vec![crate::ActionEvidence {
            id: record.id.clone(),
            content_hash: record.content_hash.clone(),
            authority: record.authority,
        }],
        model_recommended: false,
    }
}

fn consensus_observation(item: &EvidenceItem, now: u64) -> Option<crate::ConsensusObservation> {
    let fields = item.fields();
    let local_height = fields
        .get("local_height")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())?;
    if local_height != item.observed_height {
        return None;
    }
    Some(crate::ConsensusObservation {
        local_height,
        network_height: fields
            .get("network_height")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        max_peer_height: fields
            .get("max_peer_height")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        last_progress_unix_ms: fields
            .get("last_progress_unix_ms")
            .and_then(Value::as_u64)?,
        observed_at_unix_ms: now,
        view_number: fields
            .get("view")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        connected_peers: fields
            .get("connected_peers")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        active_validators: fields
            .get("active_validators")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())?,
        expected_validators: fields
            .get("expected_validators")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())?,
    })
}

fn decision_code(decision: OperationalDecision) -> &'static str {
    match decision {
        OperationalDecision::Execute => "execute",
        OperationalDecision::ObserveOnly => "observe_only",
        OperationalDecision::Disabled => "disabled",
        OperationalDecision::Killed => "killed",
        OperationalDecision::MissingDeterministicEvidence => "missing_deterministic_evidence",
        OperationalDecision::InvalidEvidence => "invalid_evidence",
        OperationalDecision::StaleEvidence => "stale_evidence",
        OperationalDecision::EvidenceTargetMismatch => "evidence_target_mismatch",
        OperationalDecision::EvidenceRuleMismatch => "evidence_rule_mismatch",
        OperationalDecision::MissingTarget => "missing_target",
        OperationalDecision::TransactionQuarantineDisabled => "transaction_quarantine_disabled",
        OperationalDecision::MissingProbeRuleFinding => "missing_probe_rule_finding",
        OperationalDecision::ModelInfluencedQuarantine => "model_influenced_quarantine",
        OperationalDecision::ConsensusRestartDisabled => "consensus_restart_disabled",
        OperationalDecision::UnpersistedCommit => "unpersisted_commit",
    }
}

mod actions;
mod evidence;
mod hosts;
mod probe;
mod validation;
mod worker;
mod workers;

pub use evidence::{EvidenceRegistry, QuarantineConfig, QuarantineMetadata};
pub use hosts::{
    ChainHost, ExecutionObservationHost, MempoolHost, ProbeHost, RuntimeConfig, RuntimeHost,
};
pub use worker::{
    NodeModelProvider, RuntimeInner, WorkerEvent, WorkerInput, WorkerState, build_model_provider,
};
