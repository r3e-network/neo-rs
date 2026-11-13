//! Capability-limited node-side diagnostics runtime.

use crate::action_store::{ActionBinding, ActionState, AdmitOutcome, DurableActionStore};
use crate::config::DiagnosticsSection;
use crate::probe_containment::NativeProbeContainment;
use crate::probe_controller::{
    ProbeContextRechecker, ProbeController, ProbeControllerConfig, ProbeControllerResult,
    ProbeVerdict, probe_context_matches, validate_probe_startup,
};
use async_trait::async_trait;
use neo_ai_agent::{
    AnthropicProvider, AuditEventKind, AuditInput, AuditLog, AuthorizerMetadata,
    DiagnosisValidator, DiagnosticAgent, EvidenceAuthority, EvidenceBundle, EvidenceExporter,
    EvidenceItem, EvidenceKind, EvidenceReceiver, EvidenceRecord, EvidenceSource, ExportResult,
    FileAuditLog, HttpProviderConfig, MemoryAuditLog, ModelProvider, ModelRequest, ModelResponse,
    OllamaProvider, OpenAiCompatibleProvider, OperationId, OperationalAction,
    OperationalActionRequest, OperationalDecision, OperationalPolicy, OperationalPolicyConfig,
    OperationalSafetyState, PROBE_ACTION_CATALOG, PROBE_DETERMINISTIC_RULE_VERSION, ProbeAction,
    ProbeBudget, ProbeBuild, ProbeContext, ProbeDeterministicRule, ProbeJob, ProbeResult,
    ProbeRuleFindingKind, ProbeTransaction, ProviderError, ProviderKind, RunId, StallDetector,
    StallPolicy,
};
use neo_io::BinaryWriter;
use neo_primitives::UInt256;
use neo_probe::{ProbeRequestFrame, SnapshotInput};
use neo_system::mempool::{QuarantineConfig, QuarantineMetadata};
use neo_system::{
    NeoSystem, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionObservation,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceSubmitResult {
    Disabled,
    Enqueued,
    DroppedFull,
    DroppedClosed,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionResult {
    Executed,
    Observed,
    Rejected(OperationalDecision),
    Unsupported,
    InvalidTarget,
    Failed,
}

#[derive(Debug, Clone)]
struct RuntimeConfig {
    enabled: bool,
    evidence_queue_capacity: usize,
    event_freshness_ms: u64,
    quarantine_ttl_ms: u64,
    rule_version: String,
    policy: OperationalPolicyConfig,
}

impl From<&DiagnosticsSection> for RuntimeConfig {
    fn from(value: &DiagnosticsSection) -> Self {
        Self {
            enabled: value.enabled,
            evidence_queue_capacity: value.evidence_queue_capacity,
            event_freshness_ms: value.event_freshness_ms,
            quarantine_ttl_ms: value.quarantine_ttl_ms,
            rule_version: value.rule_version.clone(),
            policy: OperationalPolicyConfig {
                enabled: value.enabled,
                kill_switch: value.kill_switch,
                observe_only: value.observe_only,
                allow_transaction_quarantine: value.allow_transaction_quarantine,
                allow_consensus_restart: false,
            },
        }
    }
}

#[derive(Default)]
struct WorkerState {
    task: Option<JoinHandle<()>>,
    sampler: Option<JoinHandle<()>>,
}

struct EvidenceRegistry {
    capacity: usize,
    order: VecDeque<String>,
    records: HashMap<String, EvidenceRecord>,
}

impl EvidenceRegistry {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::with_capacity(capacity.max(1)),
            records: HashMap::with_capacity(capacity.max(1)),
        }
    }

    fn insert(&mut self, record: EvidenceRecord) {
        if !self.records.contains_key(&record.id) {
            while self.records.len() >= self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.records.remove(&oldest);
                }
            }
            self.order.push_back(record.id.clone());
        }
        self.records.insert(record.id.clone(), record);
    }

    fn get(&self, id: &str) -> Option<&EvidenceRecord> {
        self.records.get(id)
    }
}

struct ExecutionRepetition {
    signature: String,
    evidence_ids: Vec<String>,
    candidate_emitted: bool,
}

/// Accumulated deterministic transaction-validation failure evidence for one
/// transaction under the current rule version. The bounded local mempool
/// quarantine escalates only after repeated deterministic failures or
/// cross-height reproduction (plan §43), never from a single failure.
struct ValidationFailure {
    rule_version: String,
    deterministic_failures: u32,
    heights: BTreeSet<u32>,
}

/// Deterministic failures (same transaction, same rule version) required
/// before the bounded local mempool quarantine may escalate.
const QUARANTINE_EVIDENCE_FAILURES: u32 = 2;

struct RuntimeInner {
    config: RuntimeConfig,
    system: Arc<NeoSystem>,
    evidence_ingress: EvidenceExporter,
    execution_ingress: mpsc::Sender<TransactionExecutionObservation>,
    probe_controller: Option<ProbeController>,
    probe_config: Mutex<Option<ProbeControllerConfig>>,
    worker: Mutex<WorkerState>,
    worker_generation: AtomicU64,
    worker_restart: Arc<Notify>,
    audit: Arc<dyn AuditLog>,
    action_store: Option<Arc<DurableActionStore>>,
    registry: Arc<Mutex<EvidenceRegistry>>,
    execution_repetitions: Mutex<BTreeMap<String, ExecutionRepetition>>,
    validation_failures: Mutex<BTreeMap<String, ValidationFailure>>,
    evidence_sequence: AtomicU64,
    probe_candidates: AtomicU64,
    probe_submission_attempts: AtomicU64,
    probe_submission_failures: AtomicU64,
    probe_submission_skips: AtomicU64,
    audit_append_failures: AtomicU64,
    #[cfg(test)]
    probe_submission_log: Mutex<Vec<ProbeJob>>,
    received_events: Arc<AtomicU64>,
    analyzed_events: Arc<AtomicU64>,
    sampling_ticks: AtomicU64,
    last_progress_unix_ms: Arc<AtomicU64>,
    last_observed_local_height: AtomicU64,
    peer_observation_available: AtomicBool,
    observed_peer_count: AtomicU64,
    observed_peer_height: AtomicU64,
    peer_observed_at_unix_ms: AtomicU64,
    sampling_interval_ms: AtomicU64,
    sampling_changed: Notify,
}

#[derive(Clone)]
pub(crate) struct AiAgentRuntime {
    inner: Arc<RuntimeInner>,
}

enum NodeModelProvider {
    Unavailable,
    OpenAiCompatible(OpenAiCompatibleProvider),
    Anthropic(AnthropicProvider),
    Ollama(OllamaProvider),
}

#[async_trait]
impl ModelProvider for NodeModelProvider {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ProviderError> {
        match self {
            Self::Unavailable => Err(ProviderError::InvalidConfig(
                "no diagnostics model provider is configured".to_owned(),
            )),
            Self::OpenAiCompatible(provider) => provider.complete(request).await,
            Self::Anthropic(provider) => provider.complete(request).await,
            Self::Ollama(provider) => provider.complete(request).await,
        }
    }
}

#[derive(Clone)]
struct SharedModelProvider(Arc<NodeModelProvider>);

#[async_trait]
impl ModelProvider for SharedModelProvider {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ProviderError> {
        self.0.complete(request).await
    }
}

fn build_model_provider(
    section: Option<&crate::config::ModelProviderSection>,
) -> Result<NodeModelProvider, ProviderError> {
    let Some(section) = section else {
        return Ok(NodeModelProvider::Unavailable);
    };
    let config = HttpProviderConfig {
        endpoint: section.endpoint.clone(),
        model: section.model.clone(),
        credential_env: section.credential_env.clone(),
        limits: section.limits,
    };
    match section.kind {
        ProviderKind::OpenAiCompatible => {
            OpenAiCompatibleProvider::new(config).map(NodeModelProvider::OpenAiCompatible)
        }
        ProviderKind::Anthropic => AnthropicProvider::new(config).map(NodeModelProvider::Anthropic),
        ProviderKind::Ollama => OllamaProvider::new(config).map(NodeModelProvider::Ollama),
    }
}

#[derive(Debug)]
enum WorkerInput {
    Evidence(EvidenceRecord),
    Execution(TransactionExecutionObservation),
    ProbeCompletion(ProbeControllerResult),
}

#[derive(Debug)]
enum WorkerEvent {
    Restart,
    Input(WorkerInput),
    ProbeCompletionsClosed,
    Closed,
}

fn advance_last_progress(last_progress: &AtomicU64, observed_at_unix_ms: u64) {
    last_progress.fetch_max(observed_at_unix_ms, Ordering::AcqRel);
}

async fn probe_completion_recv(
    completions: &mut Option<mpsc::Receiver<ProbeControllerResult>>,
) -> Option<ProbeControllerResult> {
    match completions.as_mut() {
        Some(receiver) => receiver.recv().await,
        // A permanently pending future keeps this select arm dormant when no
        // restricted probe controller was started.
        None => std::future::pending().await,
    }
}

async fn receive_worker_event(
    receiver: &mut EvidenceReceiver,
    execution_receiver: &mut mpsc::Receiver<TransactionExecutionObservation>,
    probe_completions: &mut Option<mpsc::Receiver<ProbeControllerResult>>,
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

impl AiAgentRuntime {
    pub(crate) fn new(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
    ) -> anyhow::Result<Self> {
        let config = RuntimeConfig::from(section);
        if config.enabled && config.evidence_queue_capacity == 0 {
            anyhow::bail!("diagnostics evidence queue capacity must be non-zero");
        }
        let audit: Arc<dyn AuditLog> =
            if config.enabled {
                if !section.audit_enabled || section.audit_path.trim().is_empty() {
                    anyhow::bail!("enabled diagnostics require a durable audit path");
                }
                Arc::new(FileAuditLog::open(&section.audit_path).map_err(|error| {
                    anyhow::anyhow!("failed to open diagnostics audit: {error}")
                })?)
            } else {
                Arc::new(MemoryAuditLog::default())
            };
        let action_store = if config.enabled {
            let path = format!("{}.actions", section.audit_path);
            Some(Arc::new(DurableActionStore::open(path).map_err(
                |error| anyhow::anyhow!("failed to open diagnostics action store: {error}"),
            )?))
        } else {
            None
        };
        Self::initialize(section, system, audit, action_store)
    }

    fn initialize(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
        action_store: Option<Arc<DurableActionStore>>,
    ) -> anyhow::Result<Self> {
        let config = RuntimeConfig::from(section);
        let registry_capacity = section
            .quarantine_max_entries
            .saturating_mul(4)
            .max(config.evidence_queue_capacity);
        let queue_capacity = config.evidence_queue_capacity.max(1);
        let (evidence_ingress, evidence_receiver) = EvidenceExporter::bounded(queue_capacity)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let (execution_ingress, execution_receiver) = mpsc::channel(queue_capacity);
        // An explicitly requested probe must validate and start, or initialization
        // fails with the exact reason. It is never silently degraded.
        let probe = if config.enabled {
            start_probe_controller(section, &system, action_store.as_ref())?
        } else {
            None
        };
        let model_provider = if config.enabled {
            build_model_provider(section.model_provider.as_ref())
                .map_err(|error| anyhow::anyhow!("invalid diagnostics model provider: {error}"))?
        } else {
            NodeModelProvider::Unavailable
        };
        let model_provider = Arc::new(model_provider);
        let initial_height = u64::from(system.current_block_index());
        let initialized_at = unix_time_ms();
        let runtime = Self {
            inner: Arc::new(RuntimeInner {
                sampling_interval_ms: AtomicU64::new(section.sampling_interval_ms),
                config,
                system,
                evidence_ingress,
                execution_ingress,
                probe_controller: probe.as_ref().map(|(controller, _, _)| controller.clone()),
                probe_config: Mutex::new(probe.as_ref().map(|(_, _, config)| config.clone())),
                worker: Mutex::new(WorkerState::default()),
                worker_generation: AtomicU64::new(0),
                worker_restart: Arc::new(Notify::new()),
                audit,
                action_store,
                registry: Arc::new(Mutex::new(EvidenceRegistry::new(registry_capacity))),
                execution_repetitions: Mutex::new(BTreeMap::new()),
                validation_failures: Mutex::new(BTreeMap::new()),
                evidence_sequence: AtomicU64::new(0),
                probe_candidates: AtomicU64::new(0),
                probe_submission_attempts: AtomicU64::new(0),
                probe_submission_failures: AtomicU64::new(0),
                probe_submission_skips: AtomicU64::new(0),
                audit_append_failures: AtomicU64::new(0),
                #[cfg(test)]
                probe_submission_log: Mutex::new(Vec::new()),
                received_events: Arc::new(AtomicU64::new(0)),
                analyzed_events: Arc::new(AtomicU64::new(0)),
                sampling_ticks: AtomicU64::new(0),
                last_progress_unix_ms: Arc::new(AtomicU64::new(initialized_at)),
                last_observed_local_height: AtomicU64::new(initial_height),
                peer_observation_available: AtomicBool::new(false),
                observed_peer_count: AtomicU64::new(0),
                observed_peer_height: AtomicU64::new(0),
                peer_observed_at_unix_ms: AtomicU64::new(0),
                sampling_changed: Notify::new(),
            }),
        };
        if runtime.inner.config.enabled {
            runtime.install_execution_sink()?;
            runtime.start_worker_supervisor(
                evidence_receiver,
                execution_receiver,
                probe.map(|(_, completions, _)| completions),
                model_provider,
            )?;
            runtime.start_sampler();
        }
        Ok(runtime)
    }

    #[cfg(test)]
    fn with_audit(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> anyhow::Result<Self> {
        Self::initialize(section, system, audit, None)
    }

    #[cfg(test)]
    pub(crate) fn is_enabled(&self) -> bool {
        self.inner.config.enabled
    }

    fn install_execution_sink(&self) -> anyhow::Result<()> {
        self.inner
            .system
            .register_transaction_execution_sink(self.inner.execution_ingress.clone())
            .map_err(|_| anyhow::anyhow!("canonical execution sink is already registered"))
    }

    #[cfg(test)]
    pub(crate) fn try_export_consensus_snapshot(
        &self,
        mut fields: BTreeMap<String, Value>,
    ) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        let local_height = self.inner.system.current_block_index();
        fields.insert("local_height".to_string(), json!(local_height));
        fields.insert(
            "last_progress_unix_ms".to_string(),
            json!(self.inner.last_progress_unix_ms.load(Ordering::Acquire)),
        );
        let peer_observation_available = self
            .inner
            .peer_observation_available
            .load(Ordering::Acquire);
        fields.insert(
            "peer_observation_available".to_string(),
            json!(peer_observation_available),
        );
        if peer_observation_available {
            let connected_peers = self.inner.observed_peer_count.load(Ordering::Acquire);
            let max_peer_height = self.inner.observed_peer_height.load(Ordering::Acquire);
            let network_height = u64::from(local_height).max(max_peer_height);
            fields.insert("connected_peers".to_string(), json!(connected_peers));
            fields.insert("max_peer_height".to_string(), json!(max_peer_height));
            fields.insert("network_height".to_string(), json!(network_height));
            fields.insert(
                "peer_observed_at_unix_ms".to_string(),
                json!(self.inner.peer_observed_at_unix_ms.load(Ordering::Acquire)),
            );
        }
        self.try_export(
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            local_height,
            fields,
        )
    }

    pub(crate) fn configure_mempool(&self, section: &DiagnosticsSection) -> anyhow::Result<()> {
        self.inner
            .system
            .mempool()
            .lock()
            .configure_quarantine(
                QuarantineConfig {
                    automatic_enabled: section.enabled && section.allow_transaction_quarantine,
                    max_entries: section.quarantine_max_entries,
                    max_ttl_ms: section.quarantine_ttl_ms,
                    selection_scan_budget: section.proposal_scan_budget,
                },
                unix_time_ms(),
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub(crate) fn try_export(
        &self,
        kind: EvidenceKind,
        source: EvidenceSource,
        observed_height: u32,
        fields: BTreeMap<String, Value>,
    ) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        let sequence = self.inner.evidence_sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("node:{observed_height}:{sequence}");
        let transaction_hash = fields
            .get("transaction_hash")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let Ok(item) = EvidenceItem::new(id, kind, source, observed_height, fields) else {
            return EvidenceSubmitResult::Invalid;
        };
        let tip_height = self.inner.system.current_block_index();
        let tip_hash = self
            .inner
            .system
            .block_hash_at(tip_height)
            .map(|hash| hash.to_string());
        let Ok(record) = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Observation,
            unix_time_ms(),
            transaction_hash,
            tip_height,
            tip_hash,
            self.inner.config.rule_version.clone(),
        ) else {
            return EvidenceSubmitResult::Invalid;
        };
        self.try_submit_record(record)
    }

    #[cfg(test)]
    fn try_export_execution_observation(
        &self,
        observation: &TransactionExecutionObservation,
    ) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        match self.inner.execution_ingress.try_send(observation.clone()) {
            Ok(()) => EvidenceSubmitResult::Enqueued,
            Err(mpsc::error::TrySendError::Full(_)) => EvidenceSubmitResult::DroppedFull,
            Err(mpsc::error::TrySendError::Closed(_)) => EvidenceSubmitResult::DroppedClosed,
        }
    }

    fn execution_observation_record(
        &self,
        observation: &TransactionExecutionObservation,
    ) -> Option<EvidenceRecord> {
        let sequence = self.inner.evidence_sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "canonical-execution:{}:{sequence}",
            observation.block_height
        );
        let transaction_hash = observation.transaction_hash.to_string();
        let block_hash = observation.block_hash.to_string();
        let fields = BTreeMap::from([
            ("transaction_hash".to_string(), json!(transaction_hash)),
            ("block_height".to_string(), json!(observation.block_height)),
            ("block_hash".to_string(), json!(block_hash)),
            (
                "execution_stage".to_string(),
                json!(observation.execution_stage.as_str()),
            ),
            (
                "vm_state".to_string(),
                json!(format!("{:?}", observation.vm_state).to_ascii_uppercase()),
            ),
            (
                "exception_category".to_string(),
                json!(observation.exception_category.as_str()),
            ),
            ("gas_consumed".to_string(), json!(observation.gas_consumed)),
            (
                "schema_version".to_string(),
                json!(observation.schema_version),
            ),
            (
                "observation_rule_version".to_string(),
                json!(observation.rule_version),
            ),
            (
                "rule_version".to_string(),
                json!(self.inner.config.rule_version),
            ),
            (
                "observed_at_unix_ms".to_string(),
                json!(observation.observed_at_unix_ms),
            ),
            ("default_disposition".to_string(), json!("observe_only")),
            ("attack_classification".to_string(), json!("not_determined")),
        ]);
        let Ok(item) = EvidenceItem::new(
            id,
            EvidenceKind::TransactionExecution,
            EvidenceSource::NodeRuntime,
            observation.block_height,
            fields,
        ) else {
            return None;
        };
        EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Observation,
            observation.observed_at_unix_ms,
            Some(observation.transaction_hash.to_string()),
            observation.block_height,
            Some(observation.block_hash.to_string()),
            self.inner.config.rule_version.clone(),
        )
        .ok()
    }

    pub(crate) fn try_submit_record(&self, record: EvidenceRecord) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        match self.inner.evidence_ingress.try_export(record) {
            ExportResult::Enqueued => EvidenceSubmitResult::Enqueued,
            ExportResult::DroppedFull => EvidenceSubmitResult::DroppedFull,
            ExportResult::DroppedClosed => EvidenceSubmitResult::DroppedClosed,
        }
    }

    fn reproducible_execution_candidate(&self, record: &EvidenceRecord) -> Option<EvidenceRecord> {
        if record.authority != EvidenceAuthority::Observation
            || record.kind != EvidenceKind::TransactionExecution
            || record.source != EvidenceSource::NodeRuntime
        {
            return None;
        }
        let fields = record.item.fields();
        if fields.get("vm_state").and_then(Value::as_str) != Some("FAULT")
            || fields.get("default_disposition").and_then(Value::as_str) != Some("observe_only")
        {
            return None;
        }
        let transaction_hash = record.transaction_hash.clone()?;
        let execution_stage = fields.get("execution_stage")?.as_str()?;
        let exception_category = fields.get("exception_category")?.as_str()?;
        let gas_consumed = fields.get("gas_consumed")?.as_i64()?;
        let schema_version = fields.get("schema_version")?.as_u64()?;
        let observation_rule_version = fields.get("observation_rule_version")?.as_str()?;
        if execution_stage != "application"
            || !matches!(exception_category, "vm_fault" | "caught_vm_exception")
            || gas_consumed < 0
            || schema_version != u64::from(TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION)
            || observation_rule_version != TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION
        {
            return None;
        }
        let signature = format!(
            "{execution_stage}|{exception_category}|{gas_consumed}|{schema_version}|{observation_rule_version}"
        );
        let evidence_ids = {
            let mut repetitions = self.inner.execution_repetitions.lock();
            if !repetitions.contains_key(&transaction_hash)
                && repetitions.len() >= self.inner.config.evidence_queue_capacity.max(1)
                && let Some(first_key) = repetitions.keys().next().cloned()
            {
                repetitions.remove(&first_key);
            }
            let entry = repetitions
                .entry(transaction_hash.clone())
                .or_insert_with(|| ExecutionRepetition {
                    signature: signature.clone(),
                    evidence_ids: Vec::with_capacity(2),
                    candidate_emitted: false,
                });
            if entry.signature != signature {
                entry.signature = signature;
                entry.evidence_ids.clear();
                entry.candidate_emitted = false;
            }
            if !entry.evidence_ids.iter().any(|id| id == &record.id) {
                entry.evidence_ids.push(record.id.clone());
            }
            if entry.evidence_ids.len() < 2 || entry.candidate_emitted {
                return None;
            }
            entry.candidate_emitted = true;
            entry.evidence_ids.clone()
        };
        let finding = EvidenceItem::new(
            format!("{}:execution-reproducible", record.id),
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            record.observed_height,
            BTreeMap::from([
                ("transaction_hash".to_string(), json!(transaction_hash)),
                ("parent_evidence_ids".to_string(), json!(evidence_ids)),
                ("reproducible_count".to_string(), json!(2)),
                (
                    "candidate_action".to_string(),
                    json!("restricted_probe_candidate"),
                ),
                (
                    "rule_version".to_string(),
                    json!(self.inner.config.rule_version),
                ),
            ]),
        )
        .ok()?;
        EvidenceRecord::new_with_context(
            finding,
            EvidenceAuthority::DeterministicRule,
            record.observed_at_unix_ms,
            Some(transaction_hash),
            record.tip_height,
            record.tip_hash.clone(),
            self.inner.config.rule_version.clone(),
        )
        .ok()
    }

    /// Fixed deterministic transaction-validation rule.
    ///
    /// A trusted local observation of a deterministic mempool validation
    /// outcome produces a transaction-bound `DeterministicRule` finding and
    /// drives only the bounded local mempool quarantine (or its release).
    /// This is the mempool's automatic-quarantine contract: the finding is
    /// constructed exclusively by this fixed local detector from
    /// Observation-authority records, and model-authority records terminate
    /// before it in the worker, so they can never reach this effect path.
    async fn handle_transaction_validation(&self, record: &EvidenceRecord) {
        let Some(transaction_hash) = record.transaction_hash.clone() else {
            return;
        };
        let fields = record.item.fields();
        if fields.get("deterministic").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let Some(validation_result) = fields
            .get("validation_result")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        // Plan §43 evidence threshold: a single deterministic failure only
        // records the finding. Quarantine escalates once the same transaction
        // accumulates repeated deterministic failures under the same rule
        // version, or the failure reproduces across block heights. A trusted
        // succeed observation always releases and clears the evidence.
        let (apply, quarantine) = if validation_result == "succeed" {
            self.inner
                .validation_failures
                .lock()
                .remove(&transaction_hash);
            (true, false)
        } else {
            let escalate =
                self.record_validation_failure(&transaction_hash, record.observed_height);
            (escalate, true)
        };
        let finding_fields = BTreeMap::from([
            ("parent_evidence_id".to_string(), json!(record.id)),
            ("transaction_hash".to_string(), json!(transaction_hash)),
            ("validation_result".to_string(), json!(validation_result)),
            (
                "rule_version".to_string(),
                json!(self.inner.config.rule_version),
            ),
            (
                "action".to_string(),
                json!(if validation_result == "succeed" {
                    "release_transaction"
                } else if apply {
                    "quarantine_transaction"
                } else {
                    "record_only"
                }),
            ),
        ]);
        let Ok(item) = EvidenceItem::new(
            format!("{}:transaction-validation", record.id),
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            record.observed_height,
            finding_fields,
        ) else {
            return;
        };
        let Ok(finding) = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::DeterministicRule,
            record.observed_at_unix_ms,
            Some(transaction_hash.clone()),
            record.tip_height,
            record.tip_hash.clone(),
            self.inner.config.rule_version.clone(),
        ) else {
            return;
        };
        self.inner.registry.lock().insert(finding.clone());
        if apply {
            self.apply_transaction_validation_finding(&finding, quarantine, &transaction_hash)
                .await;
        }
    }

    /// Records one deterministic validation failure and reports whether the
    /// §43 quarantine evidence threshold has been reached.
    fn record_validation_failure(&self, transaction_hash: &str, observed_height: u32) -> bool {
        let mut failures = self.inner.validation_failures.lock();
        if !failures.contains_key(transaction_hash)
            && failures.len() >= self.inner.config.evidence_queue_capacity.max(1)
            && let Some(first_key) = failures.keys().next().cloned()
        {
            failures.remove(&first_key);
        }
        let entry = failures
            .entry(transaction_hash.to_owned())
            .or_insert_with(|| ValidationFailure {
                rule_version: self.inner.config.rule_version.clone(),
                deterministic_failures: 0,
                heights: BTreeSet::new(),
            });
        if entry.rule_version != self.inner.config.rule_version {
            // A rule-version change invalidates previously accumulated
            // evidence: the failure counter restarts under the new rules.
            entry
                .rule_version
                .clone_from(&self.inner.config.rule_version);
            entry.deterministic_failures = 0;
            entry.heights.clear();
        }
        entry.deterministic_failures = entry.deterministic_failures.saturating_add(1);
        entry.heights.insert(observed_height);
        entry.deterministic_failures >= QUARANTINE_EVIDENCE_FAILURES
            || entry.heights.len() >= QUARANTINE_EVIDENCE_FAILURES as usize
    }

    /// Applies the fixed transaction-validation finding through the bounded
    /// local mempool quarantine contract and audits the durable binding.
    ///
    /// Mirrors the audited effect protocol of
    /// [`AiAgentRuntime::apply_action`]: an "authorized" audit entry must be
    /// durably appended before any effect is allowed (an append failure
    /// rejects the effect entirely), and the executed effect must be audited
    /// afterwards; if that final append fails, a `reconciliation_required`
    /// audit record is appended and the action result reflects the broken
    /// audit trail instead of a clean outcome. The mempool enforces its own
    /// enablement (automatic quarantine is configured from
    /// `allow_transaction_quarantine`), capacity, impact and lifetime
    /// ceilings, so a failed or disabled effect is audited as a non-executed
    /// evaluation without mutating quarantine state.
    async fn apply_transaction_validation_finding(
        &self,
        finding: &EvidenceRecord,
        quarantine: bool,
        transaction_hash: &str,
    ) -> ActionResult {
        let request = action_request(
            if quarantine {
                OperationalAction::QuarantineTransaction
            } else {
                OperationalAction::ReleaseTransaction
            },
            Some(transaction_hash.to_owned()),
            finding,
        );
        // Authorization must be durably audited before any effect is allowed.
        if self
            .audit_action(&request, std::slice::from_ref(finding), "authorized", false)
            .await
            .is_err()
        {
            return ActionResult::Failed;
        }
        let executed = if quarantine {
            self.quarantine_from_local_finding(finding, transaction_hash)
        } else {
            self.release_from_local_finding(transaction_hash)
        };
        let outcome = if executed { "executed" } else { "failed" };
        if self
            .audit_action(&request, std::slice::from_ref(finding), outcome, executed)
            .await
            .is_err()
        {
            if executed {
                // The effect landed but its executed-audit entry is not
                // durable: record the reconciliation requirement before
                // surfacing the non-clean result.
                self.audit_reconciliation_required(&request, std::slice::from_ref(finding))
                    .await;
            }
            return ActionResult::Failed;
        }
        if executed {
            ActionResult::Executed
        } else {
            ActionResult::Failed
        }
    }

    /// Appends the durable record that an executed local effect lacks its
    /// executed-audit entry and therefore requires reconciliation.
    async fn audit_reconciliation_required(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) {
        let reconciled = self
            .inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: AuditEventKind::OperationalActionEvaluated,
                observed_height: self.inner.system.current_block_index(),
                evidence_refs: records.iter().map(|record| record.id.clone()).collect(),
                evidence_hashes: records
                    .iter()
                    .map(|record| record.content_hash.clone())
                    .collect(),
                action: Some(format!("{:?}", request.action)),
                target_hash: request.target.clone(),
                rule_version: records
                    .first()
                    .map(|record| record.rule_version.clone())
                    .or_else(|| Some(self.inner.config.rule_version.clone())),
                outcome_code: "reconciliation_required".to_owned(),
            })
            .await
            .is_ok();
        if !reconciled {
            // Even the reconciliation record is undurable; the failed action
            // result already surfaces the broken audit trail, and the append
            // failure is counted instead of silently dropped.
            self.inner
                .audit_append_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn quarantine_from_local_finding(&self, finding: &EvidenceRecord, target: &str) -> bool {
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return false;
        };
        let now = unix_time_ms();
        {
            let mempool = self.inner.system.mempool();
            let mut pool = mempool.lock();
            if pool.is_quarantined(&transaction_hash, now) {
                // The transaction is already actively contained: repeated
                // deterministic findings stay idempotent instead of failing
                // with the mempool's AlreadyActive error.
                return true;
            }
        }
        let metadata = QuarantineMetadata {
            transaction_hash,
            evidence_refs: vec![finding.id.clone()],
            context: "deterministic transaction validation".to_string(),
            created_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(self.inner.config.quarantine_ttl_ms),
            rule_version: self.inner.config.rule_version.clone(),
            episode: 0,
            cumulative_active_ms: 0,
        };
        self.inner
            .system
            .mempool()
            .lock()
            .quarantine_transaction(metadata, now)
            .is_ok()
    }

    fn release_from_local_finding(&self, target: &str) -> bool {
        self.resolve_target_hash(target)
            .is_some_and(|transaction_hash| {
                self.inner
                    .system
                    .mempool()
                    .lock()
                    .release_quarantine(&transaction_hash)
            })
    }

    /// Consumes a `restricted_probe_candidate` finding through the controlled
    /// probe submission path (plan §38): a probe job is constructed from the
    /// finding and the startup-validated restricted probe configuration, then
    /// offered to the controller's bounded `try_submit`. Every failure mode
    /// (no controller behind the fail-closed gates, unavailable transaction
    /// bytes, full queue, admission cooldown) is recorded and never panics.
    fn handle_restricted_probe_candidate(&self, candidate: &EvidenceRecord) {
        self.inner.probe_candidates.fetch_add(1, Ordering::Relaxed);
        let config = self.inner.probe_config.lock().clone();
        let Some(config) = config else {
            // The fail-closed startup gates never validated a restricted
            // probe configuration: record the skip and consume the candidate.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        let Some(job) = self.probe_candidate_job(candidate, &config) else {
            // The transaction bytes are unavailable (for example the block is
            // no longer retrievable): record the skip without panicking.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.submit_probe_candidate_job(job);
    }

    /// Builds the controlled probe job for a reproducible-execution finding.
    /// The transaction hash is bound to the finding, the replay context is
    /// the live chain tip, and the budget comes exclusively from the
    /// startup-validated restricted probe configuration.
    fn probe_candidate_job(
        &self,
        candidate: &EvidenceRecord,
        config: &ProbeControllerConfig,
    ) -> Option<ProbeJob> {
        let transaction_hash = candidate.transaction_hash.clone()?;
        let on_chain_hash = UInt256::parse(&transaction_hash).ok()?;
        let transaction_bytes =
            self.unsigned_transaction_bytes(&on_chain_hash, candidate.observed_height)?;
        let height = self.inner.system.current_block_index();
        let tip_hash = self
            .inner
            .system
            .block_hash_at(height)
            .map(|hash| canonical_block_hash(&hash))?;
        let now = unix_time_ms().max(1);
        let fingerprint = Sha256::digest(candidate.id.as_bytes()).iter().fold(
            String::new(),
            |mut output, byte| {
                use std::fmt::Write as _;
                write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
                output
            },
        );
        let operation_id =
            OperationId::new(format!("probe-candidate-{}", &fingerprint[..24])).ok()?;
        let run_id = RunId::new(format!("probe-run-{}", &fingerprint[24..40])).ok()?;
        let spec = PROBE_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == ProbeAction::TransactionPreflight)?;
        // The budget is clamped to the fixed probe contract catalog so the
        // startup-validated configuration can never exceed it.
        let budget = ProbeBudget {
            max_duration_ms: u64::try_from(config.replay_timeout.as_millis())
                .unwrap_or(0)
                .min(spec.maximum_budget.max_duration_ms),
            max_output_bytes: u64::try_from(config.frame_limit_bytes)
                .unwrap_or(0)
                .min(spec.maximum_budget.max_output_bytes),
            max_observations: spec.maximum_budget.max_observations,
        };
        let binary_hash = std::fs::read(&config.executable)
            .ok()
            .map(|bytes| {
                Sha256::digest(&bytes)
                    .iter()
                    .fold(String::new(), |mut output, byte| {
                        use std::fmt::Write as _;
                        write!(output, "{byte:02x}")
                            .expect("formatting hex into a String cannot fail");
                        output
                    })
            })
            .filter(|hash| valid_sha256(hash))?;
        ProbeJob::new(
            operation_id,
            run_id,
            ProbeAction::TransactionPreflight,
            ProbeTransaction {
                transaction_hash: hex::encode(on_chain_hash.to_array()),
                transaction_bytes,
            },
            ProbeContext {
                observed_height: height,
                tip_hash,
                requested_at_unix_ms: now,
                node_instance_id: "neo-node-local-diagnostics".to_owned(),
            },
            ProbeBuild {
                binary_hash,
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            budget,
            AuthorizerMetadata {
                evidence_id: candidate.id.clone(),
                evidence_hash: candidate.content_hash.clone(),
                authority: EvidenceAuthority::DeterministicRule,
                rule_version: candidate.rule_version.clone(),
                authorized_at_unix_ms: now,
            },
        )
        .ok()
    }

    /// Returns the unsigned serialization of the transaction the ledger or
    /// mempool holds for `transaction_hash`; the probe contract hashes
    /// exactly these bytes.
    fn unsigned_transaction_bytes(
        &self,
        transaction_hash: &UInt256,
        observed_height: u32,
    ) -> Option<Vec<u8>> {
        let ledger = self.inner.system.ledger_context();
        let transaction = ledger.get_transaction(transaction_hash).or_else(|| {
            let block_hash = ledger.block_hash_at(observed_height)?;
            ledger
                .get_block(&block_hash)?
                .transactions
                .iter()
                .find(|transaction| {
                    transaction
                        .try_hash()
                        .is_ok_and(|hash| hash == *transaction_hash)
                })
                .cloned()
        })?;
        let mut writer = BinaryWriter::new();
        transaction.serialize_unsigned(&mut writer).ok()?;
        Some(writer.into_bytes())
    }

    /// Offers a prepared probe job to the controller's bounded submission
    /// queue; every bounded rejection is recorded instead of panicking.
    fn submit_probe_candidate_job(&self, job: ProbeJob) {
        self.inner
            .probe_submission_attempts
            .fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        self.inner.probe_submission_log.lock().push(job.clone());
        let Some(controller) = &self.inner.probe_controller else {
            // No controller exists behind the fail-closed gates: the prepared
            // job is recorded as a skip and dropped.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        let snapshot = SnapshotInput::from_job(&job, Vec::new());
        let Ok(frame) = ProbeRequestFrame::new(job, snapshot) else {
            self.inner
                .probe_submission_failures
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        if controller.try_submit(frame).is_err() {
            // A full queue, admission cooldown, or closed controller is an
            // expected bounded outcome: record it and keep the worker alive.
            self.inner
                .probe_submission_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Accepts only the controller's completed, parent-validated double-replay result.
    pub(crate) async fn apply_probe_controller_result(
        &self,
        controller_result: ProbeControllerResult,
    ) -> ActionResult {
        let ProbeControllerResult {
            operation_id,
            run_id,
            verdict,
        } = controller_result;
        let ProbeVerdict::Completed(result) = verdict else {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        };
        let fields = result.record().item.fields();
        if fields.get("operation_id").and_then(Value::as_str) != Some(operation_id.as_str())
            || fields.get("run_id").and_then(Value::as_str) != Some(run_id.as_str())
        {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        // The completion must be backed by the controller's durably completed
        // StartProbeRequest operation before any policy may act on it.
        if let Some(store) = &self.inner.action_store
            && !matches!(
                store.query(&operation_id),
                Ok(Some(record))
                    if record.state == ActionState::Completed
                        && record.binding.action == OperationalAction::StartProbeRequest
            )
        {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        self.apply_probe_result(result).await
    }

    /// Consumes only an opaque parent-validated probe result, applies the fixed rule,
    /// then delegates any optional local quarantine to the ordinary policy/action path.
    async fn apply_probe_result(&self, result: ProbeResult) -> ActionResult {
        let observation = result.record().clone();
        if self.validate_probe_observation(&observation).is_err() {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        self.inner.registry.lock().insert(observation);
        let finding = match ProbeDeterministicRule.evaluate(&result) {
            Ok(Some(finding))
                if finding.finding() == ProbeRuleFindingKind::TransactionIsolationCandidate =>
            {
                finding
            }
            Ok(_) => {
                return ActionResult::Rejected(OperationalDecision::MissingProbeRuleFinding);
            }
            Err(_) => return ActionResult::Rejected(OperationalDecision::InvalidEvidence),
        };
        let (record, _) = finding.into_parts();
        let Some(target) = record.transaction_hash.clone() else {
            return ActionResult::Rejected(OperationalDecision::EvidenceTargetMismatch);
        };
        self.inner.registry.lock().insert(record.clone());
        self.apply_action(action_request(
            OperationalAction::QuarantineTransaction,
            Some(target),
            &record,
        ))
        .await
    }

    fn validate_probe_observation(
        &self,
        record: &EvidenceRecord,
    ) -> Result<(), OperationalDecision> {
        if record.verify_integrity().is_err()
            || record.authority != EvidenceAuthority::Observation
            || record.kind != EvidenceKind::ProbeResult
            || record.source != EvidenceSource::RestrictedProbe
            || record.rule_version != self.inner.config.rule_version
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let fields = record.item.fields();
        if !valid_sha256(
            fields
                .get("context_hash")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ) || fields.get("snapshot_height").and_then(Value::as_u64)
            != Some(u64::from(record.tip_height))
            || fields.get("snapshot_root").and_then(Value::as_str) != record.tip_hash.as_deref()
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let now = unix_time_ms();
        let current_height = self.inner.system.current_block_index();
        if record.observed_at_unix_ms > now
            || now.saturating_sub(record.observed_at_unix_ms) > self.inner.config.event_freshness_ms
            || record.tip_height != current_height
            || record.observed_height != current_height
            || record.tip_hash.as_deref()
                != self
                    .inner
                    .system
                    .block_hash_at(current_height)
                    .as_ref()
                    .map(canonical_block_hash)
                    .as_deref()
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        Ok(())
    }

    pub(crate) async fn apply_action(&self, request: OperationalActionRequest) -> ActionResult {
        if !matches!(
            request.action,
            OperationalAction::IncreaseSampling
                | OperationalAction::QuarantineTransaction
                | OperationalAction::ReleaseTransaction
                | OperationalAction::RestartAiWorker
        ) {
            // The rejection itself must be durably audited: an audit append
            // failure can never be downgraded to a clean Unsupported result.
            if self
                .audit_action(&request, &[], "unsupported", false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Unsupported;
        }

        let records = match self.authorize_evidence(&request) {
            Ok(records) => records,
            Err(decision) => {
                // An unauditable rejection must surface as Failed instead of a
                // clean Rejected outcome.
                if self
                    .audit_action(&request, &[], decision_code(decision), false)
                    .await
                    .is_err()
                {
                    return ActionResult::Failed;
                }
                return ActionResult::Rejected(decision);
            }
        };
        let mut validated_request = request.clone();
        for (reference, record) in validated_request.evidence.iter_mut().zip(&records) {
            reference.authority = record.authority;
        }
        let decision = OperationalPolicy::new(self.inner.config.policy).evaluate(
            &validated_request,
            OperationalSafetyState::default(),
            &records,
        );
        if decision != OperationalDecision::Execute {
            let outcome = decision_code(decision);
            // Policy rejections (including TransactionQuarantineDisabled) must
            // still be durably audited; an audit append failure is a Failed
            // outcome rather than a silently unauditable rejection.
            if self
                .audit_action(&validated_request, &records, outcome, false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return if decision == OperationalDecision::ObserveOnly {
                ActionResult::Observed
            } else {
                ActionResult::Rejected(decision)
            };
        }
        if let Err(decision) = self.revalidate_action_context(&validated_request, &records) {
            if self
                .audit_action(&validated_request, &records, decision_code(decision), false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Rejected(decision);
        }

        let operation_id = if let Some(store) = &self.inner.action_store {
            let Ok(binding) = self.durable_action_binding(&validated_request, &records) else {
                return ActionResult::Failed;
            };
            let operation_id = binding.operation_id.clone();
            match store.admit(binding, unix_time_ms()) {
                Ok(AdmitOutcome::Admitted(_)) => {}
                Ok(AdmitOutcome::Existing(existing)) => match existing.state {
                    ActionState::Completed => return ActionResult::Executed,
                    ActionState::Admitted => {}
                    ActionState::Running
                    | ActionState::Uncertain
                    | ActionState::ReconciliationRequired
                    | ActionState::Rejected => return ActionResult::Failed,
                },
                Err(_) => return ActionResult::Failed,
            }
            Some(operation_id)
        } else {
            None
        };

        // Authorization must be durably audited before any effect is allowed.
        if self
            .audit_action(&validated_request, &records, "authorized", false)
            .await
            .is_err()
        {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                let _ = store.reject(operation_id, "pre_audit_failed", unix_time_ms());
            }
            return ActionResult::Failed;
        }
        if let Err(decision) = self.revalidate_action_context(&validated_request, &records) {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                let _ = store.reject(operation_id, "effect_rejected", unix_time_ms());
            }
            if self
                .audit_action(&validated_request, &records, decision_code(decision), false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Rejected(decision);
        }
        if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id)
            && store.mark_running(operation_id, unix_time_ms()).is_err()
        {
            return ActionResult::Failed;
        }

        let result = match validated_request.action {
            OperationalAction::IncreaseSampling => {
                let current = self.inner.sampling_interval_ms.load(Ordering::Acquire);
                self.inner
                    .sampling_interval_ms
                    .store((current / 2).max(100), Ordering::Release);
                self.inner.sampling_changed.notify_one();
                ActionResult::Executed
            }
            OperationalAction::QuarantineTransaction => self.quarantine(&validated_request),
            OperationalAction::ReleaseTransaction => self.release(&validated_request),
            OperationalAction::RestartAiWorker => {
                self.request_worker_restart();
                ActionResult::Executed
            }
            OperationalAction::StartProbeRequest | OperationalAction::RestartConsensus => {
                ActionResult::Unsupported
            }
        };
        let effect_applied = result == ActionResult::Executed;
        let outcome = if effect_applied { "executed" } else { "failed" };
        if self
            .audit_action(&validated_request, &records, outcome, effect_applied)
            .await
            .is_err()
        {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                if effect_applied {
                    let _ = store.mark_reconciliation_required(operation_id, unix_time_ms());
                } else {
                    let _ = store.reject(operation_id, "effect_rejected", unix_time_ms());
                }
            }
            return ActionResult::Failed;
        }
        if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
            let persisted = if effect_applied {
                store.mark_completed(operation_id, unix_time_ms())
            } else {
                store.reject(operation_id, "effect_rejected", unix_time_ms())
            };
            if persisted.is_err() {
                return ActionResult::Failed;
            }
        }
        result
    }

    fn durable_action_binding(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) -> Result<ActionBinding, ()> {
        let rule_version = records
            .first()
            .map(|record| record.rule_version.as_str())
            .ok_or(())?;
        if records
            .iter()
            .any(|record| record.rule_version != rule_version)
        {
            return Err(());
        }
        let material = json!({
            "action": request.action,
            "target": request.target,
            "evidence": records.iter().map(|record| (&record.id, &record.content_hash)).collect::<Vec<_>>(),
            "rule_version": rule_version,
        });
        let bytes = serde_json::to_vec(&material).map_err(|_| ())?;
        let hash = Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut output, byte| {
                use std::fmt::Write as _;
                write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
                output
            });
        let operation_id = OperationId::new(format!("action-{}", &hash[..40])).map_err(|_| ())?;
        ActionBinding::new(
            operation_id,
            request.action,
            request.target.clone(),
            records.iter().map(|record| record.id.clone()).collect(),
            records
                .iter()
                .map(|record| record.content_hash.clone())
                .collect(),
            rule_version.to_owned(),
        )
        .map_err(|_| ())
    }

    fn revalidate_action_context(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) -> Result<(), OperationalDecision> {
        if request.action != OperationalAction::QuarantineTransaction {
            return Ok(());
        }
        let record = records
            .iter()
            .find(|record| record.kind == EvidenceKind::ProbeRuleFinding)
            .ok_or(OperationalDecision::MissingProbeRuleFinding)?;
        if record.authority != EvidenceAuthority::DeterministicRule
            || record.source != EvidenceSource::DeterministicDetector
            || record.rule_version != PROBE_DETERMINISTIC_RULE_VERSION
        {
            return Err(OperationalDecision::EvidenceRuleMismatch);
        }
        let fields = record.item.fields();
        let context_hash = fields
            .get("context_hash")
            .and_then(Value::as_str)
            .ok_or(OperationalDecision::InvalidEvidence)?;
        let snapshot_height = fields
            .get("snapshot_height")
            .and_then(Value::as_u64)
            .and_then(|height| u32::try_from(height).ok())
            .ok_or(OperationalDecision::InvalidEvidence)?;
        let snapshot_root = fields
            .get("snapshot_root")
            .and_then(Value::as_str)
            .ok_or(OperationalDecision::InvalidEvidence)?;
        if !valid_sha256(context_hash)
            || snapshot_height != record.observed_height
            || snapshot_height != record.tip_height
            || record.tip_hash.as_deref() != Some(snapshot_root)
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let current_height = self.inner.system.current_block_index();
        if current_height != snapshot_height
            || self
                .inner
                .system
                .block_hash_at(current_height)
                .is_none_or(|hash| canonical_block_hash(&hash) != snapshot_root)
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        Ok(())
    }

    fn authorize_evidence(
        &self,
        request: &OperationalActionRequest,
    ) -> Result<Vec<EvidenceRecord>, OperationalDecision> {
        let registry = self.inner.registry.lock();
        let mut records = Vec::with_capacity(request.evidence.len());
        for reference in &request.evidence {
            let record = registry
                .get(&reference.id)
                .ok_or(OperationalDecision::InvalidEvidence)?;
            if record.content_hash != reference.content_hash
                || record.authority != reference.authority
            {
                return Err(OperationalDecision::InvalidEvidence);
            }
            self.validate_record(record, true)?;
            records.push(record.clone());
        }
        if matches!(
            request.action,
            OperationalAction::QuarantineTransaction | OperationalAction::ReleaseTransaction
        ) {
            let target = request
                .target
                .as_deref()
                .filter(|target| !target.is_empty())
                .ok_or(OperationalDecision::MissingTarget)?;
            if !records.iter().any(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.transaction_hash.as_deref() == Some(target)
            }) {
                return Err(OperationalDecision::EvidenceTargetMismatch);
            }
        }
        Ok(records)
    }

    fn validate_record(
        &self,
        record: &EvidenceRecord,
        allow_deterministic: bool,
    ) -> Result<(), OperationalDecision> {
        if record.verify_integrity().is_err()
            || (!allow_deterministic && record.authority == EvidenceAuthority::DeterministicRule)
            || record.kind == EvidenceKind::ProbeResult
            || record.source == EvidenceSource::RestrictedProbe
        {
            // Probe results must arrive through the future opaque parent-validated path,
            // never through this generic evidence ingress.
            return Err(OperationalDecision::InvalidEvidence);
        }
        let expected_rule_version = if record.kind == EvidenceKind::ProbeRuleFinding {
            PROBE_DETERMINISTIC_RULE_VERSION
        } else {
            self.inner.config.rule_version.as_str()
        };
        if record.rule_version != expected_rule_version {
            return Err(OperationalDecision::EvidenceRuleMismatch);
        }
        if record.kind == EvidenceKind::ProbeRuleFinding
            && (record.authority != EvidenceAuthority::DeterministicRule
                || record.source != EvidenceSource::DeterministicDetector)
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let now = unix_time_ms();
        let current_height = self.inner.system.current_block_index();
        if record.observed_at_unix_ms > now
            || now.saturating_sub(record.observed_at_unix_ms) > self.inner.config.event_freshness_ms
            || record.tip_height > current_height
            || record.observed_height > record.tip_height.saturating_add(1)
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        if let Some(expected_tip_hash) = record.tip_hash.as_deref()
            && self
                .inner
                .system
                .block_hash_at(record.tip_height)
                .is_none_or(|hash| {
                    // Generic ingress records carry the `0x`-prefixed display
                    // form, while probe rule findings carry the canonical
                    // unprefixed 64-hex form; both must match the same block.
                    hash.to_string() != expected_tip_hash
                        && canonical_block_hash(&hash) != expected_tip_hash
                })
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        Ok(())
    }

    /// Resolves an action target hash to the mempool transaction hash.
    ///
    /// Targets reach the quarantine chain in two hex conventions: the
    /// conventional display form (`UInt256::to_string`, reversed byte order,
    /// optional `0x` prefix) used by generic ingress evidence, and the
    /// canonical natural-order sha256 hex the probe contract binds to
    /// `transaction_bytes`. `UInt256::parse` only understands the display
    /// form, so the natural-order reading is tried as well and wins only
    /// when it names a transaction that is verified in the mempool or
    /// already quarantined; otherwise the display reading is authoritative.
    fn resolve_target_hash(&self, target: &str) -> Option<UInt256> {
        let display = UInt256::parse(target).ok()?;
        let unprefixed = target
            .strip_prefix("0x")
            .or_else(|| target.strip_prefix("0X"))
            .unwrap_or(target);
        let natural = hex::decode(unprefixed)
            .ok()
            .and_then(|bytes| UInt256::from_bytes(&bytes).ok())
            .filter(|hash| *hash != display);
        if let Some(natural) = natural {
            let now = unix_time_ms();
            let mempool = self.inner.system.mempool();
            let mut pool = mempool.lock();
            if pool.contains_key(&natural) || pool.is_quarantined(&natural, now) {
                return Some(natural);
            }
        }
        Some(display)
    }

    fn quarantine(&self, request: &OperationalActionRequest) -> ActionResult {
        let Some(target) = request.target.as_deref() else {
            return ActionResult::InvalidTarget;
        };
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return ActionResult::InvalidTarget;
        };
        if self.is_quarantined_target(request) {
            // The transaction is already actively contained: repeated trusted
            // probe verdicts stay idempotent instead of failing with the
            // mempool's AlreadyActive error.
            return ActionResult::Executed;
        }
        let now = unix_time_ms();
        let metadata = QuarantineMetadata {
            transaction_hash,
            evidence_refs: request
                .evidence
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            context: "trusted double replay fixed probe rule".to_string(),
            created_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(self.inner.config.quarantine_ttl_ms),
            rule_version: PROBE_DETERMINISTIC_RULE_VERSION.to_string(),
            episode: 0,
            cumulative_active_ms: 0,
        };
        self.inner
            .system
            .mempool()
            .lock()
            .quarantine_transaction(metadata, now)
            .map_or(ActionResult::Failed, |()| ActionResult::Executed)
    }

    fn release(&self, request: &OperationalActionRequest) -> ActionResult {
        let Some(target) = request.target.as_deref() else {
            return ActionResult::InvalidTarget;
        };
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return ActionResult::InvalidTarget;
        };
        self.inner
            .system
            .mempool()
            .lock()
            .release_quarantine(&transaction_hash);
        ActionResult::Executed
    }

    fn request_worker_restart(&self) {
        if self.inner.config.enabled {
            self.inner.worker_generation.fetch_add(1, Ordering::AcqRel);
            self.inner.worker_restart.notify_one();
        }
    }

    fn start_worker_supervisor(
        &self,
        mut receiver: EvidenceReceiver,
        mut execution_receiver: mpsc::Receiver<TransactionExecutionObservation>,
        mut probe_completions: Option<mpsc::Receiver<ProbeControllerResult>>,
        model_provider: Arc<NodeModelProvider>,
    ) -> anyhow::Result<()> {
        let audit = Arc::clone(&self.inner.audit);
        let restart = Arc::clone(&self.inner.worker_restart);
        let weak: Weak<RuntimeInner> = Arc::downgrade(&self.inner);
        let task = tokio::spawn(async move {
            let Ok(detector) = StallDetector::new(StallPolicy::default()) else {
                return;
            };
            let provider = SharedModelProvider(model_provider);
            let mut agent = DiagnosticAgent::new(
                detector,
                provider.clone(),
                Arc::clone(&audit),
                DiagnosisValidator::new(2),
                512,
            );
            let mut prefer_evidence = true;
            loop {
                let input = match receive_worker_event(
                    &mut receiver,
                    &mut execution_receiver,
                    &mut probe_completions,
                    &restart,
                    prefer_evidence,
                )
                .await
                {
                    WorkerEvent::Restart => {
                        let Ok(detector) = StallDetector::new(StallPolicy::default()) else {
                            return;
                        };
                        agent = DiagnosticAgent::new(
                            detector,
                            provider.clone(),
                            Arc::clone(&audit),
                            DiagnosisValidator::new(2),
                            512,
                        );
                        continue;
                    }
                    WorkerEvent::ProbeCompletionsClosed => {
                        // The controller is gone; retire this arm instead of
                        // busy-polling a closed receiver.
                        probe_completions = None;
                        continue;
                    }
                    WorkerEvent::Input(input) => input,
                    WorkerEvent::Closed => return,
                };
                prefer_evidence = matches!(&input, WorkerInput::Execution(_));
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let runtime = AiAgentRuntime { inner };
                let record = match input {
                    WorkerInput::Evidence(record) => record,
                    WorkerInput::ProbeCompletion(controller_result) => {
                        // Real production consumption path: the controller's
                        // bounded non-blocking completion stream reaches the
                        // trusted result policy here, not only from tests.
                        let _ = runtime
                            .apply_probe_controller_result(controller_result)
                            .await;
                        continue;
                    }
                    WorkerInput::Execution(observation) => {
                        let Some(record) = runtime.execution_observation_record(&observation)
                        else {
                            continue;
                        };
                        record
                    }
                };
                runtime
                    .inner
                    .received_events
                    .fetch_add(1, Ordering::Relaxed);
                if runtime.validate_record(&record, false).is_err() {
                    continue;
                }

                let item = record.item.clone();
                let observed_at = record.observed_at_unix_ms;
                runtime.inner.registry.lock().insert(record.clone());

                // Model output remains available as advisory evidence, but it must
                // terminate before any local state update, fixed detector, or action chain.
                if record.authority == EvidenceAuthority::Model {
                    if runtime
                        .audit_detector_rejection(&record, "model_authority_rejected")
                        .await
                        .is_err()
                    {
                        // The rejection itself stands (model evidence never
                        // proceeds), but the undurable audit append is counted
                        // instead of silently dropped.
                        runtime
                            .inner
                            .audit_append_failures
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                }
                if let Some(candidate) = runtime.reproducible_execution_candidate(&record) {
                    runtime.inner.registry.lock().insert(candidate.clone());
                    // Plan §38 closed loop: the candidate finding is consumed
                    // through the controlled probe submission path, not merely
                    // recorded.
                    runtime.handle_restricted_probe_candidate(&candidate);
                }
                // Fixed deterministic transaction-validation detector: only
                // trusted local Observation-authority records reach it (model
                // records terminated above), and it alone constructs the
                // transaction-bound deterministic finding for the bounded
                // local mempool quarantine contract.
                if item.kind == EvidenceKind::TransactionValidation
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::NodeRuntime
                {
                    runtime.handle_transaction_validation(&record).await;
                }
                if item.kind == EvidenceKind::ChainCommit
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::NodeRuntime
                {
                    advance_last_progress(&runtime.inner.last_progress_unix_ms, observed_at);
                }

                if item.kind == EvidenceKind::ConsensusState
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::ConsensusSnapshot
                    && let Some(observation) = consensus_observation(&item, observed_at)
                    && let Ok(bundle) =
                        EvidenceBundle::new(observed_at, item.observed_height, vec![item.clone()])
                {
                    runtime
                        .inner
                        .analyzed_events
                        .fetch_add(1, Ordering::Relaxed);
                    if let Ok(report) = agent.analyze(&observation, &bundle).await
                        && report.detection.stall_level().is_some()
                    {
                        let finding_id = format!("{}:stall-rule", item.id);
                        let fields = BTreeMap::from([
                            ("parent_evidence_id".to_string(), json!(item.id)),
                            (
                                "detection".to_string(),
                                json!(format!("{:?}", report.detection)),
                            ),
                            (
                                "rule_version".to_string(),
                                json!(runtime.inner.config.rule_version),
                            ),
                            ("action".to_string(), json!("increase_sampling")),
                        ]);
                        if let Ok(finding) = EvidenceItem::new(
                            finding_id,
                            EvidenceKind::DetectorFinding,
                            EvidenceSource::DeterministicDetector,
                            item.observed_height,
                            fields,
                        ) && let Ok(finding_record) = EvidenceRecord::new_with_context(
                            finding,
                            EvidenceAuthority::DeterministicRule,
                            observed_at,
                            None,
                            record.tip_height,
                            record.tip_hash.clone(),
                            runtime.inner.config.rule_version.clone(),
                        ) {
                            runtime.inner.registry.lock().insert(finding_record.clone());
                            let request = action_request(
                                OperationalAction::IncreaseSampling,
                                None,
                                &finding_record,
                            );
                            let _ = runtime.apply_action(request).await;
                        }
                    }
                }
            }
        });
        self.inner.worker.lock().task = Some(task);
        Ok(())
    }

    async fn audit_detector_rejection(
        &self,
        record: &EvidenceRecord,
        outcome_code: &str,
    ) -> Result<(), neo_ai_agent::AuditError> {
        self.inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: AuditEventKind::DiagnosisRejected,
                observed_height: record.observed_height,
                evidence_refs: vec![record.id.clone()],
                evidence_hashes: vec![record.content_hash.clone()],
                action: None,
                target_hash: record.transaction_hash.clone(),
                rule_version: Some(self.inner.config.rule_version.clone()),
                outcome_code: outcome_code.to_owned(),
            })
            .await
            .map(|_| ())
    }

    fn is_quarantined_target(&self, request: &OperationalActionRequest) -> bool {
        request
            .target
            .as_deref()
            .and_then(|target| self.resolve_target_hash(target))
            .is_some_and(|hash| {
                self.inner
                    .system
                    .mempool()
                    .lock()
                    .is_quarantined(&hash, unix_time_ms())
            })
    }

    fn start_sampler(&self) {
        let weak: Weak<RuntimeInner> = Arc::downgrade(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let interval = inner.sampling_interval_ms.load(Ordering::Acquire).max(1);
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(interval)) => {
                        inner.sampling_ticks.fetch_add(1, Ordering::Relaxed);
                        let runtime = AiAgentRuntime { inner: Arc::clone(&inner) };
                        let observed_at = unix_time_ms();
                        let height = inner.system.current_block_index();
                        let previous_height = inner
                            .last_observed_local_height
                            .swap(u64::from(height), Ordering::AcqRel);
                        if u64::from(height) > previous_height {
                            advance_last_progress(&inner.last_progress_unix_ms, observed_at);
                        }
                        if let Ok(peers) = inner.system.remote_node_snapshots().await {
                            let connected_peers = u64::try_from(peers.len()).unwrap_or(u64::MAX);
                            let max_peer_height = peers
                                .iter()
                                .map(|peer| u64::from(peer.last_block_index))
                                .max()
                                .unwrap_or(0);
                            inner
                                .observed_peer_count
                                .store(connected_peers, Ordering::Release);
                            inner
                                .observed_peer_height
                                .store(max_peer_height, Ordering::Release);
                            inner
                                .peer_observed_at_unix_ms
                                .store(observed_at, Ordering::Release);
                            inner
                                .peer_observation_available
                                .store(true, Ordering::Release);
                        }
                        let mut fields = BTreeMap::from([
                            ("local_height".to_string(), json!(height)),
                            ("sample_interval_ms".to_string(), json!(interval)),
                            (
                                "last_progress_unix_ms".to_string(),
                                json!(inner.last_progress_unix_ms.load(Ordering::Acquire)),
                            ),
                        ]);
                        if inner.peer_observation_available.load(Ordering::Acquire) {
                            fields.insert(
                                "connected_peers".to_string(),
                                json!(inner.observed_peer_count.load(Ordering::Acquire)),
                            );
                            fields.insert(
                                "max_peer_height".to_string(),
                                json!(inner.observed_peer_height.load(Ordering::Acquire)),
                            );
                            fields.insert(
                                "peer_observed_at_unix_ms".to_string(),
                                json!(inner.peer_observed_at_unix_ms.load(Ordering::Acquire)),
                            );
                        }
                        let _ = runtime.try_export(
                            EvidenceKind::ChainProgress,
                            EvidenceSource::NodeMetrics,
                            height,
                            fields,
                        );
                    }
                    () = inner.sampling_changed.notified() => {}
                }
            }
        });
        self.inner.worker.lock().sampler = Some(task);
    }

    async fn audit_action(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
        outcome: &str,
        executed: bool,
    ) -> Result<(), neo_ai_agent::AuditError> {
        self.inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: if executed {
                    AuditEventKind::OperationalActionExecuted
                } else {
                    AuditEventKind::OperationalActionEvaluated
                },
                observed_height: self.inner.system.current_block_index(),
                evidence_refs: records.iter().map(|record| record.id.clone()).collect(),
                evidence_hashes: records
                    .iter()
                    .map(|record| record.content_hash.clone())
                    .collect(),
                action: Some(format!("{:?}", request.action)),
                target_hash: request.target.clone(),
                rule_version: records
                    .first()
                    .map(|record| record.rule_version.clone())
                    .or_else(|| Some(self.inner.config.rule_version.clone())),
                outcome_code: outcome.to_string(),
            })
            .await
            .map(|_| ())
    }

    #[cfg(test)]
    async fn audit_snapshot(&self) -> Vec<neo_ai_agent::AuditEvent> {
        self.inner.audit.snapshot().await.unwrap_or_default()
    }

    #[cfg(test)]
    fn transaction_finding(&self, hash: &str) -> Option<EvidenceRecord> {
        self.inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.transaction_hash.as_deref() == Some(hash)
            })
            .cloned()
    }

    #[cfg(test)]
    fn execution_observation(&self, hash: &str) -> Option<EvidenceRecord> {
        self.inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| {
                record.authority == EvidenceAuthority::Observation
                    && record.kind == EvidenceKind::TransactionExecution
                    && record.transaction_hash.as_deref() == Some(hash)
            })
            .cloned()
    }
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        // Shut the restricted probe controller down deterministically before
        // the worker tasks are aborted: dropping the last controller handle
        // cancels its shutdown token and stops launching further probes.
        drop(self.probe_controller.take());
        let worker = self.worker.get_mut();
        if let Some(task) = worker.task.take() {
            task.abort();
        }
        if let Some(task) = worker.sampler.take() {
            task.abort();
        }
    }
}

/// Starts the restricted probe controller behind its fail-closed gates.
///
/// When explicitly requested, invalid containment, worker configuration or
/// controller initialization is a startup error rather than a silent bypass.
fn start_probe_controller(
    section: &DiagnosticsSection,
    system: &Arc<NeoSystem>,
    action_store: Option<&Arc<DurableActionStore>>,
) -> anyhow::Result<
    Option<(
        ProbeController,
        mpsc::Receiver<ProbeControllerResult>,
        ProbeControllerConfig,
    )>,
> {
    if !section.restricted_probe_enabled {
        return Ok(None);
    }
    let action_store = action_store
        .ok_or_else(|| anyhow::anyhow!("restricted probe requires the durable action store"))?;
    let config = validate_probe_startup(section, &NativeProbeContainment)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let rechecker = SystemProbeContextRechecker {
        system: Arc::clone(system),
    };
    let controller = ProbeController::start(
        config.clone(),
        Arc::clone(action_store),
        Arc::new(rechecker),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let completions = controller.take_completions().ok_or_else(|| {
        anyhow::anyhow!("restricted probe completion stream was already consumed")
    })?;
    Ok(Some((controller, completions, config)))
}

/// Rechecks a probe job context against the live chain tip in canonical hex.
struct SystemProbeContextRechecker {
    system: Arc<NeoSystem>,
}

impl ProbeContextRechecker for SystemProbeContextRechecker {
    fn is_current(&self, job: &ProbeJob) -> bool {
        let height = self.system.current_block_index();
        let Some(tip_hash) = self.system.block_hash_at(height) else {
            return false;
        };
        probe_context_matches(job, height, &canonical_block_hash(&tip_hash))
    }
}

fn action_request(
    action: OperationalAction,
    target: Option<String>,
    record: &EvidenceRecord,
) -> OperationalActionRequest {
    OperationalActionRequest {
        action,
        target,
        evidence: vec![neo_ai_agent::ActionEvidence {
            id: record.id.clone(),
            content_hash: record.content_hash.clone(),
            authority: record.authority,
        }],
        model_recommended: false,
    }
}

fn consensus_observation(
    item: &EvidenceItem,
    now: u64,
) -> Option<neo_ai_agent::ConsensusObservation> {
    let fields = item.fields();
    let local_height = fields
        .get("local_height")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())?;
    if local_height != item.observed_height {
        return None;
    }
    Some(neo_ai_agent::ConsensusObservation {
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

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Canonical unprefixed 64-character lowercase hex form of a block hash.
///
/// `UInt256::to_string` renders an `0x`-prefixed display form, while the probe
/// contracts require the strict unprefixed 64-hex representation.
fn canonical_block_hash(hash: &UInt256) -> String {
    hash.as_ref()
        .iter()
        .rev()
        .fold(String::new(), |mut output, byte| {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
            output
        })
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_ai_agent::{
        ActionEvidence, AuditError, AuditEvent, AuthorizerMetadata, ProbeAction, ProbeBudget,
        ProbeBuild, ProbeContext, ProbeJob, ProbeOutcome, ProbeOutcomeStatus, ProbeResultValidator,
        ProbeTransaction, RunId, TrustedLocalDiagnosticsIdentity,
    };
    use neo_io::BinaryWriter;
    use neo_native_contracts::native::AccountState;
    use neo_native_contracts::native::NativeContract;
    use neo_native_contracts::native::fungible_token::PREFIX_ACCOUNT;
    use neo_native_contracts::native::gas_token::GasToken;
    use neo_primitives::VerifyResult;
    use neo_primitives::{UInt160, WitnessScope};
    use neo_protocol::ProtocolSettings;
    use neo_protocol::get_sign_data_vec;
    use neo_protocol::payloads::Signer;
    use neo_protocol::payloads::{Block, Header, Transaction};
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::{DataCache, ReadOnlyStoreGeneric};
    use neo_storage::{StorageItem, StorageKey};
    use neo_system::state_service::StateServiceSettings;
    use neo_system::{TransactionExecutionExceptionCategory, TransactionExecutionStage};
    use neo_vm::BinarySerializer;
    use neo_vm::{ExecutionEngineLimits, OpCode, VmState};
    use neo_wallets::KeyPair;
    use num_bigint::BigInt;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use tempfile::TempDir;

    fn persisted_root_hash(system: &NeoSystem, index: u32) -> UInt256 {
        let key = StorageKey::create_with_uint32(i32::MIN, 0xf0, index);
        let item = system.store().try_get(&key).expect("persisted state root");
        UInt256::from_bytes(&item.value()[..32]).expect("state root hash")
    }

    #[derive(Debug)]
    struct FailingAudit;

    #[async_trait]
    impl AuditLog for FailingAudit {
        async fn append(&self, _input: AuditInput) -> Result<AuditEvent, AuditError> {
            Err(AuditError::Unavailable)
        }

        async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
            Err(AuditError::Unavailable)
        }
    }

    #[derive(Debug)]
    struct FailOnCallAudit {
        fail_on: usize,
        calls: AtomicUsize,
        inner: MemoryAuditLog,
    }

    impl FailOnCallAudit {
        fn new(fail_on: usize) -> Self {
            Self {
                fail_on,
                calls: AtomicUsize::new(0),
                inner: MemoryAuditLog::default(),
            }
        }
    }

    #[async_trait]
    impl AuditLog for FailOnCallAudit {
        async fn append(&self, input: AuditInput) -> Result<AuditEvent, AuditError> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel) + 1;
            if call == self.fail_on {
                Err(AuditError::Unavailable)
            } else {
                self.inner.append(input).await
            }
        }

        async fn snapshot(&self) -> Result<Vec<AuditEvent>, AuditError> {
            self.inner.snapshot().await
        }
    }

    fn section(name: &str) -> DiagnosticsSection {
        DiagnosticsSection {
            enabled: true,
            observe_only: false,
            audit_enabled: true,
            audit_path: std::env::temp_dir()
                .join(format!("neo-task9-{name}-{}.jsonl", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            evidence_queue_capacity: 16,
            event_freshness_ms: 10_000,
            sampling_interval_ms: 200,
            ..DiagnosticsSection::default()
        }
    }

    fn state_service_settings() -> StateServiceSettings {
        StateServiceSettings {
            full_state: false,
            path: "Data_MPT_DiagnosticsTest".to_owned(),
            network: ProtocolSettings::default().network,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: neo_system::state_service::UnhandledExceptionPolicy::StopPlugin,
        }
    }

    async fn wait_for_transaction_finding(runtime: &AiAgentRuntime, hash: &str) -> EvidenceRecord {
        for _ in 0..100 {
            if let Some(record) = runtime.transaction_finding(hash) {
                return record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("deterministic transaction finding was not received");
    }

    /// Returns the deterministic transaction-validation finding for `hash`
    /// carrying the given `action` field (`record_only`,
    /// `quarantine_transaction` or `release_transaction`).
    fn transaction_validation_finding_with_action(
        runtime: &AiAgentRuntime,
        hash: &str,
        action: &str,
    ) -> Option<EvidenceRecord> {
        runtime
            .inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.transaction_hash.as_deref() == Some(hash)
                    && record.item.fields().get("action").and_then(Value::as_str) == Some(action)
            })
            .cloned()
    }

    async fn wait_for_execution_observation(
        runtime: &AiAgentRuntime,
        hash: &str,
    ) -> EvidenceRecord {
        for _ in 0..100 {
            if let Some(record) = runtime.execution_observation(hash) {
                return record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("canonical transaction execution observation was not received");
    }

    fn faulting_block(system: &NeoSystem) -> (Block, UInt256) {
        let mut transaction = Transaction::new();
        transaction.set_nonce(13);
        transaction.set_valid_until_block(100);
        transaction.set_script(vec![OpCode::ABORT.byte()]);
        transaction.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        transaction.set_witnesses(vec![Witness::new()]);
        let transaction_hash = transaction.try_hash().expect("transaction hash");

        let mut genesis = system.genesis_block().as_ref().clone();
        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(genesis.hash());
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(1);
        header.witness = Witness::new();

        let mut block = Block::new();
        block.header = header;
        block.transactions = vec![transaction];
        block.try_rebuild_merkle_root().expect("merkle root");
        (block, transaction_hash)
    }

    fn persist_faulting_block(
        system: &NeoSystem,
    ) -> (VmState, Option<String>, i64, UInt256, UInt256) {
        let (mut block, transaction_hash) = faulting_block(system);
        let block_hash = block.try_hash().expect("block hash");
        let executed = system
            .persist_block_with_receipts((&block).into())
            .expect("persist faulting block");
        let transaction_result = executed
            .into_iter()
            .find(|result| result.transaction.is_some())
            .expect("transaction execution result");
        (
            transaction_result.vm_state,
            transaction_result.exception,
            transaction_result.gas_consumed,
            block_hash,
            transaction_hash,
        )
    }

    fn execution_observation(
        system: &NeoSystem,
        transaction_hash: UInt256,
    ) -> TransactionExecutionObservation {
        TransactionExecutionObservation {
            transaction_hash,
            block_height: system.current_block_index(),
            block_hash: system
                .block_hash_at(system.current_block_index())
                .expect("current block hash"),
            execution_stage: TransactionExecutionStage::Application,
            vm_state: VmState::FAULT,
            exception_category: TransactionExecutionExceptionCategory::CaughtVmException,
            gas_consumed: 7,
            schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
            rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
            observed_at_unix_ms: unix_time_ms(),
        }
    }

    fn request(
        action: OperationalAction,
        target: Option<String>,
        record: &EvidenceRecord,
    ) -> OperationalActionRequest {
        OperationalActionRequest {
            action,
            target,
            evidence: vec![ActionEvidence {
                id: record.id.clone(),
                content_hash: record.content_hash.clone(),
                authority: record.authority,
            }],
            model_recommended: false,
        }
    }

    fn deterministic_record(
        runtime: &AiAgentRuntime,
        id: &str,
        transaction_hash: Option<String>,
        observed_at_unix_ms: u64,
    ) -> EvidenceRecord {
        let mut fields = BTreeMap::from([
            (
                "rule_version".to_string(),
                json!(runtime.inner.config.rule_version),
            ),
            ("action".to_string(), json!("test_action")),
        ]);
        if let Some(hash) = transaction_hash.as_ref() {
            fields.insert("transaction_hash".to_string(), json!(hash));
        }
        let item = EvidenceItem::new(
            id,
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            0,
            fields,
        )
        .expect("item");
        EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::DeterministicRule,
            observed_at_unix_ms,
            transaction_hash,
            0,
            runtime
                .inner
                .system
                .block_hash_at(0)
                .map(|hash| hash.to_string()),
            runtime.inner.config.rule_version.clone(),
        )
        .expect("record")
    }

    fn runtime_with_action_store(
        config: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> (TempDir, AiAgentRuntime, Arc<DurableActionStore>) {
        let directory = tempfile::tempdir().expect("action store directory");
        let store = Arc::new(
            DurableActionStore::open(directory.path().join("actions.jsonl")).expect("action store"),
        );
        let runtime = AiAgentRuntime::initialize(config, system, audit, Some(store.clone()))
            .expect("runtime with action store");
        (directory, runtime, store)
    }

    fn set_gas_balance(snapshot: &DataCache, account: UInt160, amount: i64) {
        let key = StorageKey::create_with_uint160(GasToken::new().id(), PREFIX_ACCOUNT, &account);
        let state = AccountState::with_balance(BigInt::from(amount));
        let bytes = BinarySerializer::serialize_stack_value(
            &state.to_stack_value(),
            &ExecutionEngineLimits::default(),
        )
        .expect("serialize account state");
        snapshot.update(key, StorageItem::from_bytes(bytes));
    }

    fn signed_probe_transaction(
        settings: &ProtocolSettings,
        key_seed: u8,
        nonce: u32,
    ) -> Transaction {
        let keypair = KeyPair::from_private_key(&[key_seed; 32]).expect("probe keypair");
        let mut transaction = Transaction::new();
        transaction.set_nonce(nonce);
        transaction.set_valid_until_block(100);
        transaction.set_system_fee(0);
        transaction.set_network_fee(1_0000_0000);
        transaction.set_script(vec![OpCode::PUSH1.byte()]);
        transaction.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);
        let sign_data = get_sign_data_vec(&transaction, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("probe signature");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        transaction.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            keypair.get_verification_script(),
        )]);
        transaction
    }

    /// Seeds the system mempool with a fully verified transaction (plus one
    /// filler from a second sender, because the mempool's quarantine impact
    /// limit always keeps at least one unquarantined transaction in the
    /// pool) and returns the transaction hash together with its unsigned
    /// serialization, the exact bytes the probe contract hashes.
    fn seed_verified_transactions(system: &NeoSystem, key_seed: u8) -> (UInt256, Vec<u8>) {
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let transaction = signed_probe_transaction(&settings, key_seed, u32::from(key_seed));
        let filler = signed_probe_transaction(
            &settings,
            key_seed.wrapping_add(0x40),
            u32::from(key_seed) + 0x40,
        );
        set_gas_balance(
            &snapshot,
            transaction.sender().expect("transaction sender"),
            5_0000_0000,
        );
        set_gas_balance(
            &snapshot,
            filler.sender().expect("filler sender"),
            5_0000_0000,
        );
        {
            let mempool = system.mempool();
            let mut pool = mempool.lock();
            assert_eq!(
                pool.try_add(filler, &snapshot, &settings),
                VerifyResult::Succeed
            );
            assert_eq!(
                pool.try_add(transaction.clone(), &snapshot, &settings),
                VerifyResult::Succeed
            );
        }
        let mut writer = BinaryWriter::new();
        transaction
            .serialize_unsigned(&mut writer)
            .expect("serialize unsigned probe transaction");
        let transaction_bytes = writer.into_bytes();
        let hash = transaction.try_hash().expect("probe transaction hash");
        (hash, transaction_bytes)
    }

    fn successful_probe_controller_result(
        system: &NeoSystem,
        store: Option<&DurableActionStore>,
        seed: u8,
        authorizer_rule_version: &str,
    ) -> ProbeControllerResult {
        // The probe transaction must be a genuinely verified mempool
        // transaction: the trusted quarantine effect only labels transactions
        // the mempool actually holds, and the contract's natural-order sha256
        // hex of the unsigned serialization resolves back to the very same
        // on-chain hash.
        let (transaction_hash, transaction_bytes) = seed_verified_transactions(system, seed);
        let transaction_hash = hex::encode(transaction_hash.to_array());
        let now = unix_time_ms().max(2);
        let operation_id = OperationId::new(format!("task36-operation-{seed}")).unwrap();
        let run_id = RunId::new(format!("task36-run-{seed}")).unwrap();
        let height = system.current_block_index();
        let job = ProbeJob::new(
            operation_id.clone(),
            run_id.clone(),
            ProbeAction::TransactionPreflight,
            ProbeTransaction {
                transaction_hash,
                transaction_bytes,
            },
            ProbeContext {
                observed_height: height,
                tip_hash: canonical_block_hash(
                    &system.block_hash_at(height).expect("snapshot root"),
                ),
                requested_at_unix_ms: now,
                node_instance_id: "task36-local-diagnostics".to_owned(),
            },
            ProbeBuild {
                binary_hash: "bc".repeat(32),
                version: "neo-probe-task36".to_owned(),
            },
            ProbeBudget {
                max_duration_ms: 1_000,
                max_output_bytes: 4_096,
                max_observations: 8,
            },
            AuthorizerMetadata {
                evidence_id: format!("task36-authorizer-{seed}"),
                evidence_hash: "cd".repeat(32),
                authority: EvidenceAuthority::DeterministicRule,
                rule_version: authorizer_rule_version.to_owned(),
                authorized_at_unix_ms: now - 1,
            },
        )
        .expect("probe job");
        let binding_hash = job.binding_hash().expect("job binding");
        // Mirror the controller's durable StartProbeRequest lifecycle so the
        // runtime completion consumer's durable-state verification observes
        // exactly what a real controller persists for a completed probe.
        if let Some(store) = store {
            let binding = ActionBinding::new(
                operation_id.clone(),
                OperationalAction::StartProbeRequest,
                Some(format!("{run_id}:{binding_hash}")),
                vec![format!("task36-authorizer-{seed}")],
                vec!["cd".repeat(32)],
                authorizer_rule_version.to_owned(),
            )
            .expect("probe operation binding");
            assert!(matches!(
                store.admit(binding, now),
                Ok(AdmitOutcome::Admitted(_))
            ));
            store
                .mark_running(&operation_id, now)
                .expect("probe operation running");
            store
                .mark_completed(&operation_id, now)
                .expect("probe operation completed");
        }
        let replay = ProbeOutcome {
            schema_version: neo_ai_agent::PROBE_SCHEMA_VERSION,
            operation_id: operation_id.clone(),
            run_id: run_id.clone(),
            action: ProbeAction::TransactionPreflight,
            job_hash: binding_hash,
            reported_authority: EvidenceAuthority::Observation,
            status: ProbeOutcomeStatus::Succeeded,
            started_at_unix_ms: now,
            completed_at_unix_ms: now,
            observations: BTreeMap::from([(
                "preflight".to_owned(),
                json!({
                    "accepted": false,
                    "repeatable": true,
                    "classification": "deterministic_attack_transaction"
                }),
            )]),
        };
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).expect("trusted identity");
        let result = ProbeResultValidator
            .validate_double_replay(&identity, &job, replay.clone(), replay, unix_time_ms())
            .expect("validated double replay");
        ProbeControllerResult {
            operation_id,
            run_id,
            verdict: ProbeVerdict::Completed(result),
        }
    }

    fn expected_probe_action_id(
        runtime: &AiAgentRuntime,
        controller_result: &ProbeControllerResult,
    ) -> OperationId {
        let ProbeVerdict::Completed(result) = &controller_result.verdict else {
            panic!("completed probe result expected");
        };
        let finding = ProbeDeterministicRule
            .evaluate(result)
            .expect("fixed rule")
            .expect("transaction isolation finding");
        let record = finding.record();
        runtime
            .durable_action_binding(
                &action_request(
                    OperationalAction::QuarantineTransaction,
                    record.transaction_hash.clone(),
                    record,
                ),
                std::slice::from_ref(record),
            )
            .expect("durable probe action binding")
            .operation_id
    }

    async fn wait_until(predicate: impl Fn() -> bool) {
        for _ in 0..100 {
            if predicate() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("condition was not reached");
    }

    #[test]
    fn delayed_progress_signal_cannot_regress_last_progress_time() {
        let last_progress = AtomicU64::new(1_000);
        advance_last_progress(&last_progress, 2_000);
        advance_last_progress(&last_progress, 1_500);
        assert_eq!(last_progress.load(Ordering::Acquire), 2_000);
    }

    #[tokio::test]
    async fn fair_receiver_alternates_when_both_bounded_queues_stay_busy() {
        let (exporter, mut evidence_receiver) =
            EvidenceExporter::bounded(4).expect("evidence queue");
        for sequence in 0..2 {
            let item = EvidenceItem::new(
                format!("fair-evidence-{sequence}"),
                EvidenceKind::ChainProgress,
                EvidenceSource::NodeMetrics,
                0,
                BTreeMap::new(),
            )
            .expect("fair evidence item");
            let record = EvidenceRecord::new(
                item,
                EvidenceAuthority::Observation,
                unix_time_ms(),
                None,
                "fair-test-v1",
            )
            .expect("fair evidence record");
            assert_eq!(exporter.try_export(record), ExportResult::Enqueued);
        }
        let (execution_sender, mut execution_receiver) = mpsc::channel(4);
        for sequence in 0..2 {
            execution_sender
                .try_send(TransactionExecutionObservation {
                    transaction_hash: UInt256::from([0x60 + sequence; 32]),
                    block_height: 0,
                    block_hash: UInt256::zero(),
                    execution_stage: TransactionExecutionStage::Application,
                    vm_state: VmState::FAULT,
                    exception_category: TransactionExecutionExceptionCategory::VmFault,
                    gas_consumed: 0,
                    schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
                    rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
                    observed_at_unix_ms: unix_time_ms(),
                })
                .expect("canonical execution queue");
        }
        let restart = Notify::new();
        let mut prefer_evidence = true;
        let mut observed = Vec::new();
        for _ in 0..4 {
            let WorkerEvent::Input(input) = receive_worker_event(
                &mut evidence_receiver,
                &mut execution_receiver,
                &mut None,
                &restart,
                prefer_evidence,
            )
            .await
            else {
                panic!("worker input expected");
            };
            prefer_evidence = matches!(&input, WorkerInput::Execution(_));
            observed.push(matches!(input, WorkerInput::Evidence(_)));
        }
        assert_eq!(observed, vec![true, false, true, false]);
    }

    #[tokio::test]
    async fn consensus_snapshot_uses_persisted_height_and_observed_peer_frontier() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime = AiAgentRuntime::with_audit(
            &section("real-snapshot"),
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let peer_address = "127.0.0.1:30333".parse().expect("peer address");
        system
            .add_peer(peer_address, Some(30333), 0, 0, 7)
            .expect("peer observation");
        wait_until(|| {
            runtime
                .inner
                .peer_observation_available
                .load(Ordering::Acquire)
                && runtime.inner.observed_peer_count.load(Ordering::Acquire) == 1
        })
        .await;

        assert_eq!(
            runtime.try_export_consensus_snapshot(BTreeMap::from([
                ("view".to_string(), json!(2)),
                ("active_validators".to_string(), json!(1)),
                ("expected_validators".to_string(), json!(1)),
            ])),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| {
            runtime
                .inner
                .registry
                .lock()
                .records
                .values()
                .any(|record| record.kind == EvidenceKind::ConsensusState)
        })
        .await;
        let record = runtime
            .inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| record.kind == EvidenceKind::ConsensusState)
            .cloned()
            .expect("consensus snapshot record");
        let fields = record.item.fields();
        assert_eq!(record.observed_height, system.current_block_index());
        assert_eq!(fields["local_height"], system.current_block_index());
        assert_eq!(fields["connected_peers"], 1);
        assert_eq!(fields["max_peer_height"], 7);
        assert_eq!(fields["network_height"], 7);
        assert!(
            fields["last_progress_unix_ms"]
                .as_u64()
                .is_some_and(|time| time > 0)
        );
        assert!(
            fields["peer_observed_at_unix_ms"]
                .as_u64()
                .is_some_and(|time| time > 0)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn disabled_runtime_is_protocol_inert() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime =
            AiAgentRuntime::new(&DiagnosticsSection::default(), system.clone()).expect("runtime");
        assert!(!runtime.is_enabled());
        assert_eq!(
            runtime.try_export(
                EvidenceKind::ConsensusState,
                EvidenceSource::NodeRuntime,
                0,
                BTreeMap::new(),
            ),
            EvidenceSubmitResult::Disabled
        );
        assert_eq!(
            runtime.try_export_execution_observation(&execution_observation(
                &system,
                UInt256::from([0x11; 32]),
            )),
            EvidenceSubmitResult::Disabled
        );
        assert_eq!(runtime.inner.received_events.load(Ordering::Relaxed), 0);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn canonical_execution_queue_full_drops_immediately() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("canonical-queue-full");
        config.evidence_queue_capacity = 1;
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let observation = execution_observation(&system, UInt256::from([0x12; 32]));
        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        let started = std::time::Instant::now();
        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::DroppedFull
        );
        assert!(started.elapsed() < Duration::from_millis(50));
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canonical_observer_ignores_worker_lock_during_concurrent_restart() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime = AiAgentRuntime::with_audit(
            &section("observer-worker-lock"),
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let generation = runtime.inner.worker_generation.load(Ordering::Acquire);
        let worker_guard = runtime.inner.worker.lock();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let observer_runtime = runtime.clone();
        let observer_system = system.clone();
        let observer = std::thread::spawn(move || {
            for _ in 0..64 {
                observer_runtime.request_worker_restart();
            }
            let started = std::time::Instant::now();
            let result = persist_faulting_block(&observer_system);
            result_tx
                .send((started.elapsed(), result))
                .expect("observer result");
        });
        let (elapsed, result) = result_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("canonical observer blocked on worker management lock");
        assert!(elapsed < Duration::from_millis(500));
        assert_eq!(result.0, VmState::FAULT);
        observer.join().expect("observer thread");
        assert!(runtime.inner.worker_generation.load(Ordering::Acquire) > generation);
        drop(worker_guard);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn canonical_persistence_fault_enters_runtime_as_observation_only() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("canonical-fault");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");

        let (vm_state, _, gas_consumed, block_hash, transaction_hash) =
            persist_faulting_block(&system);
        assert_eq!(vm_state, VmState::FAULT);
        let record = wait_for_execution_observation(&runtime, &transaction_hash.to_string()).await;
        let fields = record.item.fields();
        assert_eq!(record.authority, EvidenceAuthority::Observation);
        assert_eq!(record.kind, EvidenceKind::TransactionExecution);
        assert_eq!(record.source, EvidenceSource::NodeRuntime);
        assert_eq!(record.observed_height, 1);
        assert_eq!(record.tip_height, 1);
        assert_eq!(
            record.tip_hash.as_deref(),
            Some(block_hash.to_string().as_str())
        );
        assert_eq!(record.rule_version, config.rule_version);
        assert_eq!(fields["transaction_hash"], transaction_hash.to_string());
        assert_eq!(fields["block_height"], 1);
        assert_eq!(fields["block_hash"], block_hash.to_string());
        assert_eq!(fields["execution_stage"], "application");
        assert_eq!(fields["vm_state"], "FAULT");
        assert_eq!(fields["exception_category"], "caught_vm_exception");
        assert_eq!(fields["gas_consumed"], gas_consumed);
        assert_eq!(fields["schema_version"], 1);
        assert_eq!(
            fields["observation_rule_version"],
            "canonical-transaction-execution-v1"
        );
        assert_eq!(fields["rule_version"], config.rule_version);
        assert!(
            fields["observed_at_unix_ms"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        assert_eq!(fields["default_disposition"], "observe_only");
        assert_eq!(fields["attack_classification"], "not_determined");
        assert!(!fields.contains_key("script"));
        assert!(
            runtime
                .transaction_finding(&transaction_hash.to_string())
                .is_none()
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        assert!(
            !runtime
                .audit_snapshot()
                .await
                .iter()
                .any(|event| event.kind == AuditEventKind::OperationalActionExecuted)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn node_openai_provider_performs_bounded_real_http_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let endpoint = url::Url::parse(&format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().expect("provider server address")
        ))
        .expect("provider endpoint");
        let (request_sender, request_receiver) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set provider read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read provider request");
                assert_ne!(count, 0, "provider request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
            }
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture provider request");
            let body = json!({
                "choices": [{"message": {"content": "{\"diagnosis\":\"local\"}"}}],
                "usage": {"prompt_tokens": 17, "completion_tokens": 5}
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write provider response");
        });

        let credential_name = format!("NEO_NODE_TEST_PROVIDER_{}", std::process::id());
        // SAFETY: this process-unique variable is not read or written by any other test.
        unsafe { std::env::set_var(&credential_name, "local-test-credential") };
        let provider = build_model_provider(Some(&crate::config::ModelProviderSection {
            kind: ProviderKind::OpenAiCompatible,
            endpoint,
            model: "local-diagnostic-model".to_owned(),
            credential_env: Some(
                neo_ai_agent::CredentialEnv::new(&credential_name).expect("credential name"),
            ),
            limits: neo_ai_agent::RuntimeLimits {
                max_retries: 0,
                ..neo_ai_agent::RuntimeLimits::default()
            },
        }))
        .expect("build node provider");
        let response = provider
            .complete(ModelRequest {
                system_instruction: "diagnose only".to_owned(),
                evidence_json: "{\"kind\":\"test\"}".to_owned(),
                max_output_tokens: 32,
            })
            .await
            .expect("complete local provider request");
        // SAFETY: see the corresponding set_var above.
        unsafe { std::env::remove_var(&credential_name) };

        assert_eq!(response.content, "{\"diagnosis\":\"local\"}");
        assert_eq!(response.input_tokens, Some(17));
        assert_eq!(response.output_tokens, Some(5));
        let request = request_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("captured provider request");
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer local-test-credential\r\n")
        );
        server.join().expect("provider server task");
    }

    #[tokio::test]
    async fn explicitly_enabled_restricted_probe_fails_runtime_startup_closed() {
        let directory = tempfile::tempdir().expect("probe worker directory");
        let executable = directory.path().join(if cfg!(windows) {
            "neo-probe.exe"
        } else {
            "neo-probe"
        });
        std::fs::write(&executable, b"not a sandbox boundary").expect("probe worker fixture");
        let mut section = section("unsupported-native-probe");
        section.restricted_probe_enabled = true;
        section.restricted_probe_worker_path = executable.to_string_lossy().into_owned();
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");

        let Err(error) = AiAgentRuntime::new(&section, system.clone()) else {
            panic!("native probe must fail closed")
        };
        assert!(
            error.to_string().contains("unsupported"),
            "unexpected startup error: {error:#}"
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn disabled_full_or_closed_sink_cannot_change_fault_result_or_state_root() {
        let disabled_system = NeoSystem::new_with_state_service(
            ProtocolSettings::default(),
            None,
            None,
            Some(state_service_settings()),
        )
        .expect("disabled system");
        let disabled_runtime =
            AiAgentRuntime::new(&DiagnosticsSection::default(), disabled_system.clone())
                .expect("disabled runtime");
        assert!(!disabled_runtime.is_enabled());
        let disabled_result = persist_faulting_block(&disabled_system);
        let disabled_root_hash = persisted_root_hash(&disabled_system, 1);

        let full_system = NeoSystem::new_with_state_service(
            ProtocolSettings::default(),
            None,
            None,
            Some(state_service_settings()),
        )
        .expect("full system");
        let (full_sink, mut full_receiver) = mpsc::channel(1);
        full_sink
            .try_send(execution_observation(
                &full_system,
                UInt256::from([0x22; 32]),
            ))
            .expect("seed full canonical queue");
        full_system
            .register_transaction_execution_sink(full_sink)
            .expect("register full sink");
        let full_result = persist_faulting_block(&full_system);
        let full_root_hash = persisted_root_hash(&full_system, 1);
        assert_eq!(
            full_receiver
                .recv()
                .await
                .expect("queue seed")
                .transaction_hash,
            UInt256::from([0x22; 32])
        );

        let closed_system = NeoSystem::new_with_state_service(
            ProtocolSettings::default(),
            None,
            None,
            Some(state_service_settings()),
        )
        .expect("closed sink system");
        let (closed_sink, closed_receiver) = mpsc::channel(1);
        drop(closed_receiver);
        closed_system
            .register_transaction_execution_sink(closed_sink)
            .expect("register closed sink");
        let closed_result = persist_faulting_block(&closed_system);
        let closed_root_hash = persisted_root_hash(&closed_system, 1);

        assert_eq!(disabled_result, full_result);
        assert_eq!(disabled_result, closed_result);
        assert_eq!(disabled_result.0, VmState::FAULT);
        assert_ne!(disabled_root_hash, UInt256::zero());
        assert_eq!(disabled_root_hash, full_root_hash);
        assert_eq!(disabled_root_hash, closed_root_hash);
        assert_eq!(disabled_system.current_block_index(), 1);
        assert_eq!(full_system.current_block_index(), 1);
        assert_eq!(closed_system.current_block_index(), 1);
        assert_eq!(disabled_system.block_hash_at(1), Some(disabled_result.3));
        assert_eq!(full_system.block_hash_at(1), Some(full_result.3));
        assert_eq!(closed_system.block_hash_at(1), Some(closed_result.3));
        disabled_system.shutdown().await.expect("disabled shutdown");
        full_system.shutdown().await.expect("full shutdown");
        closed_system
            .shutdown()
            .await
            .expect("closed sink shutdown");
    }

    #[tokio::test]
    async fn repeated_local_execution_fault_creates_probe_candidate_without_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("reproducible-execution-fault");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let transaction_hash = UInt256::from([0x24; 32]);
        let observation = TransactionExecutionObservation {
            transaction_hash,
            block_height: 0,
            block_hash: system.block_hash_at(0).expect("genesis hash"),
            execution_stage: TransactionExecutionStage::Application,
            vm_state: VmState::FAULT,
            exception_category: TransactionExecutionExceptionCategory::CaughtVmException,
            gas_consumed: 7,
            schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
            rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
            observed_at_unix_ms: unix_time_ms(),
        };

        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 1).await;
        assert!(
            runtime
                .transaction_finding(&transaction_hash.to_string())
                .is_none()
        );

        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        let candidate = wait_for_transaction_finding(&runtime, &transaction_hash.to_string()).await;
        assert_eq!(candidate.authority, EvidenceAuthority::DeterministicRule);
        assert_eq!(
            candidate.item.fields()["candidate_action"],
            "restricted_probe_candidate"
        );
        assert_eq!(candidate.item.fields()["reproducible_count"], 2);
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        assert!(
            !runtime
                .audit_snapshot()
                .await
                .iter()
                .any(|event| event.kind == AuditEventKind::OperationalActionExecuted)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn trusted_local_observation_can_trigger_controlled_stall_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("trusted-stall");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let local_height = u64::from(system.current_block_index());
        runtime.inner.last_progress_unix_ms.store(
            unix_time_ms().saturating_sub(StallPolicy::default().level1_after_ms + 1),
            Ordering::Release,
        );
        runtime
            .inner
            .observed_peer_height
            .store(local_height, Ordering::Release);
        runtime
            .inner
            .observed_peer_count
            .store(1, Ordering::Release);
        runtime
            .inner
            .peer_observed_at_unix_ms
            .store(unix_time_ms(), Ordering::Release);
        runtime
            .inner
            .peer_observation_available
            .store(true, Ordering::Release);
        assert_eq!(
            runtime.inner.sampling_interval_ms.load(Ordering::Acquire),
            200
        );
        assert_eq!(
            runtime.try_export_consensus_snapshot(BTreeMap::from([
                ("view".to_string(), json!(1)),
                ("active_validators".to_string(), json!(1)),
                ("expected_validators".to_string(), json!(1)),
            ])),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| runtime.inner.analyzed_events.load(Ordering::Relaxed) == 1).await;
        wait_until(|| runtime.inner.sampling_interval_ms.load(Ordering::Acquire) == 100).await;
        assert!(runtime.inner.received_events.load(Ordering::Relaxed) >= 1);
        let audit = runtime.audit_snapshot().await;
        assert!(audit.iter().any(|event| {
            event.kind == AuditEventKind::OperationalActionExecuted
                && event.action.as_deref() == Some("IncreaseSampling")
        }));
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn malicious_model_consensus_state_is_audited_and_cannot_trigger_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("model-stall-reject");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        runtime.inner.last_progress_unix_ms.store(
            unix_time_ms().saturating_sub(StallPolicy::default().level3_after_ms + 1),
            Ordering::Release,
        );
        let item = EvidenceItem::new(
            "malicious-model-stall",
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            0,
            BTreeMap::from([
                ("network_height".to_string(), json!(1)),
                ("view".to_string(), json!(9)),
                ("connected_peers".to_string(), json!(8)),
                ("active_validators".to_string(), json!(7)),
                ("expected_validators".to_string(), json!(7)),
            ]),
        )
        .expect("model consensus item");
        let record = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Model,
            unix_time_ms(),
            None,
            0,
            system.block_hash_at(0).map(|tip| tip.to_string()),
            config.rule_version.clone(),
        )
        .expect("model consensus record");
        assert_eq!(
            runtime.try_submit_record(record),
            EvidenceSubmitResult::Enqueued
        );

        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) == 1).await;
        let mut rejected_audit = None;
        for _ in 0..100 {
            let events = runtime.audit_snapshot().await;
            if events.iter().any(|event| {
                event.kind == AuditEventKind::DiagnosisRejected
                    && event.outcome_code == "model_authority_rejected"
            }) {
                rejected_audit = Some(events);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let audit = rejected_audit.expect("model detector input rejection audit");
        assert!(
            runtime
                .inner
                .registry
                .lock()
                .records
                .contains_key("malicious-model-stall")
        );
        assert_eq!(runtime.inner.analyzed_events.load(Ordering::Relaxed), 0);
        assert_eq!(
            runtime.inner.sampling_interval_ms.load(Ordering::Acquire),
            200
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        assert!(
            !audit
                .iter()
                .any(|event| { event.kind == AuditEventKind::OperationalActionExecuted })
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn receiver_rejects_tampered_authority_and_tip_context() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime =
            AiAgentRuntime::new(&section("receiver-validation"), system.clone()).expect("runtime");

        let item = EvidenceItem::new(
            "tampered-record",
            EvidenceKind::ChainProgress,
            EvidenceSource::NodeMetrics,
            0,
            BTreeMap::new(),
        )
        .expect("item");
        let mut tampered = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Observation,
            unix_time_ms(),
            None,
            0,
            system.block_hash_at(0).map(|hash| hash.to_string()),
            runtime.inner.config.rule_version.clone(),
        )
        .expect("record");
        tampered.content_hash = "00".repeat(32);
        assert_eq!(
            runtime.try_submit_record(tampered),
            EvidenceSubmitResult::Enqueued
        );

        let external_rule = deterministic_record(&runtime, "external-rule", None, unix_time_ms());
        assert_eq!(
            runtime.try_submit_record(external_rule),
            EvidenceSubmitResult::Enqueued
        );

        let item = EvidenceItem::new(
            "wrong-tip",
            EvidenceKind::ChainProgress,
            EvidenceSource::NodeMetrics,
            0,
            BTreeMap::new(),
        )
        .expect("item");
        let wrong_tip = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Observation,
            unix_time_ms(),
            None,
            0,
            Some(UInt256::from([0x7f; 32]).to_string()),
            runtime.inner.config.rule_version.clone(),
        )
        .expect("record");
        assert_eq!(
            runtime.try_submit_record(wrong_tip),
            EvidenceSubmitResult::Enqueued
        );

        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 3).await;
        {
            let registry = runtime.inner.registry.lock();
            assert!(!registry.records.contains_key("tampered-record"));
            assert!(!registry.records.contains_key("external-rule"));
            assert!(!registry.records.contains_key("wrong-tip"));
        }
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn model_claim_and_wrong_hash_cannot_authorize_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("model-reject");
        let runtime = AiAgentRuntime::new(&config, system.clone()).expect("runtime");
        let hash = UInt256::from([0x31; 32]).to_string();
        runtime.try_export(
            EvidenceKind::TransactionValidation,
            EvidenceSource::NodeRuntime,
            0,
            BTreeMap::from([
                ("transaction_hash".to_string(), json!(hash)),
                ("validation_result".to_string(), json!("invalid")),
                ("deterministic".to_string(), json!(true)),
            ]),
        );
        let record = wait_for_transaction_finding(&runtime, &hash).await;
        let mut forged = request(
            OperationalAction::QuarantineTransaction,
            Some(hash),
            &record,
        );
        forged.evidence[0].authority = EvidenceAuthority::Model;
        assert_eq!(
            runtime.apply_action(forged).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        let mut wrong_hash = request(OperationalAction::IncreaseSampling, None, &record);
        wrong_hash.evidence[0].content_hash = "00".repeat(32);
        assert_eq!(
            runtime.apply_action(wrong_hash).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );

        let model_item = EvidenceItem::new(
            "model-only",
            EvidenceKind::PeerState,
            EvidenceSource::PeerSnapshot,
            0,
            BTreeMap::new(),
        )
        .expect("model item");
        let model_record = EvidenceRecord::new(
            model_item,
            EvidenceAuthority::Model,
            unix_time_ms(),
            None,
            runtime.inner.config.rule_version.clone(),
        )
        .expect("model record");
        runtime.inner.registry.lock().insert(model_record.clone());
        assert_eq!(
            runtime
                .apply_action(request(
                    OperationalAction::IncreaseSampling,
                    None,
                    &model_record,
                ))
                .await,
            ActionResult::Rejected(OperationalDecision::MissingDeterministicEvidence)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn model_transaction_execution_cannot_trigger_action_or_candidate() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("model-execution-reject");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");
        let hash = UInt256::from([0x32; 32]);
        let item = EvidenceItem::new(
            "malicious-model-execution",
            EvidenceKind::TransactionExecution,
            EvidenceSource::NodeRuntime,
            0,
            BTreeMap::from([
                ("transaction_hash".to_string(), json!(hash.to_string())),
                ("block_height".to_string(), json!(0)),
                (
                    "block_hash".to_string(),
                    json!(system.block_hash_at(0).map(|tip| tip.to_string())),
                ),
                ("execution_stage".to_string(), json!("application")),
                ("vm_state".to_string(), json!("FAULT")),
                (
                    "exception_category".to_string(),
                    json!("caught_vm_exception"),
                ),
                ("gas_consumed".to_string(), json!(1)),
                ("schema_version".to_string(), json!(1)),
                ("rule_version".to_string(), json!(config.rule_version)),
                ("default_disposition".to_string(), json!("observe_only")),
            ]),
        )
        .expect("model item");
        let record = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::Model,
            unix_time_ms(),
            Some(hash.to_string()),
            0,
            system.block_hash_at(0).map(|tip| tip.to_string()),
            runtime.inner.config.rule_version.clone(),
        )
        .expect("model record");

        assert_eq!(
            runtime.try_submit_record(record),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 1).await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert!(runtime.transaction_finding(&hash.to_string()).is_none());
        assert!(
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        );
        let audit = runtime.audit_snapshot().await;
        assert!(audit.iter().any(|event| {
            event.kind == AuditEventKind::DiagnosisRejected
                && event.outcome_code == "model_authority_rejected"
        }));
        assert!(
            !audit
                .iter()
                .any(|event| event.kind == AuditEventKind::OperationalActionExecuted)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn stale_evidence_is_rejected_before_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("stale");
        config.event_freshness_ms = 10;
        let runtime = AiAgentRuntime::new(&config, system.clone()).expect("runtime");
        let record = deterministic_record(
            &runtime,
            "stale-rule",
            None,
            unix_time_ms().saturating_sub(100),
        );
        runtime.inner.registry.lock().insert(record.clone());
        assert_eq!(
            runtime
                .apply_action(request(OperationalAction::IncreaseSampling, None, &record))
                .await,
            ActionResult::Rejected(OperationalDecision::StaleEvidence)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn deterministic_transaction_evidence_quarantines_and_audits_binding() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("quarantine");
        config.allow_transaction_quarantine = true;
        let runtime = AiAgentRuntime::new(&config, system.clone()).expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");
        // Quarantine labels only mempool-verified transactions, so seed a
        // genuinely verified transaction (plus filler) and target its hash.
        let (hash, _transaction_bytes) = seed_verified_transactions(&system, 0x42);
        runtime.try_export(
            EvidenceKind::TransactionValidation,
            EvidenceSource::NodeRuntime,
            0,
            BTreeMap::from([
                ("transaction_hash".to_string(), json!(hash.to_string())),
                ("validation_result".to_string(), json!("invalid")),
                ("deterministic".to_string(), json!(true)),
            ]),
        );
        let record = wait_for_transaction_finding(&runtime, &hash.to_string()).await;
        let observed_now = unix_time_ms();
        assert_eq!(
            record.transaction_hash.as_deref(),
            Some(hash.to_string().as_str())
        );
        assert_eq!(record.rule_version, config.rule_version);
        assert!(record.observed_at_unix_ms <= observed_now);
        assert!(
            observed_now.saturating_sub(record.observed_at_unix_ms) <= config.event_freshness_ms
        );
        // Plan §43 evidence threshold: the first deterministic failure only
        // records the finding, without quarantine and without any audited
        // operational action.
        assert_eq!(record.item.fields()["action"], "record_only");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        );
        assert!(
            !runtime
                .audit_snapshot()
                .await
                .iter()
                .any(|event| event.kind == AuditEventKind::OperationalActionExecuted)
        );
        // The second deterministic failure under the same rule version
        // reaches the quarantine evidence threshold and escalates.
        assert_eq!(
            runtime.try_export(
                EvidenceKind::TransactionValidation,
                EvidenceSource::NodeRuntime,
                0,
                BTreeMap::from([
                    ("transaction_hash".to_string(), json!(hash.to_string())),
                    ("validation_result".to_string(), json!("invalid")),
                    ("deterministic".to_string(), json!(true)),
                ]),
            ),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| {
            system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        })
        .await;
        let quarantine_finding = transaction_validation_finding_with_action(
            &runtime,
            &hash.to_string(),
            "quarantine_transaction",
        )
        .expect("quarantine finding");
        assert_eq!(
            quarantine_finding.item.fields()["validation_result"],
            "invalid"
        );
        let audit = runtime.audit_snapshot().await;
        let executed = audit
            .iter()
            .rev()
            .find(|event| event.kind == AuditEventKind::OperationalActionExecuted)
            .expect("executed audit");
        assert_eq!(
            executed.target_hash.as_deref(),
            Some(hash.to_string().as_str())
        );
        assert_eq!(
            executed.rule_version.as_deref(),
            Some(config.rule_version.as_str())
        );
        assert_eq!(
            executed.evidence_hashes,
            vec![quarantine_finding.content_hash]
        );

        assert_eq!(
            runtime.try_export(
                EvidenceKind::TransactionValidation,
                EvidenceSource::NodeRuntime,
                0,
                BTreeMap::from([
                    ("transaction_hash".to_string(), json!(hash.to_string())),
                    ("validation_result".to_string(), json!("succeed")),
                    ("deterministic".to_string(), json!(true)),
                ]),
            ),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| {
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        })
        .await;
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn increase_sampling_reschedules_real_sampler() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime = AiAgentRuntime::new(&section("sampling"), system.clone()).expect("runtime");
        let record = deterministic_record(&runtime, "sampling-rule", None, unix_time_ms());
        runtime.inner.registry.lock().insert(record.clone());
        assert_eq!(
            runtime
                .apply_action(request(OperationalAction::IncreaseSampling, None, &record))
                .await,
            ActionResult::Executed
        );
        assert_eq!(
            runtime.inner.sampling_interval_ms.load(Ordering::Acquire),
            100
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(runtime.inner.sampling_ticks.load(Ordering::Relaxed) >= 2);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn worker_restart_executes_but_probe_and_consensus_restart_are_rejected() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let runtime = AiAgentRuntime::new(&section("actions"), system.clone()).expect("runtime");
        let record = deterministic_record(&runtime, "worker-rule", None, unix_time_ms());
        runtime.inner.registry.lock().insert(record.clone());
        let task_before = runtime
            .inner
            .worker
            .lock()
            .task
            .as_ref()
            .map(JoinHandle::id)
            .expect("worker supervisor task");
        let generation_before = runtime.inner.worker_generation.load(Ordering::Acquire);
        assert_eq!(
            runtime
                .apply_action(request(OperationalAction::RestartAiWorker, None, &record))
                .await,
            ActionResult::Executed
        );
        let task_after = runtime
            .inner
            .worker
            .lock()
            .task
            .as_ref()
            .map(JoinHandle::id)
            .expect("worker supervisor task");
        assert_eq!(task_before, task_after);
        assert_eq!(
            runtime.inner.worker_generation.load(Ordering::Acquire),
            generation_before + 1
        );
        for action in [
            OperationalAction::StartProbeRequest,
            OperationalAction::RestartConsensus,
        ] {
            assert_eq!(
                runtime.apply_action(request(action, None, &record)).await,
                ActionResult::Unsupported
            );
        }
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn trusted_controller_result_runs_policy_and_persists_completed_action() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("task36-success");
        config.allow_transaction_quarantine = true;
        let (_directory, runtime, store) =
            runtime_with_action_store(&config, system.clone(), Arc::new(MemoryAuditLog::default()));
        runtime.configure_mempool(&config).expect("mempool config");
        let controller_result = successful_probe_controller_result(
            &system,
            Some(store.as_ref()),
            1,
            &config.rule_version,
        );
        let operation_id = expected_probe_action_id(&runtime, &controller_result);
        assert_eq!(
            runtime
                .apply_probe_controller_result(controller_result)
                .await,
            ActionResult::Executed
        );
        assert_eq!(
            store.query(&operation_id).unwrap().unwrap().state,
            ActionState::Completed
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn uncertain_or_outer_binding_mismatch_never_enters_action_chain() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("task36-controller-reject");
        config.allow_transaction_quarantine = true;
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let uncertain = ProbeControllerResult {
            operation_id: OperationId::new("task36-uncertain").unwrap(),
            run_id: RunId::new("task36-uncertain-run").unwrap(),
            verdict: ProbeVerdict::Uncertain(
                crate::probe_controller::ProbeUncertainReason::ReplayMismatch,
            ),
        };
        assert_eq!(
            runtime.apply_probe_controller_result(uncertain).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        let mut mismatched =
            successful_probe_controller_result(&system, None, 2, &config.rule_version);
        mismatched.operation_id = OperationId::new("task36-forged-outer-id").unwrap();
        assert_eq!(
            runtime.apply_probe_controller_result(mismatched).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn snapshot_context_or_authorizer_rule_change_is_rejected() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("task36-context-change");
        config.allow_transaction_quarantine = true;
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        let stale = successful_probe_controller_result(&system, None, 3, &config.rule_version);
        let _ = persist_faulting_block(&system);
        assert_eq!(
            runtime.apply_probe_controller_result(stale).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        let wrong_rule =
            successful_probe_controller_result(&system, None, 4, "forged-authorizer-rule-v1");
        assert_eq!(
            runtime.apply_probe_controller_result(wrong_rule).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn pre_effect_audit_failure_has_no_effect_and_persists_rejection() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("task36-pre-audit");
        config.allow_transaction_quarantine = true;
        let (_directory, runtime, store) =
            runtime_with_action_store(&config, system.clone(), Arc::new(FailOnCallAudit::new(1)));
        runtime.configure_mempool(&config).expect("mempool config");
        let controller_result = successful_probe_controller_result(
            &system,
            Some(store.as_ref()),
            5,
            &config.rule_version,
        );
        let operation_id = expected_probe_action_id(&runtime, &controller_result);
        assert_eq!(
            runtime
                .apply_probe_controller_result(controller_result)
                .await,
            ActionResult::Failed
        );
        assert_eq!(
            store.query(&operation_id).unwrap().unwrap().state,
            ActionState::Rejected
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn post_effect_audit_failure_requires_durable_reconciliation() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("task36-post-audit");
        config.allow_transaction_quarantine = true;
        let (_directory, runtime, store) =
            runtime_with_action_store(&config, system.clone(), Arc::new(FailOnCallAudit::new(2)));
        runtime.configure_mempool(&config).expect("mempool config");
        let controller_result = successful_probe_controller_result(
            &system,
            Some(store.as_ref()),
            6,
            &config.rule_version,
        );
        let operation_id = expected_probe_action_id(&runtime, &controller_result);
        assert_eq!(
            runtime
                .apply_probe_controller_result(controller_result)
                .await,
            ActionResult::Failed
        );
        assert_eq!(
            store.query(&operation_id).unwrap().unwrap().state,
            ActionState::ReconciliationRequired
        );
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 1);
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn single_deterministic_validation_failure_records_finding_without_quarantine() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("single-failure-record-only");
        config.allow_transaction_quarantine = true;
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");
        let (hash, _transaction_bytes) = seed_verified_transactions(&system, 0x51);
        assert_eq!(
            runtime.try_export(
                EvidenceKind::TransactionValidation,
                EvidenceSource::NodeRuntime,
                0,
                BTreeMap::from([
                    ("transaction_hash".to_string(), json!(hash.to_string())),
                    ("validation_result".to_string(), json!("invalid")),
                    ("deterministic".to_string(), json!(true)),
                ]),
            ),
            EvidenceSubmitResult::Enqueued
        );
        let record = wait_for_transaction_finding(&runtime, &hash.to_string()).await;
        assert_eq!(record.item.fields()["action"], "record_only");
        // A single deterministic failure never escalates to quarantine.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        assert!(
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        );
        assert!(
            !runtime
                .audit_snapshot()
                .await
                .iter()
                .any(|event| event.kind == AuditEventKind::OperationalActionExecuted)
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn validation_quarantine_audit_failure_leaves_state_unchanged() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let mut config = section("validation-audit-failure");
        config.allow_transaction_quarantine = true;
        let runtime = AiAgentRuntime::with_audit(&config, system.clone(), Arc::new(FailingAudit))
            .expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");
        let (hash, _transaction_bytes) = seed_verified_transactions(&system, 0x43);
        for attempt in 0..2 {
            assert_eq!(
                runtime.try_export(
                    EvidenceKind::TransactionValidation,
                    EvidenceSource::NodeRuntime,
                    0,
                    BTreeMap::from([
                        ("transaction_hash".to_string(), json!(hash.to_string())),
                        ("validation_result".to_string(), json!("invalid")),
                        ("deterministic".to_string(), json!(true)),
                    ]),
                ),
                EvidenceSubmitResult::Enqueued,
                "invalid validation export {attempt}"
            );
        }
        // The evidence threshold is reached, so the quarantine finding is
        // constructed and the audited effect path runs.
        wait_until(|| {
            transaction_validation_finding_with_action(
                &runtime,
                &hash.to_string(),
                "quarantine_transaction",
            )
            .is_some()
        })
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        // H-1 regression: the "authorized" pre-effect audit append fails, so
        // the quarantine effect must be refused entirely.
        assert_eq!(system.mempool().lock().quarantine_count(unix_time_ms()), 0);
        assert!(
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn restricted_probe_candidate_triggers_probe_job_submission() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("probe-candidate-loop");
        let runtime = AiAgentRuntime::with_audit(
            &config,
            system.clone(),
            Arc::new(MemoryAuditLog::default()),
        )
        .expect("runtime");
        // Persist the deterministic faulting transaction so its unsigned
        // serialization is retrievable for the probe contract.
        let (_vm_state, _exception, _gas, _block_hash, transaction_hash) =
            persist_faulting_block(&system);
        // Inject a startup-validated restricted probe configuration (with a
        // readable worker executable) as if `start_probe_controller` had
        // passed its fail-closed gates.
        let directory = tempfile::tempdir().expect("probe worker directory");
        let executable = directory.path().join("probe-worker");
        std::fs::write(&executable, b"probe-worker-bytes").expect("probe worker executable");
        let mut probe_config = ProbeControllerConfig::from(&config);
        probe_config.executable = executable;
        *runtime.inner.probe_config.lock() = Some(probe_config);
        let observation = execution_observation(&system, transaction_hash);
        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 1).await;
        assert_eq!(
            runtime
                .inner
                .probe_submission_attempts
                .load(Ordering::Relaxed),
            0,
            "a single execution observation never yields a probe candidate"
        );
        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        // H-2 closed loop: the reproducible candidate finding is consumed by
        // a probe job submission attempt.
        wait_until(|| {
            runtime
                .inner
                .probe_submission_attempts
                .load(Ordering::Relaxed)
                >= 1
        })
        .await;
        assert_eq!(runtime.inner.probe_candidates.load(Ordering::Relaxed), 1);
        let submitted = runtime.inner.probe_submission_log.lock().clone();
        assert_eq!(submitted.len(), 1);
        let job = &submitted[0];
        assert_eq!(job.action, ProbeAction::TransactionPreflight);
        assert_eq!(
            job.transaction.transaction_hash,
            hex::encode(transaction_hash.to_array())
        );
        assert!(!job.transaction.transaction_bytes.is_empty());
        assert_eq!(
            job.authorizer.authority,
            EvidenceAuthority::DeterministicRule
        );
        // No controller exists behind the fail-closed gates: the attempt is
        // recorded as a skip instead of panicking.
        assert_eq!(
            runtime.inner.probe_submission_skips.load(Ordering::Relaxed),
            1
        );
        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn audit_append_failure_prevents_quarantine_mutation() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
        let config = section("audit-failure");
        let runtime = AiAgentRuntime::with_audit(&config, system.clone(), Arc::new(FailingAudit))
            .expect("runtime");
        runtime.configure_mempool(&config).expect("mempool config");
        let hash = UInt256::from([0x55; 32]);
        let record = deterministic_record(
            &runtime,
            "audit-failure-rule",
            Some(hash.to_string()),
            unix_time_ms(),
        );
        runtime.inner.registry.lock().insert(record.clone());
        assert_eq!(
            runtime
                .apply_action(request(
                    OperationalAction::QuarantineTransaction,
                    Some(hash.to_string()),
                    &record,
                ))
                .await,
            ActionResult::Failed
        );
        assert!(
            !system
                .mempool()
                .lock()
                .is_quarantined(&hash, unix_time_ms())
        );
        system.shutdown().await.expect("shutdown");
    }
}
