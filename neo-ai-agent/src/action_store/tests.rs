use super::journal::JournalEntry;
use super::*;
use crate::store_files::generation_path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "neo-node-action-{label}-{}-{}.jsonl",
        std::process::id(),
        PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn binding(id: &str) -> ActionBinding {
    ActionBinding::new(
        OperationId::new(id).unwrap(),
        OperationalAction::IncreaseSampling,
        None,
        vec!["deterministic-rule".to_owned()],
        vec!["ab".repeat(32)],
        "task-23-v1".to_owned(),
    )
    .unwrap()
}

fn rotation_options() -> ActionStoreOptions {
    ActionStoreOptions {
        rotate_max_bytes: u64::MAX,
        rotate_max_entries: 3,
        generations: 2,
    }
}

fn write_entries(path: &std::path::Path, entries: &[JournalEntry]) {
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
fn state_serialization_covers_required_vocabulary() {
    for state in [
        ActionState::Admitted,
        ActionState::Running,
        ActionState::Completed,
        ActionState::Uncertain,
        ActionState::ReconciliationRequired,
        ActionState::Rejected,
    ] {
        let encoded = serde_json::to_string(&state).unwrap();
        assert_eq!(
            serde_json::from_str::<ActionState>(&encoded).unwrap(),
            state
        );
    }
}

#[test]
fn same_id_same_binding_is_idempotent_and_changed_binding_is_rejected() {
    let path = path("idempotency");
    let store = DurableActionStore::open(&path).unwrap();
    assert!(matches!(
        store.admit(binding("operation-1"), 10),
        Ok(AdmitOutcome::Admitted(_))
    ));
    assert!(matches!(
        store.admit(binding("operation-1"), 11),
        Ok(AdmitOutcome::Existing(_))
    ));
    let mut changed = binding("operation-1");
    changed.rule_version = "task-23-v2".to_owned();
    assert_eq!(
        store.admit(changed, 12),
        Err(ActionStoreError::BindingConflict)
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn reopen_recovers_completed_state() {
    let path = path("reopen-completed");
    let id = OperationId::new("operation-2").unwrap();
    {
        let store = DurableActionStore::open(&path).unwrap();
        store.admit(binding(id.as_str()), 10).unwrap();
        store.mark_running(&id, 11).unwrap();
        store.mark_completed(&id, 12).unwrap();
    }
    let reopened = DurableActionStore::open(&path).unwrap();
    assert_eq!(
        reopened.query(&id).unwrap().unwrap().state,
        ActionState::Completed
    );
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

#[test]
fn reopen_converts_lost_running_completion_to_uncertain() {
    let path = path("lost-completion");
    let id = OperationId::new("operation-3").unwrap();
    {
        let store = DurableActionStore::open(&path).unwrap();
        store.admit(binding(id.as_str()), 10).unwrap();
        store.mark_running(&id, 11).unwrap();
    }
    let reopened = DurableActionStore::open(&path).unwrap();
    assert_eq!(
        reopened.query(&id).unwrap().unwrap().state,
        ActionState::Uncertain
    );
    assert!(matches!(
        reopened.mark_running(&id, 12),
        Err(ActionStoreError::InvalidTransition(ActionState::Uncertain))
    ));
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

#[test]
fn pre_audit_failure_rejects_without_effect() {
    let path = path("pre-audit");
    let id = OperationId::new("operation-4").unwrap();
    let effect = AtomicBool::new(false);
    let store = DurableActionStore::open(&path).unwrap();
    store.admit(binding(id.as_str()), 10).unwrap();
    store.reject(&id, "pre_audit_failed", 11).unwrap();
    assert!(!effect.load(Ordering::Acquire));
    assert_eq!(
        store.query(&id).unwrap().unwrap().state,
        ActionState::Rejected
    );
    assert!(store.mark_running(&id, 12).is_err());
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn post_effect_audit_failure_requires_reconciliation() {
    let path = path("post-audit");
    let id = OperationId::new("operation-5").unwrap();
    let effect = AtomicBool::new(false);
    let store = DurableActionStore::open(&path).unwrap();
    store.admit(binding(id.as_str()), 10).unwrap();
    store.mark_running(&id, 11).unwrap();
    effect.store(true, Ordering::Release);
    store.mark_reconciliation_required(&id, 12).unwrap();
    assert!(effect.load(Ordering::Acquire));
    assert_eq!(
        store.query(&id).unwrap().unwrap().state,
        ActionState::ReconciliationRequired
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn rotation_keeps_chain_continuous_and_preserves_state_and_idempotency() {
    let path = path("rotate-continuity");
    let options = rotation_options();
    let first = OperationId::new("rotate-op-1").unwrap();
    {
        let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        store.admit(binding(first.as_str()), 10).unwrap();
        for index in 0..5 {
            store
                .admit(binding(&format!("rotate-op-{index}")), 20 + index)
                .unwrap();
        }
        // Drive one operation across a rotation boundary.
        store.mark_running(&first, 30).unwrap();
        store.mark_completed(&first, 31).unwrap();
    }
    assert!(generation_path(&path, 1).exists());
    assert!(generation_path(&path, 2).exists());
    assert!(!generation_path(&path, 3).exists());
    // The active file opens with a checkpoint carrying the previous generation's tail.
    let active = read_entries(&path).unwrap();
    let previous = read_entries(&generation_path(&path, 1)).unwrap();
    let previous_tail = previous.last().unwrap().entry_hash.clone();
    let checkpoint = active[0].rotation.as_ref().unwrap();
    assert_eq!(checkpoint.previous_tail_hash, previous_tail);
    assert_eq!(active[0].previous_hash, previous_tail);
    // Reopen verifies the cross-file chain and replays the full operations map.
    let reopened = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
    assert_eq!(
        reopened.query(&first).unwrap().unwrap().state,
        ActionState::Completed
    );
    // Idempotent admission is unaffected by rotation.
    assert!(matches!(
        reopened.admit(binding(first.as_str()), 40),
        Ok(AdmitOutcome::Existing(_))
    ));
    // The state machine keeps working after a rotation and a reopen.
    let later = OperationId::new("rotate-op-0").unwrap();
    reopened.mark_running(&later, 41).unwrap();
    reopened.mark_completed(&later, 42).unwrap();
    assert_eq!(
        reopened.query(&later).unwrap().unwrap().state,
        ActionState::Completed
    );
    drop(reopened);
    assert!(DurableActionStore::open_with_options(&path, options.clone()).is_ok());
    cleanup(&path, 2);
}

#[test]
fn tampered_previous_generation_fails_closed_on_reopen() {
    let path = path("rotate-tamper");
    let options = rotation_options();
    {
        let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            store
                .admit(binding(&format!("tamper-op-{index}")), 10 + index)
                .unwrap();
        }
    }
    // Corrupt one record inside the previous generation: its own entry hash breaks.
    let archive = generation_path(&path, 2);
    let mut entries = read_entries(&archive).unwrap();
    let target = entries
        .iter_mut()
        .find_map(|entry| entry.record.as_mut())
        .expect("the previous generation contains record entries");
    target.outcome_code = "corrupted".to_owned();
    write_entries(&archive, &entries);
    assert!(matches!(
        DurableActionStore::open_with_options(&path, options.clone()),
        Err(ActionStoreError::BrokenJournal(_))
    ));
    cleanup(&path, 2);
}

#[test]
fn truncated_previous_generation_fails_closed_on_reopen() {
    let path = path("rotate-link");
    let options = rotation_options();
    {
        let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            store
                .admit(binding(&format!("link-op-{index}")), 10 + index)
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
        DurableActionStore::open_with_options(&path, options.clone()),
        Err(ActionStoreError::BrokenJournal(_))
    ));
    cleanup(&path, 2);
}

#[test]
fn interrupted_rotation_is_recovered_with_linking_checkpoint() {
    let path = path("rotate-recovery");
    let options = rotation_options();
    {
        let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
        for index in 0..6 {
            store
                .admit(binding(&format!("recovery-op-{index}")), 10 + index)
                .unwrap();
        }
    }
    // Simulate a crash between the rename and the checkpoint write.
    std::fs::remove_file(&path).unwrap();
    let store = DurableActionStore::open_with_options(&path, options.clone()).unwrap();
    let active = read_entries(&path).unwrap();
    let previous = read_entries(&generation_path(&path, 1)).unwrap();
    assert_eq!(
        active[0].rotation.as_ref().unwrap().previous_tail_hash,
        previous.last().unwrap().entry_hash
    );
    // Admissions continue the global sequence after the recovered checkpoint.
    let admitted = store.admit(binding("recovery-new"), 50).unwrap();
    assert!(matches!(admitted, AdmitOutcome::Admitted(_)));
    let active_after = read_entries(&path).unwrap();
    assert_eq!(
        active_after.last().unwrap().sequence,
        active[0].sequence + 1
    );
    drop(store);
    // Reopen after recovery still verifies the cross-file chain.
    assert!(DurableActionStore::open_with_options(&path, options.clone()).is_ok());
    cleanup(&path, 2);
}
