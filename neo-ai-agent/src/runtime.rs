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

/// Bounded review-quarantine policy for this runtime.
///
/// Mirrors `neo-mempool`'s node-local quarantine policy, but this crate is
/// deliberately free of workspace-internal dependencies and keys its evidence by
/// a hex transaction hash, so it keeps its own bounded DTO instead of importing
/// the mempool and its ledger/storage/VM dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuarantineConfig {
    pub automatic_enabled: bool,
    pub max_entries: usize,
    pub max_ttl_ms: u64,
    pub selection_scan_budget: usize,
}

/// Evidence-side mirror of the mempool's quarantine record, keyed by a hex hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineMetadata {
    pub transaction_hash: String,
    pub evidence_refs: Vec<String>,
    pub context: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub rule_version: String,
    pub episode: u8,
    pub cumulative_active_ms: u64,
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

#[async_trait]
pub trait ChainHost: Send + Sync {
    fn current_block_index(&self) -> u32;
    fn block_hash_at(&self, height: u32) -> Option<ChainBlockHash>;
    async fn peer_observation(&self) -> Option<PeerObservation>;
    fn unsigned_transaction(
        &self,
        transaction_hash: &str,
        observed_height: u32,
    ) -> Option<UnsignedTransaction>;
}

pub trait MempoolHost: Send + Sync {
    fn configure(&self, config: QuarantineConfig, now_unix_ms: u64) -> Result<(), String>;
    fn resolve_target(&self, target: &str, now_unix_ms: u64) -> Option<String>;
    fn is_quarantined(&self, transaction_hash: &str, now_unix_ms: u64) -> bool;
    fn quarantine(&self, metadata: QuarantineMetadata, now_unix_ms: u64) -> Result<(), String>;
    fn release(&self, transaction_hash: &str) -> bool;
}

pub trait ExecutionObservationHost: Send + Sync {
    fn register_sink(&self, sink: mpsc::Sender<ExecutionObservation>) -> Result<(), String>;
}

pub trait ProbeHost: Send + Sync {
    fn start(
        &self,
        section: &DiagnosticsSection,
        action_store: Option<&Arc<DurableActionStore>>,
        completions: mpsc::Sender<ProbeCompletion>,
    ) -> anyhow::Result<Option<ProbeSession>>;
}

#[derive(Clone)]
pub struct RuntimeHost {
    pub chain: Arc<dyn ChainHost>,
    pub mempool: Arc<dyn MempoolHost>,
    pub execution: Arc<dyn ExecutionObservationHost>,
    pub probe: Arc<dyn ProbeHost>,
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

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub enabled: bool,
    pub evidence_queue_capacity: usize,
    pub event_freshness_ms: u64,
    pub quarantine_ttl_ms: u64,
    pub rule_version: String,
    pub policy: OperationalPolicyConfig,
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

#[doc(hidden)]
#[derive(Default)]
pub struct WorkerState {
    pub task: Option<JoinHandle<()>>,
    pub sampler: Option<JoinHandle<()>>,
}

#[doc(hidden)]
pub struct EvidenceRegistry {
    capacity: usize,
    order: VecDeque<String>,
    pub records: HashMap<String, EvidenceRecord>,
}

impl EvidenceRegistry {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::with_capacity(capacity.max(1)),
            records: HashMap::with_capacity(capacity.max(1)),
        }
    }

    pub fn insert(&mut self, record: EvidenceRecord) {
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

#[doc(hidden)]
pub struct RuntimeInner {
    pub config: RuntimeConfig,
    pub host: RuntimeHost,
    pub evidence_ingress: EvidenceExporter,
    pub execution_ingress: mpsc::Sender<ExecutionObservation>,
    pub probe_controller: Option<Arc<dyn ProbeControl>>,
    pub probe_config: Mutex<Option<ProbeSessionConfig>>,
    pub worker: Mutex<WorkerState>,
    pub worker_generation: AtomicU64,
    pub worker_restart: Arc<Notify>,
    pub audit: Arc<dyn AuditLog>,
    pub action_store: Option<Arc<DurableActionStore>>,
    pub registry: Arc<Mutex<EvidenceRegistry>>,
    execution_repetitions: Mutex<BTreeMap<String, ExecutionRepetition>>,
    validation_failures: Mutex<BTreeMap<String, ValidationFailure>>,
    pub evidence_sequence: AtomicU64,
    pub probe_candidates: AtomicU64,
    pub probe_submission_attempts: AtomicU64,
    pub probe_submission_failures: AtomicU64,
    pub probe_submission_skips: AtomicU64,
    pub audit_append_failures: AtomicU64,
    pub probe_submission_log: Mutex<Vec<ProbeJob>>,
    pub received_events: Arc<AtomicU64>,
    pub analyzed_events: Arc<AtomicU64>,
    pub sampling_ticks: AtomicU64,
    pub last_progress_unix_ms: Arc<AtomicU64>,
    pub last_observed_local_height: AtomicU64,
    pub peer_observation_available: AtomicBool,
    pub observed_peer_count: AtomicU64,
    pub observed_peer_height: AtomicU64,
    pub peer_observed_at_unix_ms: AtomicU64,
    pub sampling_interval_ms: AtomicU64,
    pub sampling_changed: Notify,
}

#[derive(Clone)]
pub struct AiAgentRuntime {
    #[doc(hidden)]
    pub inner: Arc<RuntimeInner>,
}

#[doc(hidden)]
pub enum NodeModelProvider {
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

#[doc(hidden)]
pub fn build_model_provider(
    section: Option<&ModelProviderSection>,
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

#[doc(hidden)]
#[derive(Debug)]
pub enum WorkerInput {
    Evidence(EvidenceRecord),
    Execution(ExecutionObservation),
    ProbeCompletion(ProbeCompletion),
}

#[doc(hidden)]
#[derive(Debug)]
pub enum WorkerEvent {
    Restart,
    Input(WorkerInput),
    ProbeCompletionsClosed,
    Closed,
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

impl AiAgentRuntime {
    pub fn new_with_host(section: &DiagnosticsSection, host: RuntimeHost) -> anyhow::Result<Self> {
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
        Self::initialize_with_host(section, host, audit, action_store)
    }

    #[doc(hidden)]
    pub fn initialize_with_host(
        section: &DiagnosticsSection,
        host: RuntimeHost,
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
        let (probe_completion_sender, probe_completion_receiver) = mpsc::channel(queue_capacity);
        // An explicitly requested probe must validate and start, or initialization
        // fails with the exact reason. It is never silently degraded.
        let probe = if config.enabled {
            host.probe
                .start(section, action_store.as_ref(), probe_completion_sender)?
        } else {
            None
        };
        let probe_completions = probe.as_ref().map(|_| probe_completion_receiver);
        let model_provider = if config.enabled {
            build_model_provider(section.model_provider.as_ref())
                .map_err(|error| anyhow::anyhow!("invalid diagnostics model provider: {error}"))?
        } else {
            NodeModelProvider::Unavailable
        };
        let model_provider = Arc::new(model_provider);
        let initial_height = u64::from(host.chain.current_block_index());
        let initialized_at = unix_time_ms();
        let runtime = Self {
            inner: Arc::new(RuntimeInner {
                sampling_interval_ms: AtomicU64::new(section.sampling_interval_ms),
                config,
                host,
                evidence_ingress,
                execution_ingress,
                probe_controller: probe.as_ref().map(|session| session.control.clone()),
                probe_config: Mutex::new(probe.as_ref().map(|session| session.config.clone())),
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
                probe_completions,
                model_provider,
            )?;
            runtime.start_sampler();
        }
        Ok(runtime)
    }

    #[doc(hidden)]
    pub fn is_enabled(&self) -> bool {
        self.inner.config.enabled
    }

    fn install_execution_sink(&self) -> anyhow::Result<()> {
        self.inner
            .host
            .execution
            .register_sink(self.inner.execution_ingress.clone())
            .map_err(|error| anyhow::anyhow!(error))
    }

    #[doc(hidden)]
    pub fn try_export_consensus_snapshot(
        &self,
        mut fields: BTreeMap<String, Value>,
    ) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        let local_height = self.inner.host.chain.current_block_index();
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

    pub fn configure_mempool(&self, section: &DiagnosticsSection) -> anyhow::Result<()> {
        self.inner
            .host
            .mempool
            .configure(
                QuarantineConfig {
                    automatic_enabled: section.enabled && section.allow_transaction_quarantine,
                    max_entries: section.quarantine_max_entries,
                    max_ttl_ms: section.quarantine_ttl_ms,
                    selection_scan_budget: section.proposal_scan_budget,
                },
                unix_time_ms(),
            )
            .map_err(|error| anyhow::anyhow!(error))
    }

    pub fn try_export(
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
        let tip_height = self.inner.host.chain.current_block_index();
        let tip_hash = self
            .inner
            .host
            .chain
            .block_hash_at(tip_height)
            .map(|hash| hash.display);
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

    #[doc(hidden)]
    pub fn try_export_execution_observation(
        &self,
        observation: &ExecutionObservation,
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
        observation: &ExecutionObservation,
    ) -> Option<EvidenceRecord> {
        let sequence = self.inner.evidence_sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "canonical-execution:{}:{sequence}",
            observation.block_height
        );
        let transaction_hash = observation.transaction_hash.clone();
        let block_hash = observation.block_hash.clone();
        let fields = BTreeMap::from([
            ("transaction_hash".to_string(), json!(transaction_hash)),
            ("block_height".to_string(), json!(observation.block_height)),
            ("block_hash".to_string(), json!(block_hash)),
            (
                "execution_stage".to_string(),
                json!(observation.execution_stage),
            ),
            ("vm_state".to_string(), json!(observation.vm_state)),
            (
                "exception_category".to_string(),
                json!(observation.exception_category),
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
            Some(observation.transaction_hash.clone()),
            observation.block_height,
            Some(observation.block_hash.clone()),
            self.inner.config.rule_version.clone(),
        )
        .ok()
    }

    pub fn try_submit_record(&self, record: EvidenceRecord) -> EvidenceSubmitResult {
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
                observed_height: self.inner.host.chain.current_block_index(),
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
        if self
            .inner
            .host
            .mempool
            .is_quarantined(&transaction_hash, now)
        {
            // The transaction is already actively contained: repeated
            // deterministic findings stay idempotent instead of failing
            // with the mempool's AlreadyActive error.
            return true;
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
        self.inner.host.mempool.quarantine(metadata, now).is_ok()
    }

    fn release_from_local_finding(&self, target: &str) -> bool {
        self.resolve_target_hash(target)
            .is_some_and(|transaction_hash| self.inner.host.mempool.release(&transaction_hash))
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
        config: &ProbeSessionConfig,
    ) -> Option<ProbeJob> {
        let transaction_hash = candidate.transaction_hash.clone()?;
        let transaction = self
            .inner
            .host
            .chain
            .unsigned_transaction(&transaction_hash, candidate.observed_height)?;
        let height = self.inner.host.chain.current_block_index();
        let tip_hash = self
            .inner
            .host
            .chain
            .block_hash_at(height)
            .map(|hash| hash.canonical)?;
        let now = unix_time_ms();
        let fingerprint = hex::encode(Sha256::digest(candidate.id.as_bytes()).as_slice());
        let operation_id =
            OperationId::new(format!("probe-candidate-{}", &fingerprint[..24])).ok()?;
        let run_id = RunId::new(format!("probe-run-{}", &fingerprint[24..40])).ok()?;
        let spec = PROBE_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == ProbeAction::TransactionPreflight)?;
        // The budget is clamped to the fixed probe contract catalog so the
        // startup-validated configuration can never exceed it.
        let budget = ProbeBudget {
            max_duration_ms: config
                .replay_timeout_ms
                .min(spec.maximum_budget.max_duration_ms),
            max_output_bytes: u64::try_from(config.frame_limit_bytes)
                .unwrap_or(0)
                .min(spec.maximum_budget.max_output_bytes),
            max_observations: spec.maximum_budget.max_observations,
        };
        if !valid_hash(&config.worker_binary_hash) {
            return None;
        }
        ProbeJob::new(
            operation_id,
            run_id,
            ProbeAction::TransactionPreflight,
            ProbeTransaction {
                transaction_hash: transaction.hash_hex,
                transaction_bytes: transaction.bytes,
            },
            ProbeContext {
                observed_height: height,
                tip_hash,
                requested_at_unix_ms: now,
                node_instance_id: "neo-node-local-diagnostics".to_owned(),
            },
            ProbeBuild {
                binary_hash: config.worker_binary_hash.clone(),
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

    /// Offers a prepared probe job to the controller's bounded submission
    /// queue; every bounded rejection is recorded instead of panicking.
    fn submit_probe_candidate_job(&self, job: ProbeJob) {
        self.inner
            .probe_submission_attempts
            .fetch_add(1, Ordering::Relaxed);
        self.inner.probe_submission_log.lock().push(job.clone());
        let Some(control) = &self.inner.probe_controller else {
            // No controller exists behind the fail-closed gates: the prepared
            // job is recorded as a skip and dropped.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        if control.try_submit(job).is_err() {
            // A full queue, admission cooldown, or closed controller is an
            // expected bounded outcome: record it and keep the worker alive.
            self.inner
                .probe_submission_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Accepts only the controller's completed, parent-validated double-replay result.
    pub async fn apply_probe_completion(&self, completion: ProbeCompletion) -> ActionResult {
        let ProbeCompletion {
            operation_id,
            run_id,
            result,
        } = completion;
        let Some(result) = result else {
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
        if !valid_hash(
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
        let current_height = self.inner.host.chain.current_block_index();
        if record.observed_at_unix_ms > now
            || now.saturating_sub(record.observed_at_unix_ms) > self.inner.config.event_freshness_ms
            || record.tip_height != current_height
            || record.observed_height != current_height
            || record.tip_hash.as_deref()
                != self
                    .inner
                    .host
                    .chain
                    .block_hash_at(current_height)
                    .map(|hash| hash.canonical)
                    .as_deref()
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        Ok(())
    }

    pub async fn apply_action(&self, request: OperationalActionRequest) -> ActionResult {
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

    // Node integration tests exercise this legacy internal contract cross-crate.
    #[doc(hidden)]
    #[allow(clippy::result_unit_err)]
    pub fn durable_action_binding(
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
        let hash = hex::encode(Sha256::digest(bytes).as_slice());
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
        if !valid_hash(context_hash)
            || snapshot_height != record.observed_height
            || snapshot_height != record.tip_height
            || record.tip_hash.as_deref() != Some(snapshot_root)
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let current_height = self.inner.host.chain.current_block_index();
        if current_height != snapshot_height
            || self
                .inner
                .host
                .chain
                .block_hash_at(current_height)
                .is_none_or(|hash| hash.canonical != snapshot_root)
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
        let current_height = self.inner.host.chain.current_block_index();
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
                .host
                .chain
                .block_hash_at(record.tip_height)
                .is_none_or(|hash| {
                    // Generic ingress records carry the `0x`-prefixed display
                    // form, while probe rule findings carry the canonical
                    // unprefixed 64-hex form; both must match the same block.
                    hash.display != expected_tip_hash && hash.canonical != expected_tip_hash
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
    fn resolve_target_hash(&self, target: &str) -> Option<String> {
        self.inner
            .host
            .mempool
            .resolve_target(target, unix_time_ms())
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
            .host
            .mempool
            .quarantine(metadata, now)
            .map_or(ActionResult::Failed, |()| ActionResult::Executed)
    }

    fn release(&self, request: &OperationalActionRequest) -> ActionResult {
        let Some(target) = request.target.as_deref() else {
            return ActionResult::InvalidTarget;
        };
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return ActionResult::InvalidTarget;
        };
        self.inner.host.mempool.release(&transaction_hash);
        ActionResult::Executed
    }

    #[doc(hidden)]
    pub fn request_worker_restart(&self) {
        if self.inner.config.enabled {
            self.inner.worker_generation.fetch_add(1, Ordering::AcqRel);
            self.inner.worker_restart.notify_one();
        }
    }

    fn start_worker_supervisor(
        &self,
        mut receiver: EvidenceReceiver,
        mut execution_receiver: mpsc::Receiver<ExecutionObservation>,
        mut probe_completions: Option<mpsc::Receiver<ProbeCompletion>>,
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
                    WorkerInput::ProbeCompletion(completion) => {
                        // Real production consumption path: the controller's
                        // bounded non-blocking completion stream reaches the
                        // trusted result policy here, not only from tests.
                        let _ = runtime.apply_probe_completion(completion).await;
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
    ) -> Result<(), crate::AuditError> {
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
                    .host
                    .mempool
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
                        let height = inner.host.chain.current_block_index();
                        let previous_height = inner
                            .last_observed_local_height
                            .swap(u64::from(height), Ordering::AcqRel);
                        if u64::from(height) > previous_height {
                            advance_last_progress(&inner.last_progress_unix_ms, observed_at);
                        }
                        if let Some(peers) = inner.host.chain.peer_observation().await {
                            inner
                                .observed_peer_count
                                .store(peers.connected_peers, Ordering::Release);
                            inner
                                .observed_peer_height
                                .store(peers.max_peer_height, Ordering::Release);
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
    ) -> Result<(), crate::AuditError> {
        self.inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: if executed {
                    AuditEventKind::OperationalActionExecuted
                } else {
                    AuditEventKind::OperationalActionEvaluated
                },
                observed_height: self.inner.host.chain.current_block_index(),
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

    #[doc(hidden)]
    pub async fn audit_snapshot(&self) -> Vec<crate::AuditEvent> {
        self.inner.audit.snapshot().await.unwrap_or_default()
    }

    #[doc(hidden)]
    pub fn transaction_finding(&self, hash: &str) -> Option<EvidenceRecord> {
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

    #[doc(hidden)]
    pub fn execution_observation(&self, hash: &str) -> Option<EvidenceRecord> {
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
