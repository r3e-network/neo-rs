//! `NeoSystem` adapters for the reverse-injected AI diagnostics runtime.

use crate::probe_containment::NativeProbeContainment;
use crate::probe_controller::{
    ProbeContextRechecker, ProbeController, ProbeControllerConfig, ProbeControllerResult,
    ProbeVerdict, probe_context_matches, validate_probe_startup,
};
use async_trait::async_trait;
use neo_ai_agent::{
    ChainBlockHash, ChainHost, DiagnosticsSection, DurableActionStore, ExecutionObservation,
    ExecutionObservationHost, MempoolHost, PeerObservation, ProbeCompletion, ProbeControl,
    ProbeHost, ProbeJob, ProbeSession, ProbeSessionConfig, ProbeSubmissionError, QuarantineConfig,
    QuarantineMetadata, RuntimeHost, UnsignedTransaction,
};
use neo_io::BinaryWriter;
use neo_mempool::{
    QuarantineConfig as MempoolQuarantineConfig, QuarantineMetadata as MempoolQuarantineMetadata,
};
use neo_primitives::UInt256;
use neo_probe::{ProbeRequestFrame, SnapshotInput};
use neo_system::{NeoSystem, TransactionExecutionObservation};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::mpsc;

struct SystemAiAgentHost {
    system: Arc<NeoSystem>,
}

pub(crate) fn runtime_host(system: Arc<NeoSystem>) -> RuntimeHost {
    let host = Arc::new(SystemAiAgentHost { system });
    RuntimeHost {
        chain: host.clone(),
        mempool: host.clone(),
        execution: host.clone(),
        probe: host,
    }
}

fn block_hash(hash: UInt256) -> ChainBlockHash {
    ChainBlockHash {
        display: hash.to_string(),
        canonical: canonical_block_hash(&hash),
    }
}

#[async_trait]
impl ChainHost for SystemAiAgentHost {
    fn current_block_index(&self) -> u32 {
        self.system.current_block_index()
    }

    fn block_hash_at(&self, height: u32) -> Option<ChainBlockHash> {
        self.system.block_hash_at(height).map(block_hash)
    }

    async fn peer_observation(&self) -> Option<PeerObservation> {
        let peers = self.system.remote_node_snapshots().await.ok()?;
        Some(PeerObservation {
            connected_peers: u64::try_from(peers.len()).unwrap_or(u64::MAX),
            max_peer_height: peers
                .iter()
                .map(|peer| u64::from(peer.last_block_index))
                .max()
                .unwrap_or(0),
        })
    }

    fn unsigned_transaction(
        &self,
        transaction_hash: &str,
        observed_height: u32,
    ) -> Option<UnsignedTransaction> {
        let transaction_hash = UInt256::parse(transaction_hash).ok()?;
        let ledger = self.system.ledger_context();
        let transaction = ledger.get_transaction(&transaction_hash).or_else(|| {
            let block_hash = ledger.block_hash_at(observed_height)?;
            ledger
                .get_block(&block_hash)?
                .transactions
                .iter()
                .find(|transaction| {
                    transaction
                        .try_hash()
                        .is_ok_and(|hash| hash == transaction_hash)
                })
                .cloned()
        })?;
        let mut writer = BinaryWriter::new();
        transaction.serialize_unsigned(&mut writer).ok()?;
        Some(UnsignedTransaction {
            bytes: writer.into_bytes(),
            hash_hex: hex::encode(transaction_hash.to_array()),
        })
    }
}

impl MempoolHost for SystemAiAgentHost {
    fn configure(&self, config: QuarantineConfig, now_unix_ms: u64) -> Result<(), String> {
        self.system
            .mempool()
            .lock()
            .configure_quarantine(
                MempoolQuarantineConfig {
                    automatic_enabled: config.automatic_enabled,
                    max_entries: config.max_entries,
                    max_ttl_ms: config.max_ttl_ms,
                    selection_scan_budget: config.selection_scan_budget,
                },
                now_unix_ms,
            )
            .map_err(|error| error.to_string())
    }

    fn resolve_target(&self, target: &str, now_unix_ms: u64) -> Option<String> {
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
            let mempool = self.system.mempool();
            let mut pool = mempool.lock();
            if pool.contains_key(&natural) || pool.is_quarantined(&natural, now_unix_ms) {
                return Some(natural.to_string());
            }
        }
        Some(display.to_string())
    }

    fn is_quarantined(&self, transaction_hash: &str, now_unix_ms: u64) -> bool {
        UInt256::parse(transaction_hash).is_ok_and(|hash| {
            self.system
                .mempool()
                .lock()
                .is_quarantined(&hash, now_unix_ms)
        })
    }

    fn quarantine(&self, metadata: QuarantineMetadata, now_unix_ms: u64) -> Result<(), String> {
        let transaction_hash =
            UInt256::parse(&metadata.transaction_hash).map_err(|error| error.to_string())?;
        self.system
            .mempool()
            .lock()
            .quarantine_transaction(
                MempoolQuarantineMetadata {
                    transaction_hash,
                    evidence_refs: metadata.evidence_refs,
                    context: metadata.context,
                    created_at_unix_ms: metadata.created_at_unix_ms,
                    expires_at_unix_ms: metadata.expires_at_unix_ms,
                    rule_version: metadata.rule_version,
                    episode: metadata.episode,
                    cumulative_active_ms: metadata.cumulative_active_ms,
                },
                now_unix_ms,
            )
            .map_err(|error| error.to_string())
    }

    fn release(&self, transaction_hash: &str) -> bool {
        UInt256::parse(transaction_hash)
            .is_ok_and(|hash| self.system.mempool().lock().release_quarantine(&hash))
    }
}

impl ExecutionObservationHost for SystemAiAgentHost {
    fn register_sink(&self, sink: mpsc::Sender<ExecutionObservation>) -> Result<(), String> {
        let (system_sink, mut system_receiver) = mpsc::channel(sink.max_capacity());
        self.system
            .register_transaction_execution_sink(system_sink)
            .map_err(|_| "canonical execution sink is already registered".to_owned())?;
        tokio::spawn(async move {
            while let Some(observation) = system_receiver.recv().await {
                let observation = plain_execution_observation(observation);
                match sink.try_send(observation) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        });
        Ok(())
    }
}

fn plain_execution_observation(
    observation: TransactionExecutionObservation,
) -> ExecutionObservation {
    ExecutionObservation {
        transaction_hash: observation.transaction_hash.to_string(),
        block_height: observation.block_height,
        block_hash: observation.block_hash.to_string(),
        execution_stage: observation.execution_stage.as_str().to_owned(),
        vm_state: format!("{:?}", observation.vm_state).to_ascii_uppercase(),
        exception_category: observation.exception_category.as_str().to_owned(),
        gas_consumed: observation.gas_consumed,
        schema_version: observation.schema_version,
        rule_version: observation.rule_version,
        observed_at_unix_ms: observation.observed_at_unix_ms,
    }
}

struct SystemProbeControl {
    controller: ProbeController,
}

impl ProbeControl for SystemProbeControl {
    fn try_submit(&self, job: ProbeJob) -> Result<(), ProbeSubmissionError> {
        let snapshot = SnapshotInput::from_job(&job, Vec::new());
        let frame = ProbeRequestFrame::new(job, snapshot).map_err(|_| ProbeSubmissionError)?;
        self.controller
            .try_submit(frame)
            .map(|_| ())
            .map_err(|_| ProbeSubmissionError)
    }
}

impl ProbeHost for SystemAiAgentHost {
    fn start(
        &self,
        section: &DiagnosticsSection,
        action_store: Option<&Arc<DurableActionStore>>,
        completions: mpsc::Sender<ProbeCompletion>,
    ) -> anyhow::Result<Option<ProbeSession>> {
        if !section.restricted_probe_enabled {
            return Ok(None);
        }
        let action_store = action_store
            .ok_or_else(|| anyhow::anyhow!("restricted probe requires the durable action store"))?;
        let config = validate_probe_startup(section, &NativeProbeContainment)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_config = probe_session_config(&config)
            .ok_or_else(|| anyhow::anyhow!("restricted probe worker binary is unreadable"))?;
        let rechecker = SystemProbeContextRechecker {
            system: Arc::clone(&self.system),
        };
        let controller =
            ProbeController::start(config, Arc::clone(action_store), Arc::new(rechecker))
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let mut controller_completions = controller.take_completions().ok_or_else(|| {
            anyhow::anyhow!("restricted probe completion stream was already consumed")
        })?;
        tokio::spawn(async move {
            while let Some(result) = controller_completions.recv().await {
                let completion = plain_probe_completion(result);
                if completions.send(completion).await.is_err() {
                    break;
                }
            }
        });
        Ok(Some(ProbeSession {
            control: Arc::new(SystemProbeControl { controller }),
            config: session_config,
        }))
    }
}

fn plain_probe_completion(result: ProbeControllerResult) -> ProbeCompletion {
    let ProbeControllerResult {
        operation_id,
        run_id,
        verdict,
    } = result;
    let result = match verdict {
        ProbeVerdict::Completed(result) => Some(result),
        ProbeVerdict::Uncertain(_) => None,
    };
    ProbeCompletion {
        operation_id,
        run_id,
        result,
    }
}

pub(crate) fn probe_session_config(config: &ProbeControllerConfig) -> Option<ProbeSessionConfig> {
    let worker_binary_hash = std::fs::read(&config.executable).ok().map(|bytes| {
        Sha256::digest(&bytes)
            .iter()
            .fold(String::new(), |mut output, byte| {
                use std::fmt::Write as _;
                write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
                output
            })
    })?;
    Some(ProbeSessionConfig {
        replay_timeout_ms: u64::try_from(config.replay_timeout.as_millis()).unwrap_or(0),
        frame_limit_bytes: config.frame_limit_bytes,
        worker_binary_hash,
    })
}

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

/// Canonical unprefixed 64-character lowercase hex form of a block hash.
pub(crate) fn canonical_block_hash(hash: &UInt256) -> String {
    hash.as_ref()
        .iter()
        .rev()
        .fold(String::new(), |mut output, byte| {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("formatting hex into a String cannot fail");
            output
        })
}
