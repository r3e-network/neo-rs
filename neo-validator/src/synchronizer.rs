//! Synchronous two-upstream validation state machine.

use crate::audit::AuditLog;
use crate::executor::{BlockExecutor, ExecutionOutcome};
use crate::state::{GateRegistry, ValidationPhase, VerifiedPoint};
use crate::upstream::{BlockEvidence, NodeKind, NodeRpc, validate_protocol_identity};
use anyhow::{Context, Result, bail};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Block;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopOutcome {
    Waiting,
    Verified(u32),
    Quarantined,
}

#[derive(Debug)]
struct RecordedIncidentError(String);

impl std::fmt::Display for RecordedIncidentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordedIncidentError {}

pub struct Synchronizer {
    neo_cli: Arc<dyn NodeRpc>,
    neo_go: Arc<dyn NodeRpc>,
    executor: Arc<dyn BlockExecutor>,
    gates: GateRegistry,
    audit: AuditLog,
    settings: ProtocolSettings,
    poll_interval: Duration,
    max_catch_up_blocks: u32,
}

impl Synchronizer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        neo_cli: Arc<dyn NodeRpc>,
        neo_go: Arc<dyn NodeRpc>,
        executor: Arc<dyn BlockExecutor>,
        gates: GateRegistry,
        audit: AuditLog,
        settings: ProtocolSettings,
        poll_interval: Duration,
        max_catch_up_blocks: u32,
    ) -> Self {
        Self {
            neo_cli,
            neo_go,
            executor,
            gates,
            audit,
            settings,
            poll_interval,
            max_catch_up_blocks,
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        for node in [&self.neo_cli, &self.neo_go] {
            let identity = node
                .protocol_identity()
                .await
                .with_context(|| format!("{} getversion failed", node.kind().as_str()))?;
            validate_protocol_identity(&identity, &self.settings).with_context(|| {
                format!(
                    "{} protocol configuration is incompatible",
                    node.kind().as_str()
                )
            })?;
            self.gates.mark_upstream(node.kind(), true, None);
        }

        if let Some(checkpoint) = self.executor.checkpoint()? {
            let (cli, go) = self.fetch_pair(checkpoint.height).await?;
            compare_remote(checkpoint.height, &cli, &go)?;
            if cli.block_hash != checkpoint.block_hash {
                self.fail(
                    ValidationPhase::Mismatch,
                    checkpoint.height,
                    "restart_block_mismatch",
                    "persisted block hash differs from both upstreams",
                )?;
            }
            if cli.state_root != checkpoint.state_root {
                self.fail(
                    ValidationPhase::Mismatch,
                    checkpoint.height,
                    "restart_state_root_mismatch",
                    "persisted state root differs from both upstreams",
                )?;
            }
            self.gates.mark_verified(checkpoint);
        }
        Ok(())
    }

    /// Runs after [`Self::initialize`] has validated identities and any restart baseline.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        loop {
            if matches!(
                self.gates.status().phase,
                ValidationPhase::Mismatch
                    | ValidationPhase::CapabilityError
                    | ValidationPhase::ExecutorError
            ) {
                tokio::time::sleep(self.poll_interval).await;
                continue;
            }
            if let Err(error) = self.run_once().await {
                if error.downcast_ref::<RecordedIncidentError>().is_some() {
                    // The incident is durably audited and both gateways are closed.
                    // Keep the admin API alive for an explicit, authenticated recovery.
                    tokio::time::sleep(self.poll_interval).await;
                    continue;
                }
                return Err(error);
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    pub async fn run_once(&self) -> Result<LoopOutcome> {
        if matches!(
            self.gates.status().phase,
            ValidationPhase::Mismatch
                | ValidationPhase::CapabilityError
                | ValidationPhase::ExecutorError
        ) {
            return Ok(LoopOutcome::Quarantined);
        }
        self.gates.begin_verifying();
        let cli_count = match self.neo_cli.block_count().await {
            Ok(count) => {
                self.gates.mark_upstream(NodeKind::NeoCli, true, None);
                count
            }
            Err(error) => {
                self.unavailable(NodeKind::NeoCli, &error);
                return Ok(LoopOutcome::Waiting);
            }
        };
        let go_count = match self.neo_go.block_count().await {
            Ok(count) => {
                self.gates.mark_upstream(NodeKind::NeoGo, true, None);
                count
            }
            Err(error) => {
                self.unavailable(NodeKind::NeoGo, &error);
                return Ok(LoopOutcome::Waiting);
            }
        };
        if cli_count == 0 || go_count == 0 {
            self.fail(
                ValidationPhase::CapabilityError,
                0,
                "empty_upstream",
                "an upstream returned block count zero",
            )?;
        }
        let common_tip = cli_count.min(go_count) - 1;
        let local_height = self.executor.height()?;
        if local_height.is_some_and(|height| height > common_tip) {
            self.fail(
                ValidationPhase::Mismatch,
                local_height.unwrap_or(0),
                "local_height_above_common_tip",
                "validator storage is ahead of the common upstream chain",
            )?;
        }
        let next = local_height.map_or(0, |height| height + 1);
        if next > common_tip {
            let point = self
                .executor
                .checkpoint()?
                .ok_or_else(|| anyhow::anyhow!("caught-up validator has no checkpoint"))?;
            self.gates.mark_verified(point);
            return Ok(LoopOutcome::Waiting);
        }
        let target = common_tip.min(next + self.max_catch_up_blocks - 1);
        for height in next..=target {
            if !self.verify_height(height).await? {
                return Ok(LoopOutcome::Waiting);
            }
        }
        Ok(LoopOutcome::Verified(target))
    }

    async fn verify_height(&self, height: u32) -> Result<bool> {
        let (cli, go) = match self.fetch_pair(height).await {
            Ok(pair) => pair,
            Err(error) if is_transport_error(&error) => {
                let kind = if format!("{error:#}").contains("neo_go") {
                    NodeKind::NeoGo
                } else {
                    NodeKind::NeoCli
                };
                self.unavailable(kind, &error);
                return Ok(false);
            }
            Err(error) => {
                self.fail(
                    ValidationPhase::CapabilityError,
                    height,
                    "evidence_unavailable",
                    error.to_string(),
                )?;
                unreachable!()
            }
        };
        let (mut block, remote_root, remote_receipts) = match compare_remote(height, &cli, &go) {
            Ok(value) => value,
            Err(error) => {
                self.fail(
                    ValidationPhase::Mismatch,
                    height,
                    "upstream_mismatch",
                    error.to_string(),
                )?;
                unreachable!()
            }
        };
        if block.hash() != cli.block_hash {
            self.fail(
                ValidationPhase::Mismatch,
                height,
                "decoded_block_hash_mismatch",
                "decoded block hash differs from getblockhash",
            )?;
        }

        let executor = self.executor.clone();
        let execution_block = block.clone();
        let outcome =
            match tokio::task::spawn_blocking(move || executor.execute(execution_block)).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(error)) => {
                    self.fail(
                        ValidationPhase::ExecutorError,
                        height,
                        "execution_failed",
                        error.to_string(),
                    )?;
                    unreachable!()
                }
                Err(error) => {
                    self.fail(
                        ValidationPhase::ExecutorError,
                        height,
                        "execution_task_failed",
                        error.to_string(),
                    )?;
                    unreachable!()
                }
            };
        self.compare_execution(height, &cli, remote_root, &remote_receipts, outcome)?;
        Ok(true)
    }

    fn compare_execution(
        &self,
        height: u32,
        cli: &BlockEvidence,
        remote_root: UInt256,
        remote_receipts: &[crate::upstream::ExecutionReceipt],
        outcome: ExecutionOutcome,
    ) -> Result<()> {
        if outcome.block_hash != cli.block_hash {
            self.fail(
                ValidationPhase::Mismatch,
                height,
                "local_block_hash_mismatch",
                "neo-rs produced a different block hash",
            )?;
        }
        if outcome.state_root != remote_root {
            self.fail(
                ValidationPhase::Mismatch,
                height,
                "state_root_mismatch",
                format!(
                    "neo-rs root {} differs from upstream root {}",
                    outcome.state_root, remote_root
                ),
            )?;
        }
        if let Some(local_receipts) = outcome.executions
            && local_receipts != remote_receipts
        {
            self.fail(
                ValidationPhase::Mismatch,
                height,
                "execution_receipt_mismatch",
                "neo-rs trigger, VM state, exception presence, or GAS differs from both upstreams",
            )?;
        }
        self.gates.mark_verified(VerifiedPoint {
            height,
            block_hash: outcome.block_hash,
            state_root: outcome.state_root,
        });
        Ok(())
    }

    async fn fetch_pair(&self, height: u32) -> Result<(BlockEvidence, BlockEvidence)> {
        // Evidence acquisition is independent, but comparison and local execution
        // remain strictly ordered by height.
        let (cli, go) = tokio::join!(
            self.neo_cli.block_evidence(height),
            self.neo_go.block_evidence(height)
        );
        let cli = cli.context("neo_cli evidence fetch failed")?;
        let go = go.context("neo_go evidence fetch failed")?;
        Ok((cli, go))
    }

    fn unavailable(&self, kind: NodeKind, error: &anyhow::Error) {
        self.gates
            .mark_upstream(kind, false, Some(error.to_string()));
    }

    fn fail(
        &self,
        phase: ValidationPhase,
        height: u32,
        code: &str,
        detail: impl Into<String>,
    ) -> Result<()> {
        let incident = self.gates.quarantine(phase, height, code, detail);
        tracing::error!(
            target: "neo_validator",
            height,
            code,
            incident_id = %incident.id,
            "validation failed; both RPC gateways are quarantined"
        );
        self.audit.append("validation_incident", &incident)?;
        Err(RecordedIncidentError(format!(
            "validation incident {}: {}",
            incident.code, incident.detail
        ))
        .into())
    }
}

fn compare_remote(
    height: u32,
    cli: &BlockEvidence,
    go: &BlockEvidence,
) -> Result<(Block, UInt256, Vec<crate::upstream::ExecutionReceipt>)> {
    if cli.raw_block != go.raw_block {
        bail!("neo-cli and neo-go raw block bytes differ");
    }
    if cli.block_hash != go.block_hash {
        bail!("neo-cli and neo-go block hashes differ");
    }
    if cli.state_root != go.state_root {
        bail!("neo-cli and neo-go state roots differ");
    }
    if cli.executions != go.executions {
        bail!("neo-cli and neo-go execution receipts differ");
    }
    let mut reader = MemoryReader::new(&cli.raw_block);
    let block = Block::deserialize(&mut reader).context("strict block deserialization failed")?;
    if reader.remaining() != 0 || block.index() != height {
        bail!("block has trailing bytes or a mismatched height");
    }
    Ok((block, cli.state_root, cli.executions.clone()))
}

fn is_transport_error(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}");
    text.contains("HTTP request failed")
        || text.contains("reading upstream response")
        || text.contains("upstream returned HTTP")
}
