use neo_ai_agent::{
    EvidenceAuthority, PROBE_SCHEMA_VERSION, ProbeAction, ProbeContractError, ProbeJob,
    ProbeOutcome, ProbeOutcomeStatus,
};
use neo_native_contracts::native::composition::default_native_services;
use neo_primitives::{CallFlags, TriggerType, Verifiable};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Transaction;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::diagnostic::Diagnostic;
use neo_storage::persistence::data_cache::DataCache;
use neo_storage::persistence::{
    DetachedSnapshot, DetachedSnapshotEntry, DetachedSnapshotError, DetachedSnapshotLimits,
};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{ExecutionContext, Instruction, VmState};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thiserror::Error;

pub const MAX_FRAME_BYTES: usize = 512 * 1024;
const MAX_DETACHED_ENTRIES: usize = 4_096;
const MAX_DETACHED_BYTES: usize = 384 * 1024;
/// Deterministic sandbox GAS cap (in datoshi) granted per admitted millisecond
/// of the probe budget.
///
/// Wall-clock timing cannot be serialized into the outcome without breaking
/// the byte-identical replay contract (the controller compares two worker runs
/// for exact equality), so the duration budget is enforced through this
/// deterministic GAS cap instead.
const SANDBOX_GAS_PER_DURATION_MS: i64 = 10_000;
/// Maximum length of the fault message excerpt embedded in observations.
const FAULT_MESSAGE_EXCERPT_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBindings {
    pub transaction: String,
    pub context: String,
    pub build: String,
    pub budget: String,
}

impl ProbeBindings {
    pub fn calculate(job: &ProbeJob) -> Result<Self, WorkerError> {
        Ok(Self {
            transaction: job.transaction.content_hash()?,
            context: job.context.content_hash()?,
            build: job.build.content_hash()?,
            budget: job.budget.content_hash()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub constant: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotInput {
    pub root_hash: String,
    pub height: u32,
    pub entries: Vec<SnapshotEntry>,
}

impl SnapshotInput {
    #[must_use]
    pub fn from_job(job: &ProbeJob, entries: Vec<SnapshotEntry>) -> Self {
        Self {
            root_hash: job.context.tip_hash.clone(),
            height: job.context.observed_height,
            entries,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequestFrame {
    pub job: ProbeJob,
    pub bindings: ProbeBindings,
    pub snapshot: SnapshotInput,
}

impl ProbeRequestFrame {
    pub fn new(job: ProbeJob, snapshot: SnapshotInput) -> Result<Self, WorkerError> {
        let bindings = ProbeBindings::calculate(&job)?;
        Ok(Self {
            job,
            bindings,
            snapshot,
        })
    }
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("probe frame length must be non-zero")]
    ZeroLengthFrame,
    #[error("probe frame exceeds 512 KiB")]
    FrameTooLarge,
    #[error("probe frame is truncated or has trailing bytes")]
    FrameBoundary,
    #[error("probe JSON contains trailing bytes")]
    TrailingBytes,
    #[error("probe JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("probe I/O failed")]
    Io(#[from] std::io::Error),
    #[error("probe job is invalid")]
    Contract(#[from] ProbeContractError),
    #[error("probe {0} binding is zero or mismatched")]
    BindingMismatch(&'static str),
    #[error("only transaction preflight jobs are accepted")]
    UnsupportedAction,
    #[error("detached snapshot root is invalid or mismatched")]
    SnapshotRootMismatch,
    #[error("detached snapshot height is mismatched")]
    SnapshotHeightMismatch,
    #[error("detached snapshot is invalid")]
    Snapshot(#[from] DetachedSnapshotError),
    #[error("disposable overlay rejected the deterministic write")]
    Overlay,
    #[error("disposable overlay changed detached backing state")]
    IsolationViolation,
    #[error("probe outcome exceeds its admitted budget")]
    BudgetViolation,
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, WorkerError> {
    let payload = serde_json::to_vec(value)?;
    if payload.is_empty() {
        return Err(WorkerError::ZeroLengthFrame);
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(WorkerError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| WorkerError::FrameTooLarge)?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

pub fn decode_frame<T: DeserializeOwned>(framed: &[u8]) -> Result<T, WorkerError> {
    let header: [u8; 4] = framed
        .get(..4)
        .ok_or(WorkerError::FrameBoundary)?
        .try_into()
        .map_err(|_| WorkerError::FrameBoundary)?;
    let length =
        usize::try_from(u32::from_be_bytes(header)).map_err(|_| WorkerError::FrameTooLarge)?;
    validate_frame_length(length)?;
    if framed.len() != 4 + length {
        return Err(WorkerError::FrameBoundary);
    }
    decode_payload(&framed[4..])
}

pub fn run_worker_once<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> Result<(), WorkerError> {
    let payload = read_frame(reader)?;
    let request = decode_payload::<ProbeRequestFrame>(&payload)?;
    let outcome = execute_request(&request)?;
    write_frame(writer, &outcome)
}

pub fn execute_request(request: &ProbeRequestFrame) -> Result<ProbeOutcome, WorkerError> {
    request.job.validate()?;
    if request.job.action != ProbeAction::TransactionPreflight {
        return Err(WorkerError::UnsupportedAction);
    }

    let recalculated = ProbeBindings::calculate(&request.job)?;
    verify_binding(
        "transaction",
        &request.bindings.transaction,
        &recalculated.transaction,
    )?;
    verify_binding("context", &request.bindings.context, &recalculated.context)?;
    verify_binding("build", &request.bindings.build, &recalculated.build)?;
    verify_binding("budget", &request.bindings.budget, &recalculated.budget)?;

    if !valid_hash(&request.snapshot.root_hash)
        || !request
            .snapshot
            .root_hash
            .eq_ignore_ascii_case(&request.job.context.tip_hash)
    {
        return Err(WorkerError::SnapshotRootMismatch);
    }
    if request.snapshot.height != request.job.context.observed_height {
        return Err(WorkerError::SnapshotHeightMismatch);
    }

    let detached_entries = request
        .snapshot
        .entries
        .iter()
        .map(|entry| DetachedSnapshotEntry {
            key: entry.key.clone(),
            value: entry.value.clone(),
            constant: entry.constant,
        })
        .collect::<Vec<_>>();
    let snapshot = DetachedSnapshot::try_from_entries(
        detached_entries,
        DetachedSnapshotLimits {
            max_entries: MAX_DETACHED_ENTRIES,
            max_bytes: MAX_DETACHED_BYTES,
        },
    )?;
    let snapshot_entries_hash = snapshot_entries_hash(&request.snapshot.entries);

    let overlay = snapshot.disposable_overlay();
    let transaction_digest = Sha256::digest(&request.job.transaction.transaction_bytes);
    let mut marker_suffix = Vec::with_capacity(1 + transaction_digest.len());
    marker_suffix.push(0x29);
    marker_suffix.extend_from_slice(&transaction_digest);
    let marker_key = StorageKey::new(i32::MIN, marker_suffix);
    let marker_bytes = marker_key.to_array();
    let detached_before = snapshot.get(&marker_bytes);
    let marker_value = StorageItem::from_bytes(snapshot_entries_hash.as_bytes().to_vec());
    overlay
        .upsert(marker_key, marker_value)
        .map_err(|_| WorkerError::Overlay)?;
    if snapshot.get(&marker_bytes) != detached_before {
        return Err(WorkerError::IsolationViolation);
    }

    let job_hash = request.job.binding_hash()?;
    let vm_report = sandbox_preexecute(&request.job, &request.snapshot.entries);
    let preflight_hash = hash_parts(&[
        job_hash.as_bytes(),
        recalculated.transaction.as_bytes(),
        recalculated.context.as_bytes(),
        recalculated.build.as_bytes(),
        recalculated.budget.as_bytes(),
        request.snapshot.root_hash.to_ascii_lowercase().as_bytes(),
        &request.snapshot.height.to_be_bytes(),
        snapshot_entries_hash.as_bytes(),
        &snapshot.len().to_be_bytes(),
        &snapshot.encoded_bytes().to_be_bytes(),
        &overlay.modified_count().to_be_bytes(),
    ]);
    let observations = BTreeMap::from([(
        "preflight".to_owned(),
        json!({
            "accepted": vm_report.accepted(),
            "transaction_hash": request.job.transaction.transaction_hash,
            "transaction_binding": recalculated.transaction,
            "context_binding": recalculated.context,
            "build_binding": recalculated.build,
            "budget_binding": recalculated.budget,
            "snapshot_root": request.snapshot.root_hash.to_ascii_lowercase(),
            "snapshot_height": request.snapshot.height,
            "snapshot_entries": snapshot.len(),
            "snapshot_bytes": snapshot.encoded_bytes(),
            "snapshot_entries_hash": snapshot_entries_hash,
            "overlay_modified_count": overlay.modified_count(),
            "preflight_hash": preflight_hash,
            // Deterministic sandbox pre-execution evidence (task 39). All fields
            // are optional JSON keys: old readers ignore unknown observation
            // members, and no shared protocol struct was modified.
            "vm_state": vm_report.vm_state,
            "classification": vm_report.classification,
            "uncertain_reason": vm_report.uncertain_reason,
            "fault_category": vm_report.fault_category,
            "fault_fingerprint": vm_report.fault_fingerprint,
            "fault_message": vm_report.fault_message,
            "instructions_executed": vm_report.instructions_executed,
            "gas_consumed_datoshi": vm_report.gas_consumed,
            "sandbox_gas_limit_datoshi": sandbox_gas_limit(request.job.budget.max_duration_ms),
            "memory_footprint_bytes": vm_report.memory_footprint_bytes,
            "notifications": vm_report.notifications,
            "logs": vm_report.logs,
            "result_stack_items": vm_report.result_stack_items,
            "result_fingerprint": vm_report.result_fingerprint,
        }),
    )]);
    if observations.len() > usize::from(request.job.budget.max_observations)
        || serde_json::to_vec(&observations)?.len()
            > usize::try_from(request.job.budget.max_output_bytes).unwrap_or(usize::MAX)
    {
        return Err(WorkerError::BudgetViolation);
    }

    let timestamp = request.job.context.requested_at_unix_ms;
    let outcome = ProbeOutcome {
        schema_version: PROBE_SCHEMA_VERSION,
        operation_id: request.job.operation_id.clone(),
        run_id: request.job.run_id.clone(),
        action: request.job.action,
        job_hash,
        reported_authority: EvidenceAuthority::Observation,
        status: ProbeOutcomeStatus::Succeeded,
        started_at_unix_ms: timestamp,
        completed_at_unix_ms: timestamp,
        observations,
    };
    outcome.content_hash()?;
    Ok(outcome)
}

fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, WorkerError> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let length =
        usize::try_from(u32::from_be_bytes(header)).map_err(|_| WorkerError::FrameTooLarge)?;
    validate_frame_length(length)?;
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), WorkerError> {
    let frame = encode_frame(value)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

fn validate_frame_length(length: usize) -> Result<(), WorkerError> {
    if length == 0 {
        return Err(WorkerError::ZeroLengthFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(WorkerError::FrameTooLarge);
    }
    Ok(())
}

fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, WorkerError> {
    let mut stream = serde_json::Deserializer::from_slice(payload).into_iter::<T>();
    let value = stream.next().ok_or(WorkerError::ZeroLengthFrame)??;
    if stream.byte_offset() != payload.len() {
        return Err(WorkerError::TrailingBytes);
    }
    Ok(value)
}

fn verify_binding(name: &'static str, supplied: &str, expected: &str) -> Result<(), WorkerError> {
    if !valid_hash(supplied) || supplied != expected {
        return Err(WorkerError::BindingMismatch(name));
    }
    Ok(())
}

fn snapshot_entries_hash(entries: &[SnapshotEntry]) -> String {
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.key.cmp(&right.key));
    let mut hasher = Sha256::new();
    hasher.update(b"neo-probe-detached-snapshot-v1");
    for entry in sorted {
        update_length_prefixed(&mut hasher, &entry.key);
        update_length_prefixed(&mut hasher, &entry.value);
        hasher.update([u8::from(entry.constant)]);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn hash_parts(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"neo-probe-preflight-v1");
    for part in parts {
        update_length_prefixed(&mut hasher, part);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

// ============================================================================
// Deterministic sandbox pre-execution (task 39)
// ============================================================================
//
// Execution entry: `neo_smart_contract::application_engine::ApplicationEngine`
// — the same public engine the block-persistence pipeline uses — driven through
// its public API (`new`, `load_script`, `execute_allow_fault`). No neo-core file
// was modified. The engine reads from an isolated, memory-only `DataCache`
// seeded from the detached snapshot entries (no live store is reachable,
// since `DataCache::new_with_config`'s backing-store closure types are
// `pub(crate)` in neo-storage and cannot be constructed from outside).
//
// The controller owns double replay (two independent worker processes are
// compared for exact outcome equality), so the worker performs a single
// deterministic execution and exposes a stable `result_fingerprint` covering
// the VM state and all recorded metrics. Divergent replays therefore surface
// as controller-side fingerprint mismatches, never as worker-side verdicts.

/// Result of one deterministic sandbox pre-execution of the probe transaction.
struct VmProbeReport {
    vm_state: &'static str,
    classification: &'static str,
    uncertain_reason: Option<&'static str>,
    fault_category: Option<String>,
    fault_fingerprint: Option<String>,
    fault_message: Option<String>,
    instructions_executed: u64,
    gas_consumed: i64,
    memory_footprint_bytes: u64,
    notifications: usize,
    logs: usize,
    result_stack_items: usize,
    result_fingerprint: String,
}

impl VmProbeReport {
    fn base(
        vm_state: &'static str,
        classification: &'static str,
        memory_footprint_bytes: u64,
    ) -> Self {
        Self {
            vm_state,
            classification,
            uncertain_reason: None,
            fault_category: None,
            fault_fingerprint: None,
            fault_message: None,
            instructions_executed: 0,
            gas_consumed: 0,
            memory_footprint_bytes,
            notifications: 0,
            logs: 0,
            result_stack_items: 0,
            result_fingerprint: String::new(),
        }
    }

    fn completed(memory_footprint_bytes: u64) -> Self {
        Self::base("HALT", "completed", memory_footprint_bytes)
    }

    fn uncertain(
        reason: &'static str,
        vm_state: &'static str,
        memory_footprint_bytes: u64,
    ) -> Self {
        let mut report = Self::base(vm_state, "uncertain", memory_footprint_bytes);
        report.uncertain_reason = Some(reason);
        report
    }

    fn deterministic_fault(
        category: &str,
        message: &str,
        vm_state: &'static str,
        memory_footprint_bytes: u64,
    ) -> Self {
        let mut report = Self::base(vm_state, "deterministic_fault", memory_footprint_bytes);
        report.fault_category = Some(category.to_owned());
        report.fault_fingerprint = Some(vm_fault_fingerprint(category, message, 0, vm_state));
        report.fault_message = Some(fault_excerpt(message));
        report
    }

    /// A transaction is only "accepted" when the sandbox execution halted.
    /// Uncertain and fault outcomes are never reported as accepted.
    fn accepted(&self) -> bool {
        self.classification == "completed"
    }

    fn finalize(mut self) -> Self {
        self.result_fingerprint = vm_result_fingerprint(&self);
        self
    }
}

/// Diagnostic hook counting executed instructions.
///
/// The external fast VM path deliberately bypasses diagnostics (engine
/// policy: diagnostics must not change the consensus execution path), so this
/// counter only advances on the fork-aware interpreter. GAS is charged
/// identically on both paths and is the authoritative work metric; the
/// fingerprint therefore never depends on this counter being non-zero.
#[derive(Debug)]
struct InstructionCounter {
    executed: Arc<AtomicU64>,
}

impl Diagnostic for InstructionCounter {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}
    fn disposed(&mut self) {}
    fn context_loaded(&mut self, _context: &ExecutionContext) {}
    fn context_unloaded(&mut self, _context: &ExecutionContext) {}
    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {
        self.executed.fetch_add(1, Ordering::Relaxed);
    }
    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}

fn sandbox_gas_limit(max_duration_ms: u64) -> i64 {
    SANDBOX_GAS_PER_DURATION_MS.saturating_mul(i64::try_from(max_duration_ms).unwrap_or(i64::MAX))
}

fn memory_footprint(job: &ProbeJob, entries: &[SnapshotEntry]) -> u64 {
    let bytes = job.transaction.transaction_bytes.len()
        + entries
            .iter()
            .map(|entry| entry.key.len() + entry.value.len())
            .sum::<usize>();
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

/// Budget exhaustion (GAS or instruction cap) is a property of the sandbox,
/// not of the transaction: it must never be reported as a fault or an attack.
fn is_budget_exhaustion_fault(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("gas exhausted")
        || lowered.contains("insufficient gas")
        || lowered.contains("maxinstructions")
}

fn fault_category_of(message: &str) -> &'static str {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("stack underflow")
        || lowered.contains("stack overflow")
        || lowered.contains("insufficient stack")
    {
        "stack"
    } else if lowered.contains("abort") {
        "abort"
    } else if lowered.contains("assert") {
        "assert"
    } else if lowered.contains("division")
        || lowered.contains("overflow")
        || lowered.contains("underflow")
    {
        "arithmetic"
    } else if lowered.contains("invalid") {
        "invalid_instruction"
    } else {
        "vm_fault"
    }
}

fn fault_excerpt(message: &str) -> String {
    message.chars().take(FAULT_MESSAGE_EXCERPT_CHARS).collect()
}

fn vm_fault_fingerprint(
    category: &str,
    message: &str,
    gas_consumed: i64,
    vm_state: &str,
) -> String {
    hash_parts(&[
        b"neo-probe-vm-fault-v1",
        category.as_bytes(),
        vm_state.as_bytes(),
        message.as_bytes(),
        &gas_consumed.to_be_bytes(),
    ])
}

fn vm_result_fingerprint(report: &VmProbeReport) -> String {
    hash_parts(&[
        b"neo-probe-vm-result-v1",
        report.vm_state.as_bytes(),
        report.classification.as_bytes(),
        report.uncertain_reason.unwrap_or("").as_bytes(),
        report.fault_category.as_deref().unwrap_or("").as_bytes(),
        report.fault_fingerprint.as_deref().unwrap_or("").as_bytes(),
        &report.instructions_executed.to_be_bytes(),
        &report.gas_consumed.to_be_bytes(),
        &report.notifications.to_be_bytes(),
        &report.logs.to_be_bytes(),
        &report.result_stack_items.to_be_bytes(),
    ])
}

/// Runs one deterministic sandbox pre-execution of `job.transaction` against
/// an isolated, memory-only copy of the detached snapshot.
fn sandbox_preexecute(job: &ProbeJob, entries: &[SnapshotEntry]) -> VmProbeReport {
    let started = Instant::now();
    let budget_ms = job.budget.max_duration_ms;
    let gas_limit = sandbox_gas_limit(budget_ms);
    let memory = memory_footprint(job, entries);

    let tx = match Transaction::from_bytes(&job.transaction.transaction_bytes) {
        Ok(tx) => tx,
        Err(error) => {
            return VmProbeReport::deterministic_fault(
                "malformed_transaction",
                &format!("transaction deserialization failed: {error}"),
                "FAULT",
                memory,
            )
            .finalize();
        }
    };

    // Isolated, memory-only snapshot: a writable DataCache without a backing
    // store, seeded from the detached snapshot entries. All writes performed
    // by the engine are discarded together with the engine.
    let snapshot_cache = Arc::new(DataCache::new(false));
    for entry in entries {
        snapshot_cache.add(
            StorageKey::from_bytes(&entry.key),
            StorageItem::from_bytes(entry.value.clone()),
        );
    }

    let instructions_executed = Arc::new(AtomicU64::new(0));
    let script = tx.script().to_vec();
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let mut engine = match ApplicationEngine::new(
        TriggerType::APPLICATION,
        Some(container),
        snapshot_cache,
        None,
        ProtocolSettings::default_settings(),
        gas_limit,
        Some(Box::new(InstructionCounter {
            executed: Arc::clone(&instructions_executed),
        })),
        default_native_services(),
    ) {
        Ok(engine) => engine,
        Err(error) => {
            return VmProbeReport::deterministic_fault(
                "engine_initialization",
                &format!("sandbox engine initialization failed: {error}"),
                "NONE",
                memory,
            )
            .finalize();
        }
    };

    if let Err(error) = engine.load_script(script, CallFlags::ALL, None) {
        return VmProbeReport::deterministic_fault(
            "script_load",
            &format!("sandbox script load failed: {error}"),
            "NONE",
            memory,
        )
        .finalize();
    }

    let state = engine.execute_allow_fault();
    let gas_consumed = engine.gas_consumed();
    let fault_message = engine.fault_exception().map(str::to_owned);
    let notifications = engine.notifications().len();
    let logs = engine.logs().len();
    let result_stack_items = engine.result_stack().len();
    let timeout_exceeded = started.elapsed().as_millis() > u128::from(budget_ms);
    let vm_state = match state {
        VmState::HALT => "HALT",
        VmState::FAULT => "FAULT",
        VmState::BREAK => "BREAK",
        _ => "NONE",
    };

    let fallback_message = "VM faulted without an exception message";
    let message = fault_message.as_deref().unwrap_or(fallback_message);
    let mut report = if state == VmState::FAULT && is_budget_exhaustion_fault(message) {
        VmProbeReport::uncertain("budget_exceeded", vm_state, memory)
    } else if timeout_exceeded {
        VmProbeReport::uncertain("timeout", vm_state, memory)
    } else if state == VmState::HALT {
        VmProbeReport::completed(memory)
    } else if state == VmState::FAULT {
        let category = fault_category_of(message);
        let mut report = VmProbeReport::base(vm_state, "deterministic_fault", memory);
        report.fault_category = Some(category.to_owned());
        report.fault_fingerprint = Some(vm_fault_fingerprint(
            category,
            message,
            gas_consumed,
            vm_state,
        ));
        report.fault_message = Some(fault_excerpt(message));
        report
    } else {
        VmProbeReport::uncertain("non_final_vm_state", vm_state, memory)
    };
    report.instructions_executed = instructions_executed.load(Ordering::Relaxed);
    report.gas_consumed = gas_consumed;
    report.notifications = notifications;
    report.logs = logs;
    report.result_stack_items = result_stack_items;
    report.finalize()
}

#[cfg(test)]
mod tests {
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
                version: "neo-probe-0.20.0".to_owned(),
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
}
