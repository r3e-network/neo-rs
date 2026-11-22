use super::*;
use crate::store_files::generation_path;
use std::sync::atomic::{AtomicU64, Ordering};

static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "neo-ai-agent-{label}-{}-{}.jsonl",
        std::process::id(),
        PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn authorizer(authority: EvidenceAuthority) -> ProbeAuthorizerMetadata {
    ProbeAuthorizerMetadata {
        evidence_id: "deterministic-rule-1".to_owned(),
        evidence_hash: "ab".repeat(32),
        authority,
        rule_version: "task-23-v1".to_owned(),
        authorized_at_unix_ms: 10,
    }
}

fn binding(id: &str) -> DurableActionBinding {
    DurableActionBinding::new(
        OperationId::new(id).unwrap(),
        OperationalAction::StartProbeRequest,
        None,
        authorizer(EvidenceAuthority::DeterministicRule),
        ActionResourceReservation {
            slots: 1,
            memory_bytes: 32_768,
            max_runtime_ms: 1_000,
        },
    )
    .unwrap()
}

fn rotation_options() -> DurableActionLogOptions {
    DurableActionLogOptions {
        rotate_max_bytes: u64::MAX,
        rotate_max_entries: 3,
        generations: 2,
    }
}

fn write_entries(path: &std::path::Path, entries: &[ActionJournalEntry]) {
    let mut buffer = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut buffer, entry).unwrap();
        buffer.push(b'\n');
    }
    std::fs::write(path, buffer).unwrap();
}

fn cleanup(path: &std::path::Path, generations: u32) {
    let _ = std::fs::remove_file(path);
    for generation in 1..=generations {
        let _ = std::fs::remove_file(generation_path(path, generation));
    }
}

#[test]
fn same_operation_binding_is_idempotent_and_different_binding_is_rejected() {
    let path = path("idempotency");
    let log = DurableActionLog::open(&path).unwrap();
    let admitted = log.admit(binding("operation-1"), 20).unwrap();
    assert!(matches!(admitted, ActionAdmissionOutcome::Admitted(_)));
    let existing = log.admit(binding("operation-1"), 21).unwrap();
    assert!(matches!(existing, ActionAdmissionOutcome::Existing(_)));
    let mut changed = binding("operation-1");
    changed.reservation.memory_bytes += 1;
    assert_eq!(
        log.admit(changed, 22),
        Err(DurableActionError::BindingConflict)
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn snapshot_serialization_is_strict_and_replay_rejects_impossible_first_state() {
    let action_binding = binding("serialization-operation");
    let snapshot = DurableActionSnapshot {
        binding_hash: action_binding.binding_hash().unwrap(),
        binding: action_binding,
        state: DurableActionState::AuthorizedReserved,
        updated_at_unix_ms: 20,
        outcome_code: "authorized_reserved".to_owned(),
    };
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_str::<DurableActionSnapshot>(&encoded).unwrap(),
        snapshot
    );
    let mut with_extra = serde_json::to_value(&snapshot).unwrap();
    with_extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<DurableActionSnapshot>(with_extra).is_err());

    let path = path("invalid-replay-transition");
    let mut impossible = snapshot;
    impossible.state = DurableActionState::EffectInFlight;
    impossible.outcome_code = "effect_started".to_owned();
    let mut entry = ActionJournalEntry {
        schema_version: ACTION_LOG_SCHEMA_VERSION,
        sequence: 0,
        rotation: None,
        snapshot: Some(impossible),
        previous_hash: GENESIS_HASH.to_owned(),
        entry_hash: String::new(),
    };
    entry.entry_hash = calculate_entry_hash(&entry).unwrap();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    serde_json::to_writer(&mut file, &entry).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_data().unwrap();
    assert!(matches!(
        DurableActionLog::open(&path),
        Err(DurableActionError::BrokenJournal(0))
    ));
    let _ = std::fs::remove_file(path);
}

#[test]
fn crash_reopen_marks_lost_completion_uncertain_without_retrying() {
    let path = path("crash-reopen");
    let operation_id = OperationId::new("operation-2").unwrap();
    {
        let log = DurableActionLog::open(&path).unwrap();
        log.admit(binding(operation_id.as_str()), 20).unwrap();
        log.transition(&operation_id, ActionTransition::PreAuditSucceeded, 21)
            .unwrap();
        log.transition(&operation_id, ActionTransition::EffectStarted, 22)
            .unwrap();
    }
    let reopened = DurableActionLog::open(&path).unwrap();
    let snapshot = reopened.query(&operation_id).unwrap().unwrap();
    assert_eq!(snapshot.state, DurableActionState::Uncertain);
    assert!(!snapshot.state.may_execute());
    assert_eq!(
        reopened.transition(&operation_id, ActionTransition::EffectStarted, 23),
        Err(DurableActionError::InvalidTransition {
            from: DurableActionState::Uncertain,
        })
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn audit_failures_fail_closed_and_require_post_effect_reconciliation() {
    let path = path("audit-failures");
    let log = DurableActionLog::open(&path).unwrap();
    let pre_id = OperationId::new("pre-audit").unwrap();
    log.admit(binding(pre_id.as_str()), 20).unwrap();
    let failed = log
        .transition(&pre_id, ActionTransition::PreAuditFailed, 21)
        .unwrap();
    assert_eq!(failed.state, DurableActionState::FailedBeforeEffect);
    assert!(!failed.state.effect_may_have_occurred());
    assert!(matches!(
        log.transition(&pre_id, ActionTransition::EffectStarted, 22),
        Err(DurableActionError::InvalidTransition { .. })
    ));

    let post_id = OperationId::new("post-audit").unwrap();
    log.admit(binding(post_id.as_str()), 30).unwrap();
    log.transition(&post_id, ActionTransition::PreAuditSucceeded, 31)
        .unwrap();
    log.transition(&post_id, ActionTransition::EffectStarted, 32)
        .unwrap();
    log.transition(&post_id, ActionTransition::EffectCompleted, 33)
        .unwrap();
    let reconcile = log
        .transition(&post_id, ActionTransition::PostAuditFailed, 34)
        .unwrap();
    assert_eq!(reconcile.state, DurableActionState::ReconciliationRequired);
    assert!(reconcile.state.effect_may_have_occurred());
    let completed = log
        .reconcile(&post_id, ReconciliationDecision::ConfirmedAndAudited, 35)
        .unwrap();
    assert_eq!(completed.state, DurableActionState::Completed);
    let _ = std::fs::remove_file(path);
}

#[test]
fn model_authority_and_unterminated_tail_are_rejected_or_repaired() {
    let model_binding = DurableActionBinding::new(
        OperationId::new("model-operation").unwrap(),
        OperationalAction::StartProbeRequest,
        None,
        authorizer(EvidenceAuthority::Model),
        ActionResourceReservation {
            slots: 1,
            memory_bytes: 1,
            max_runtime_ms: 1,
        },
    );
    assert_eq!(model_binding, Err(DurableActionError::UntrustedAuthorizer));

    let path = path("tail-repair");
    let operation_id = OperationId::new("tail-operation").unwrap();
    {
        let log = DurableActionLog::open(&path).unwrap();
        log.admit(binding(operation_id.as_str()), 20).unwrap();
    }
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"partial\":").unwrap();
    file.sync_data().unwrap();
    let reopened = DurableActionLog::open(&path).unwrap();
    assert_eq!(
        reopened.query(&operation_id).unwrap().unwrap().state,
        DurableActionState::AuthorizedReserved
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn rotation_keeps_chain_continuous_and_preserves_state_and_idempotency() {
    let path = path("rotate-continuity");
    let options = rotation_options();
    let first = OperationId::new("rotate-op-1").unwrap();
    {
        let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        log.admit(binding(first.as_str()), 10).unwrap();
        for index in 0..5 {
            log.admit(binding(&format!("rotate-op-{index}")), 20 + index)
                .unwrap();
        }
        // Drive one operation across a rotation boundary (and past pruning of
        // its admission entry) to a terminal, non-interrupted state.
        log.transition(&first, ActionTransition::PreAuditSucceeded, 30)
            .unwrap();
        log.transition(&first, ActionTransition::EffectStarted, 31)
            .unwrap();
        log.transition(&first, ActionTransition::EffectCompleted, 32)
            .unwrap();
        log.transition(&first, ActionTransition::PostAuditSucceeded, 33)
            .unwrap();
    }
    assert!(generation_path(&path, 1).exists());
    assert!(generation_path(&path, 2).exists());
    assert!(!generation_path(&path, 3).exists());
    // The active file opens with a checkpoint carrying the previous generation's tail.
    let active = read_entries(&path).unwrap();
    let previous = read_entries(&generation_path(&path, 1)).unwrap();
    let previous_tail = previous.last().unwrap().entry_hash.clone();
    assert_eq!(
        active[0].rotation.as_ref().unwrap().previous_tail_hash,
        previous_tail
    );
    assert_eq!(active[0].previous_hash, previous_tail);
    // Reopen verifies the cross-file chain and replays the full operations map;
    // the operation whose admission entry was pruned anchors on its first
    // retained transition entry.
    let reopened = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
    assert_eq!(
        reopened.query(&first).unwrap().unwrap().state,
        DurableActionState::Completed
    );
    // Idempotent admission is unaffected by rotation.
    assert!(matches!(
        reopened.admit(binding(first.as_str()), 40),
        Ok(ActionAdmissionOutcome::Existing(_))
    ));
    // The state machine keeps working after a rotation and a reopen. Note that
    // operations whose every entry was pruned by retention (e.g. "rotate-op-0")
    // are forgotten: their idempotency window is bounded by the retained files.
    let later = OperationId::new("rotate-op-3").unwrap();
    reopened
        .transition(&later, ActionTransition::PreAuditSucceeded, 41)
        .unwrap();
    assert_eq!(
        reopened.query(&later).unwrap().unwrap().state,
        DurableActionState::ReadyForEffect
    );
    drop(reopened);
    assert!(DurableActionLog::open_with_options(&path, options.clone()).is_ok());
    cleanup(&path, 2);
}

#[test]
fn tampered_previous_generation_fails_closed_on_reopen() {
    let path = path("rotate-tamper");
    let options = rotation_options();
    {
        let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            log.admit(binding(&format!("tamper-op-{index}")), 10 + index)
                .unwrap();
        }
    }
    // Corrupt one snapshot inside the previous generation: its own entry hash breaks.
    let archive = generation_path(&path, 2);
    let mut entries = read_entries(&archive).unwrap();
    let target = entries
        .iter_mut()
        .find_map(|entry| entry.snapshot.as_mut())
        .expect("the previous generation contains record entries");
    target.outcome_code = "corrupted".to_owned();
    write_entries(&archive, &entries);
    assert!(matches!(
        DurableActionLog::open_with_options(&path, options.clone()),
        Err(DurableActionError::BrokenJournal(_))
    ));
    cleanup(&path, 2);
}

#[test]
fn truncated_previous_generation_fails_closed_on_reopen() {
    let path = path("rotate-link");
    let options = rotation_options();
    {
        let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            log.admit(binding(&format!("link-op-{index}")), 10 + index)
                .unwrap();
        }
    }
    // Truncate the previous generation: its new tail no longer matches the
    // checkpoint that opens the next file.
    let archive = generation_path(&path, 2);
    let mut entries = read_entries(&archive).unwrap();
    entries.pop();
    write_entries(&archive, &entries);
    assert!(matches!(
        DurableActionLog::open_with_options(&path, options.clone()),
        Err(DurableActionError::BrokenJournal(_))
    ));
    cleanup(&path, 2);
}

#[test]
fn interrupted_rotation_is_recovered_with_linking_checkpoint() {
    let path = path("rotate-recovery");
    let options = rotation_options();
    {
        let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            log.admit(binding(&format!("recovery-op-{index}")), 10 + index)
                .unwrap();
        }
    }
    // Simulate a crash between the rename and the checkpoint write.
    std::fs::remove_file(&path).unwrap();
    let log = DurableActionLog::open_with_options(&path, options.clone()).unwrap();
    let active = read_entries(&path).unwrap();
    let previous = read_entries(&generation_path(&path, 1)).unwrap();
    assert_eq!(
        active[0].rotation.as_ref().unwrap().previous_tail_hash,
        previous.last().unwrap().entry_hash
    );
    // Admissions continue the global sequence after the recovered checkpoint.
    assert!(matches!(
        log.admit(binding("recovery-new"), 50),
        Ok(ActionAdmissionOutcome::Admitted(_))
    ));
    let active_after = read_entries(&path).unwrap();
    assert_eq!(
        active_after.last().unwrap().sequence,
        active[0].sequence + 1
    );
    drop(log);
    // Reopen after recovery still verifies the cross-file chain.
    assert!(DurableActionLog::open_with_options(&path, options.clone()).is_ok());
    cleanup(&path, 2);
}
