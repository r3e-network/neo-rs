use super::frame::{decode_payload, read_frame, verify_binding, write_frame};
use super::*;
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

// ============================================================================
// Deterministic sandbox pre-execution (task 39)
// ============================================================================
//
// Execution entry: `neo_smart_contract::application_engine::ApplicationEngine`
// — the same public engine the block-persistence pipeline uses — driven through
// its public API (`new`, `load_script`, `execute_allow_fault`). No neo-core file
// was modified. The engine reads from an isolated, memory-only `PersistenceDataCache`
// seeded from the detached snapshot entries (no live store is reachable,
// since `PersistenceDataCache::new_with_config`'s backing-store closure types are
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

    // Isolated, memory-only snapshot: a writable PersistenceDataCache without a backing
    // store, seeded from the detached snapshot entries. All writes performed
    // by the engine are discarded together with the engine.
    let snapshot_cache = Arc::new(PersistenceDataCache::new(false));
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
