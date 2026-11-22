use super::*;
use neo_ai_agent::{
    AuthorizerMetadata, EvidenceAuthority, OperationId, ProbeBudget, ProbeBuild, ProbeContext,
    ProbeTransaction, RunId,
};
use neo_primitives::{UInt160, WitnessScope};
use neo_protocol::payloads::Signer;
use neo_protocol::witness::Witness;
use neo_vm::{OpCode, interop_hash};
use std::io::Cursor;

fn sha256(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes).as_slice())
}

fn job() -> ProbeJob {
    let transaction_bytes = b"task-29-transaction".to_vec();
    ProbeJob::new(
        OperationId::new("operation-29").unwrap(),
        RunId::new("run-1").unwrap(),
        ProbeAction::TransactionPreflight,
        ProbeTransaction {
            transaction_hash: sha256(&transaction_bytes),
            transaction_bytes,
        },
        ProbeContext {
            observed_height: 29,
            tip_hash: "ab".repeat(32),
            requested_at_unix_ms: 1_000,
            node_instance_id: "probe-test-node".to_owned(),
        },
        ProbeBuild {
            binary_hash: "bc".repeat(32),
            version: "neo-probe-0.21.0".to_owned(),
        },
        ProbeBudget {
            max_duration_ms: 1_000,
            max_output_bytes: 4_096,
            max_observations: 4,
        },
        AuthorizerMetadata {
            evidence_id: "task-29-rule".to_owned(),
            evidence_hash: "cd".repeat(32),
            authority: EvidenceAuthority::DeterministicRule,
            rule_version: "task-29-v1".to_owned(),
            authorized_at_unix_ms: 999,
        },
    )
    .unwrap()
}

fn snapshot_entry() -> SnapshotEntry {
    SnapshotEntry {
        key: StorageKey::new(29, vec![1, 2, 3]).to_array(),
        value: vec![4, 5, 6],
        constant: false,
    }
}

fn request() -> ProbeRequestFrame {
    let job = job();
    let snapshot = SnapshotInput::from_job(&job, vec![snapshot_entry()]);
    ProbeRequestFrame::new(job, snapshot).unwrap()
}

#[test]
fn framed_worker_round_trip_is_single_request_single_response() {
    let input = encode_frame(&request()).unwrap();
    let mut reader = Cursor::new(input);
    let mut output = Vec::new();
    run_worker_once(&mut reader, &mut output).unwrap();
    let outcome = decode_frame::<ProbeOutcome>(&output).unwrap();
    assert_eq!(outcome.status, ProbeOutcomeStatus::Succeeded);
    assert_eq!(outcome.job_hash, request().job.binding_hash().unwrap());
    assert_eq!(outcome.reported_authority, EvidenceAuthority::Observation);
}

#[test]
fn oversized_and_zero_frames_are_rejected_before_allocation() {
    let oversized = u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_be_bytes();
    assert!(matches!(
        decode_frame::<ProbeRequestFrame>(&oversized),
        Err(WorkerError::FrameTooLarge)
    ));
    assert!(matches!(
        decode_frame::<ProbeRequestFrame>(&0u32.to_be_bytes()),
        Err(WorkerError::ZeroLengthFrame)
    ));
}

#[test]
fn trailing_payload_bytes_are_rejected() {
    let mut payload = serde_json::to_vec(&request()).unwrap();
    payload.push(b' ');
    let mut frame = u32::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(&payload);
    assert!(matches!(
        decode_frame::<ProbeRequestFrame>(&frame),
        Err(WorkerError::TrailingBytes)
    ));
}

#[test]
fn unknown_fields_are_rejected_at_every_protocol_boundary() {
    let mut value = serde_json::to_value(request()).unwrap();
    value["open_path"] = json!("forbidden");
    let payload = serde_json::to_vec(&value).unwrap();
    let mut frame = u32::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(&payload);
    assert!(matches!(
        decode_frame::<ProbeRequestFrame>(&frame),
        Err(WorkerError::Serialization(_))
    ));
}

#[test]
fn transaction_hash_mismatch_is_rejected() {
    let mut request = request();
    request.job.transaction.transaction_bytes.push(0xff);
    assert!(matches!(
        execute_request(&request),
        Err(WorkerError::Contract(
            ProbeContractError::InvalidTransaction
        ))
    ));
}

#[test]
fn context_and_snapshot_mismatch_are_rejected() {
    let mut binding_mismatch = request();
    binding_mismatch.job.context.node_instance_id = "other-node".to_owned();
    assert!(matches!(
        execute_request(&binding_mismatch),
        Err(WorkerError::BindingMismatch("context"))
    ));

    let mut snapshot_mismatch = request();
    snapshot_mismatch.snapshot.height += 1;
    assert!(matches!(
        execute_request(&snapshot_mismatch),
        Err(WorkerError::SnapshotHeightMismatch)
    ));
}

#[test]
fn zero_and_mismatched_explicit_bindings_are_rejected() {
    let mut zero = request();
    zero.bindings.build = "00".repeat(32);
    assert!(matches!(
        execute_request(&zero),
        Err(WorkerError::BindingMismatch("build"))
    ));
    let mut mismatch = request();
    mismatch.bindings.budget = "ef".repeat(32);
    assert!(matches!(
        execute_request(&mismatch),
        Err(WorkerError::BindingMismatch("budget"))
    ));
}

#[test]
fn admitted_but_too_small_output_budget_is_rejected() {
    let mut job = job();
    job.budget.max_output_bytes = 1;
    let snapshot = SnapshotInput::from_job(&job, vec![snapshot_entry()]);
    let request = ProbeRequestFrame::new(job, snapshot).unwrap();
    assert!(matches!(
        execute_request(&request),
        Err(WorkerError::BudgetViolation)
    ));
}

#[test]
fn disposable_overlay_never_mutates_detached_source() {
    let entry = snapshot_entry();
    let detached = DetachedSnapshot::try_from_entries(
        [DetachedSnapshotEntry {
            key: entry.key.clone(),
            value: entry.value.clone(),
            constant: entry.constant,
        }],
        DetachedSnapshotLimits {
            max_entries: 1,
            max_bytes: 64,
        },
    )
    .unwrap();
    let before = detached.get(&entry.key);
    let overlay = detached.disposable_overlay();
    overlay
        .upsert(
            StorageKey::from_bytes(&entry.key),
            StorageItem::from_bytes(vec![9, 9, 9]),
        )
        .unwrap();
    assert_eq!(overlay.modified_count(), 1);
    assert_eq!(detached.get(&entry.key), before);
    assert_eq!(detached.len(), 1);
}

#[test]
fn same_input_produces_byte_identical_content_bound_outcome() {
    let request = request();
    let first = execute_request(&request).unwrap();
    let second = execute_request(&request).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        encode_frame(&first).unwrap(),
        encode_frame(&second).unwrap()
    );

    let mut changed = request;
    changed.snapshot.entries[0].value.push(7);
    let changed_outcome = execute_request(&changed).unwrap();
    assert_ne!(
        first.content_hash().unwrap(),
        changed_outcome.content_hash().unwrap()
    );
}

// ------------------------------------------------------------------------
// Deterministic sandbox pre-execution (task 39)
// ------------------------------------------------------------------------

/// Serializes a minimal, structurally valid Neo transaction whose script is
/// `script`. One signer, one matching empty witness — enough for full wire
/// deserialization by the sandbox engine.
fn transaction_bytes(script: &[u8]) -> Vec<u8> {
    let mut tx = Transaction::new();
    tx.set_nonce(0x2A2A_2A2A);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1_000);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_script(script.to_vec());
    tx.set_witnesses(vec![Witness::empty()]);
    tx.try_to_bytes().unwrap()
}

fn preflight_request(transaction: Vec<u8>, budget: ProbeBudget) -> ProbeRequestFrame {
    let transaction_hash = sha256(&transaction);
    let mut job = job();
    job.transaction = ProbeTransaction {
        transaction_hash,
        transaction_bytes: transaction,
    };
    job.budget = budget;
    let snapshot = SnapshotInput::from_job(&job, vec![snapshot_entry()]);
    ProbeRequestFrame::new(job, snapshot).unwrap()
}

/// `System.Contract.GetCallFlags` is a host-path syscall (not part of the
/// external fast-VM runtime set), so this script forces the fork-aware
/// interpreter and the instruction-count diagnostic fires.
fn syscall_script() -> Vec<u8> {
    let mut script = vec![OpCode::SYSCALL.byte()];
    script.extend_from_slice(&interop_hash("System.Contract.GetCallFlags").to_le_bytes());
    script.push(OpCode::DROP.byte());
    script.push(OpCode::RET.byte());
    script
}

fn abort_script() -> Vec<u8> {
    vec![OpCode::ABORT.byte()]
}

/// A two-jump script that self-loops regardless of whether the active
/// interpreter resolves the jump offset relative to the instruction start
/// or to the end of the jump instruction — an infinite loop that can only
/// be stopped by the sandbox GAS cap.
fn infinite_loop_script() -> Vec<u8> {
    // JMP 0 (loops under start-relative semantics), JMP -2 (loops under
    // end-relative semantics).
    vec![OpCode::JMP.byte(), 0x00, OpCode::JMP.byte(), 0xFE]
}

fn preflight_observation(outcome: &ProbeOutcome) -> &serde_json::Value {
    outcome
        .observations
        .get("preflight")
        .expect("preflight observation is always present")
}

#[test]
fn successful_preexecution_records_metrics_and_result_fingerprint() {
    let request = preflight_request(transaction_bytes(&syscall_script()), job().budget);
    let outcome = execute_request(&request).unwrap();
    assert_eq!(outcome.status, ProbeOutcomeStatus::Succeeded);
    let preflight = preflight_observation(&outcome);
    assert_eq!(preflight["vm_state"], "HALT");
    assert_eq!(preflight["classification"], "completed");
    assert_eq!(preflight["accepted"], true);
    assert!(preflight["gas_consumed_datoshi"].as_i64().unwrap() > 0);
    assert!(preflight["instructions_executed"].as_u64().unwrap() > 0);
    assert!(preflight["memory_footprint_bytes"].as_u64().unwrap() > 0);
    assert!(valid_hash(
        preflight["result_fingerprint"].as_str().unwrap()
    ));
}

#[test]
fn deterministic_fault_records_fault_evidence() {
    let request = preflight_request(transaction_bytes(&abort_script()), job().budget);
    let first = execute_request(&request).unwrap();
    let second = execute_request(&request).unwrap();
    assert_eq!(first, second);
    let preflight = preflight_observation(&first);
    assert_eq!(preflight["vm_state"], "FAULT");
    assert_eq!(preflight["classification"], "deterministic_fault");
    assert_eq!(preflight["accepted"], false);
    assert_eq!(preflight["fault_category"], "abort");
    assert!(valid_hash(preflight["fault_fingerprint"].as_str().unwrap()));
    assert!(
        preflight["fault_message"]
            .as_str()
            .unwrap()
            .contains("ABORT")
    );
}

#[test]
fn budget_exhaustion_is_reported_as_uncertain() {
    let budget = job().budget;
    let request = preflight_request(transaction_bytes(&infinite_loop_script()), budget);
    let outcome = execute_request(&request).unwrap();
    // The worker itself completed; the uncertainty lives in the observation
    // so the controller can still validate and compare the outcome.
    assert_eq!(outcome.status, ProbeOutcomeStatus::Succeeded);
    let preflight = preflight_observation(&outcome);
    assert_eq!(preflight["classification"], "uncertain");
    assert_eq!(preflight["uncertain_reason"], "budget_exceeded");
    assert_eq!(preflight["vm_state"], "FAULT");
    assert_eq!(preflight["accepted"], false);
}

#[test]
fn result_fingerprint_is_stable_and_execution_sensitive() {
    let request = preflight_request(transaction_bytes(&syscall_script()), job().budget);
    let first = execute_request(&request).unwrap();
    let second = execute_request(&request).unwrap();
    assert_eq!(
        preflight_observation(&first)["result_fingerprint"],
        preflight_observation(&second)["result_fingerprint"]
    );

    let other = preflight_request(
        transaction_bytes(&[OpCode::PUSHINT8.byte(), 0x2A, OpCode::RET.byte()]),
        job().budget,
    );
    let other_outcome = execute_request(&other).unwrap();
    assert_eq!(preflight_observation(&other_outcome)["vm_state"], "HALT");
    assert_ne!(
        preflight_observation(&first)["result_fingerprint"],
        preflight_observation(&other_outcome)["result_fingerprint"]
    );
}
