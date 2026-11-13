use anyhow::Result;
use async_trait::async_trait;
use neo_io::SerializableExtensions;
use neo_primitives::{Hardfork, UInt256};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Block;
use neo_validator::audit::AuditLog;
use neo_validator::executor::{BlockExecutor, ExecutionOutcome};
use neo_validator::state::{GateRegistry, ValidationPhase, VerifiedPoint};
use neo_validator::synchronizer::{LoopOutcome, Synchronizer};
use neo_validator::upstream::{BlockEvidence, ExecutionReceipt, NodeKind, NodeRpc};
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

struct MockNode {
    kind: NodeKind,
    count: RwLock<Result<u32, String>>,
    evidence: RwLock<Result<BlockEvidence, String>>,
}

impl MockNode {
    fn new(kind: NodeKind, height: u32) -> Arc<Self> {
        Arc::new(Self {
            kind,
            count: RwLock::new(Ok(height + 1)),
            evidence: RwLock::new(Ok(evidence(height))),
        })
    }

    fn set_evidence_error(&self, message: &str) {
        *self.evidence.write() = Err(message.to_owned());
    }
}

#[async_trait]
impl NodeRpc for MockNode {
    fn kind(&self) -> NodeKind {
        self.kind
    }

    async fn protocol_identity(&self) -> Result<Value> {
        Ok(identity(&ProtocolSettings::default()))
    }

    async fn block_count(&self) -> Result<u32> {
        self.count
            .read()
            .clone()
            .map_err(|error| anyhow::anyhow!(error))
    }

    async fn block_evidence(&self, _height: u32) -> Result<BlockEvidence> {
        self.evidence
            .read()
            .clone()
            .map_err(|error| anyhow::anyhow!(error))
    }
}

struct MockExecutor {
    checkpoint: RwLock<Option<VerifiedPoint>>,
    fail: RwLock<bool>,
    wrong_root: RwLock<bool>,
}

impl MockExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            checkpoint: RwLock::new(None),
            fail: RwLock::new(false),
            wrong_root: RwLock::new(false),
        })
    }
}

impl BlockExecutor for MockExecutor {
    fn height(&self) -> Result<Option<u32>> {
        Ok(self.checkpoint.read().as_ref().map(|point| point.height))
    }

    fn checkpoint(&self) -> Result<Option<VerifiedPoint>> {
        Ok(self.checkpoint.read().clone())
    }

    fn execute(&self, mut block: Block) -> Result<ExecutionOutcome> {
        if *self.fail.read() {
            anyhow::bail!("injected execution failure");
        }
        let height = block.index();
        let outcome = ExecutionOutcome {
            block_hash: block.hash(),
            state_root: if *self.wrong_root.read() {
                root(height + 100)
            } else {
                root(height)
            },
            executions: Some(Vec::new()),
        };
        *self.checkpoint.write() = Some(VerifiedPoint {
            height,
            block_hash: outcome.block_hash,
            state_root: outcome.state_root,
        });
        Ok(outcome)
    }
}

struct Fixture {
    sync: Synchronizer,
    gates: GateRegistry,
    cli: Arc<MockNode>,
    go: Arc<MockNode>,
    executor: Arc<MockExecutor>,
    _temp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let cli = MockNode::new(NodeKind::NeoCli, 0);
    let go = MockNode::new(NodeKind::NeoGo, 0);
    let executor = MockExecutor::new();
    let gates = GateRegistry::default();
    let temp = tempfile::TempDir::new().unwrap();
    let audit = AuditLog::open(&temp.path().join("audit.jsonl")).unwrap();
    let sync = Synchronizer::new(
        cli.clone(),
        go.clone(),
        executor.clone(),
        gates.clone(),
        audit,
        ProtocolSettings::default(),
        Duration::from_millis(50),
        16,
    );
    Fixture {
        sync,
        gates,
        cli,
        go,
        executor,
        _temp: temp,
    }
}

#[tokio::test]
async fn matching_block_executes_and_opens_both_gateways() {
    let fixture = fixture();
    assert_eq!(
        fixture.sync.run_once().await.unwrap(),
        LoopOutcome::Verified(0)
    );
    let status = fixture.gates.status();
    assert_eq!(status.phase, ValidationPhase::Healthy);
    assert_eq!(status.verified.unwrap().height, 0);
    assert!(fixture.gates.proxy_permit(NodeKind::NeoCli).is_ok());
    assert!(fixture.gates.proxy_permit(NodeKind::NeoGo).is_ok());
}

#[tokio::test]
async fn raw_block_fork_quarantines_both_upstreams() {
    let fixture = fixture();
    let mut changed = evidence(0);
    changed.raw_block[0] ^= 1;
    *fixture.go.evidence.write() = Ok(changed);
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(fixture.gates.status().phase, ValidationPhase::Mismatch);
    assert!(fixture.gates.proxy_permit(NodeKind::NeoCli).is_err());
    assert!(fixture.gates.proxy_permit(NodeKind::NeoGo).is_err());
    assert!(fixture.executor.checkpoint.read().is_none());
}

#[tokio::test]
async fn upstream_state_root_mismatch_is_not_executed() {
    let fixture = fixture();
    let mut changed = evidence(0);
    changed.state_root = root(99);
    *fixture.go.evidence.write() = Ok(changed);
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(fixture.gates.status().phase, ValidationPhase::Mismatch);
    assert!(fixture.executor.checkpoint.read().is_none());
}

#[tokio::test]
async fn local_state_root_mismatch_quarantines_after_real_execution_boundary() {
    let fixture = fixture();
    *fixture.executor.wrong_root.write() = true;
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(fixture.gates.status().phase, ValidationPhase::Mismatch);
    assert_eq!(fixture.executor.height().unwrap(), Some(0));
    assert!(fixture.gates.status().verified.is_none());
}

#[tokio::test]
async fn upstream_execution_receipt_mismatch_quarantines() {
    let fixture = fixture();
    let mut changed = evidence(0);
    changed.executions = vec![ExecutionReceipt {
        trigger: "Application".to_owned(),
        vm_state: "HALT".to_owned(),
        gas_consumed: 1,
        has_exception: false,
    }];
    *fixture.go.evidence.write() = Ok(changed);
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(fixture.gates.status().phase, ValidationPhase::Mismatch);
}

#[tokio::test]
async fn local_execution_failure_quarantines_both() {
    let fixture = fixture();
    *fixture.executor.fail.write() = true;
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(fixture.gates.status().phase, ValidationPhase::ExecutorError);
    assert!(fixture.gates.proxy_permit(NodeKind::NeoCli).is_err());
    assert!(fixture.gates.proxy_permit(NodeKind::NeoGo).is_err());
}

#[tokio::test]
async fn neo_go_timeout_only_marks_neo_go_unavailable() {
    let fixture = fixture();
    fixture
        .go
        .set_evidence_error("upstream HTTP request failed: timeout");
    assert_eq!(fixture.sync.run_once().await.unwrap(), LoopOutcome::Waiting);
    let status = fixture.gates.status();
    assert!(status.neo_cli.available);
    assert!(!status.neo_go.available);
    assert_ne!(status.phase, ValidationPhase::Mismatch);
}

#[tokio::test]
async fn missing_state_root_capability_quarantines_both() {
    let fixture = fixture();
    fixture
        .go
        .set_evidence_error("upstream getstateroot failed with JSON-RPC -32601: method not found");
    fixture.sync.run_once().await.unwrap_err();
    assert_eq!(
        fixture.gates.status().phase,
        ValidationPhase::CapabilityError
    );
    assert!(fixture.gates.proxy_permit(NodeKind::NeoCli).is_err());
    assert!(fixture.gates.proxy_permit(NodeKind::NeoGo).is_err());
}

#[tokio::test]
async fn recorded_incident_keeps_management_runtime_alive() {
    let fixture = fixture();
    fixture
        .go
        .set_evidence_error("missing getstateroot capability");
    let sync = Arc::new(fixture.sync);
    let running = sync.clone();
    let task = tokio::spawn(async move { running.run().await });
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(!task.is_finished(), "incident must not stop admin recovery");
    assert_eq!(
        fixture.gates.status().phase,
        ValidationPhase::CapabilityError
    );
    task.abort();
}

#[tokio::test]
async fn restart_baseline_is_rechecked_before_gateway_opens() {
    let first = fixture();
    first.sync.run_once().await.unwrap();
    let gates = GateRegistry::default();
    let temp = tempfile::TempDir::new().unwrap();
    let sync = Synchronizer::new(
        first.cli,
        first.go,
        first.executor,
        gates.clone(),
        AuditLog::open(&temp.path().join("audit.jsonl")).unwrap(),
        ProtocolSettings::default(),
        Duration::from_millis(50),
        16,
    );
    sync.initialize().await.unwrap();
    assert_eq!(gates.status().phase, ValidationPhase::Healthy);
    assert_eq!(gates.status().verified.unwrap().height, 0);
}

fn evidence(height: u32) -> BlockEvidence {
    let mut block = block(height);
    BlockEvidence {
        raw_block: block.to_array().unwrap(),
        block_hash: block.hash(),
        state_root: root(height),
        executions: Vec::new(),
    }
}

fn block(height: u32) -> Block {
    let mut block = Block::new();
    block.header.set_index(height);
    block.header.set_timestamp(u64::from(height) + 1);
    block
}

fn root(height: u32) -> UInt256 {
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&height.to_le_bytes());
    UInt256::from_bytes(&bytes).unwrap()
}

fn identity(settings: &ProtocolSettings) -> Value {
    let hardforks = Hardfork::ALL
        .iter()
        .filter_map(|fork| {
            settings
                .hardforks
                .get(fork)
                .map(|height| (fork.name().to_owned(), *height))
        })
        .collect::<BTreeMap<_, _>>();
    json!({"protocol": {
        "network": settings.network,
        "addressversion": settings.address_version,
        "millisecondsperblock": settings.milliseconds_per_block,
        "maxtransactionsperblock": settings.max_transactions_per_block,
        "maxvaliduntilblockincrement": settings.max_valid_until_block_increment,
        "maxtraceableblocks": settings.max_traceable_blocks,
        "validatorscount": settings.validators_count,
        "memorypoolmaxtransactions": settings.memory_pool_max_transactions,
        "initialgasdistribution": settings.initial_gas_distribution,
        "standbycommittee": [],
        "hardforks": hardforks,
    }})
}
