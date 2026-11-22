use super::*;

use super::evidence::{ExecutionRepetition, ValidationFailure};
#[doc(hidden)]
#[derive(Default)]
pub struct WorkerState {
    pub task: Option<JoinHandle<()>>,
    pub sampler: Option<JoinHandle<()>>,
}
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
    pub(super) execution_repetitions: Mutex<BTreeMap<String, ExecutionRepetition>>,
    pub(super) validation_failures: Mutex<BTreeMap<String, ValidationFailure>>,
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
pub(super) struct SharedModelProvider(pub(super) Arc<NodeModelProvider>);

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

    pub(super) fn execution_observation_record(
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
