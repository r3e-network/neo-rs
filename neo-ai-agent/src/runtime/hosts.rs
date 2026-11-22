use super::*;
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
