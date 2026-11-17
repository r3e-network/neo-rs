//! Compatibility paths and real-system integration tests for AI diagnostics.

pub(crate) use neo_ai_agent::AiAgentRuntime;

#[cfg(test)]
pub(crate) use crate::ai_agent_host::{canonical_block_hash, probe_session_config, runtime_host};
#[cfg(test)]
pub(crate) use crate::probe_controller::ProbeControllerConfig;
#[cfg(test)]
pub(crate) use async_trait::async_trait;
#[cfg(test)]
pub(crate) use neo_ai_agent::{
    ActionBinding, ActionResult, ActionState, AdmitOutcome, AnthropicProvider, AuditEventKind,
    AuditInput, AuditLog, AuthorizerMetadata, DiagnosisValidator, DiagnosticAgent,
    DiagnosticsSection, DurableActionStore, EvidenceAuthority, EvidenceBundle, EvidenceExporter,
    EvidenceItem, EvidenceKind, EvidenceReceiver, EvidenceRecord, EvidenceSource,
    EvidenceSubmitResult, ExecutionObservation, ExportResult, FileAuditLog, HttpProviderConfig,
    MemoryAuditLog, ModelProvider, ModelRequest, ModelResponse, NodeModelProvider, OllamaProvider,
    OpenAiCompatibleProvider, OperationId, OperationalAction, OperationalActionRequest,
    OperationalDecision, OperationalPolicy, OperationalPolicyConfig, OperationalSafetyState,
    PROBE_ACTION_CATALOG, PROBE_DETERMINISTIC_RULE_VERSION, ProbeAction, ProbeBudget, ProbeBuild,
    ProbeCompletion, ProbeContext, ProbeDeterministicRule, ProbeJob, ProbeResult,
    ProbeRuleFindingKind, ProbeTransaction, ProviderError, ProviderKind, RunId, StallDetector,
    StallPolicy, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, WorkerEvent, WorkerInput, action_request,
    advance_last_progress, build_model_provider, receive_worker_event, unix_time_ms,
};
#[cfg(test)]
pub(crate) use neo_primitives::UInt256;
#[cfg(test)]
pub(crate) use neo_system::{
    NeoSystem, TransactionExecutionObservation as SystemTransactionExecutionObservation,
};
#[cfg(test)]
pub(crate) use parking_lot::Mutex;
#[cfg(test)]
pub(crate) use serde_json::{Value, json};
#[cfg(test)]
pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(test)]
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(test)]
pub(crate) use std::sync::{Arc, Weak};
#[cfg(test)]
pub(crate) use std::time::Duration;
#[cfg(test)]
pub(crate) use tokio::sync::{Notify, mpsc};
#[cfg(test)]
pub(crate) use tokio::task::JoinHandle;

#[cfg(test)]
pub(crate) trait AiAgentRuntimeNodeExt: Sized {
    fn new(section: &DiagnosticsSection, system: Arc<NeoSystem>) -> anyhow::Result<Self>;
    fn with_audit(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> anyhow::Result<Self>;
    fn initialize(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
        action_store: Option<Arc<DurableActionStore>>,
    ) -> anyhow::Result<Self>;
}

#[cfg(test)]
impl AiAgentRuntimeNodeExt for AiAgentRuntime {
    fn new(section: &DiagnosticsSection, system: Arc<NeoSystem>) -> anyhow::Result<Self> {
        Self::new_with_host(section, runtime_host(system))
    }

    fn with_audit(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> anyhow::Result<Self> {
        Self::initialize_with_host(section, runtime_host(system), audit, None)
    }

    fn initialize(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
        action_store: Option<Arc<DurableActionStore>>,
    ) -> anyhow::Result<Self> {
        Self::initialize_with_host(section, runtime_host(system), audit, action_store)
    }
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
    use neo_state_service::StateServiceSettings;
    use neo_storage::persistence::{DataCache, ReadOnlyStoreGeneric};
    use neo_storage::{StorageItem, StorageKey};
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
            exception_policy: neo_state_service::UnhandledExceptionPolicy::StopPlugin,
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

    fn system_execution_observation(
        system: &NeoSystem,
        transaction_hash: UInt256,
    ) -> SystemTransactionExecutionObservation {
        SystemTransactionExecutionObservation {
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

    fn execution_observation(
        system: &NeoSystem,
        transaction_hash: UInt256,
    ) -> ExecutionObservation {
        let observation = system_execution_observation(system, transaction_hash);
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
                .host
                .chain
                .block_hash_at(0)
                .map(|hash| hash.display),
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
    ) -> ProbeCompletion {
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
        ProbeCompletion {
            operation_id,
            run_id,
            result: Some(result),
        }
    }

    fn expected_probe_action_id(
        runtime: &AiAgentRuntime,
        controller_result: &ProbeCompletion,
    ) -> OperationId {
        let result = controller_result
            .result
            .as_ref()
            .expect("completed probe result expected");
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
                .try_send(ExecutionObservation {
                    transaction_hash: UInt256::from([0x60 + sequence; 32]).to_string(),
                    block_height: 0,
                    block_hash: UInt256::zero().to_string(),
                    execution_stage: TransactionExecutionStage::Application.as_str().to_owned(),
                    vm_state: "FAULT".to_owned(),
                    exception_category: TransactionExecutionExceptionCategory::VmFault
                        .as_str()
                        .to_owned(),
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
            .try_send(system_execution_observation(
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
        let observation = execution_observation(&system, transaction_hash);

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
            runtime.apply_probe_completion(controller_result).await,
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
        let uncertain = ProbeCompletion {
            operation_id: OperationId::new("task36-uncertain").unwrap(),
            run_id: RunId::new("task36-uncertain-run").unwrap(),
            result: None,
        };
        assert_eq!(
            runtime.apply_probe_completion(uncertain).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        let mut mismatched =
            successful_probe_controller_result(&system, None, 2, &config.rule_version);
        mismatched.operation_id = OperationId::new("task36-forged-outer-id").unwrap();
        assert_eq!(
            runtime.apply_probe_completion(mismatched).await,
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
            runtime.apply_probe_completion(stale).await,
            ActionResult::Rejected(OperationalDecision::InvalidEvidence)
        );
        let wrong_rule =
            successful_probe_controller_result(&system, None, 4, "forged-authorizer-rule-v1");
        assert_eq!(
            runtime.apply_probe_completion(wrong_rule).await,
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
            runtime.apply_probe_completion(controller_result).await,
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
            runtime.apply_probe_completion(controller_result).await,
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
        // The node adapter converts the persisted block's canonical observation
        // on a forwarding task. Let it arrive before injecting the controlled
        // pair so that it cannot interleave between their matching signatures.
        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 1).await;
        // Inject a startup-validated restricted probe configuration (with a
        // readable worker executable) as if `start_probe_controller` had
        // passed its fail-closed gates.
        let directory = tempfile::tempdir().expect("probe worker directory");
        let executable = directory.path().join("probe-worker");
        std::fs::write(&executable, b"probe-worker-bytes").expect("probe worker executable");
        let mut probe_config = ProbeControllerConfig::from(&config);
        probe_config.executable = executable;
        *runtime.inner.probe_config.lock() = probe_session_config(&probe_config);
        let observation = execution_observation(&system, transaction_hash);
        assert_eq!(
            runtime.try_export_execution_observation(&observation),
            EvidenceSubmitResult::Enqueued
        );
        wait_until(|| runtime.inner.received_events.load(Ordering::Relaxed) >= 2).await;
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
