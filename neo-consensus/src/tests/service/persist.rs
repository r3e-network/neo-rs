use super::helpers::{
    PersistCompletedHarness, create_validators_with_keys, sign_payload,
};
use crate::messages::{ConsensusPayload, PrepareResponseMessage};
use crate::{ConsensusEvent, ConsensusMessageType, ConsensusService};
use neo_primitives::{UInt160, UInt256};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Returns a unique, non-existent temp file path for a recovery-log test and a
/// guard that deletes it (and its `.tmp` sibling) on drop.
fn temp_state_path(tag: &str) -> (PathBuf, TempFileGuard) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "neo-consensus-{tag}-{}-{nanos}.bin",
        std::process::id()
    ));
    let guard = TempFileGuard(path.clone());
    (path, guard)
}

struct TempFileGuard(PathBuf);
impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.0.with_extension("tmp"));
    }
}

/// Drives a fresh 4-validator primary (index 0) to sign and broadcast its own
/// Commit for block 0 / view 0, persisting to `state_path`. Returns the service
/// (post-commit) and the proposed block hash it committed to.
async fn drive_primary_to_own_commit(
    network: u32,
    keys: &[[u8; 32]],
    validators: Vec<crate::ValidatorInfo>,
    state_path: Option<PathBuf>,
) -> (ConsensusService, UInt256, mpsc::Receiver<ConsensusEvent>) {
    let (tx, mut rx) = mpsc::channel(100);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);
    service.set_state_path(state_path);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).await.unwrap();

    // Drain until the primary's PrepareRequest has been emitted.
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                break;
            }
        }
    }
    let preparation_hash = service.context().preparation_hash.expect("preparation hash");

    // Two PrepareResponses (+ the primary's own prepare) reach M=3 → the primary
    // signs and broadcasts its OWN Commit, which triggers the save.
    for validator_index in 1..=2u8 {
        let response = PrepareResponseMessage::new(0, 0, validator_index, preparation_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::PrepareResponse,
            response.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).await.unwrap();
    }

    let block_hash = service.context().proposed_block_hash.expect("committed block hash");
    (service, block_hash, rx)
}

#[tokio::test]
async fn persist_completed_starts_consensus_round() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    let prev_hash = UInt256::from([0x01; 32]);

    harness
        .persist_completed_all(0, prev_hash, 1_000)
        .expect("persist completed");
    harness
        .fire_primary_prepare_timers()
        .await
        .expect("primary prepare timer");
    harness.drive_until_idle(50).await.expect("drive");

    assert!(harness.saw_prepare_request(1));
}

#[tokio::test]
async fn persist_completed_multiple_rounds() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);

    for round in 0u32..3 {
        let prev_hash = UInt256::from([round as u8; 32]);
        harness
            .persist_completed_all(round, prev_hash, 1_000 + round as u64)
            .expect("persist completed");
        harness
            .fire_primary_prepare_timers()
            .await
            .expect("primary prepare timer");
        harness.drive_until_idle(50).await.expect("drive");

        assert!(harness.saw_prepare_request(round + 1));
        harness.take_events();
    }
}

#[tokio::test]
async fn persist_completed_round_emits_block_committed() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    let prev_hash = UInt256::from([0x02; 32]);

    harness
        .persist_completed_all(0, prev_hash, 1_000)
        .expect("persist completed");
    harness
        .fire_primary_prepare_timers()
        .await
        .expect("primary prepare timer");
    harness.drive_until_idle(200).await.expect("drive");

    assert!(harness.saw_block_committed(1));
}

/// P0 crash-safety (FIX 2): after this node signs its own Commit, the recovery
/// log is written BEFORE the Commit is broadcast (C# `CheckPreparations` ->
/// `context.Save()` before `localNode.Tell`).
#[tokio::test]
async fn own_commit_persists_recovery_log_before_broadcast() {
    let network = 0x4E454F;
    let (validators, keys) = create_validators_with_keys(4);
    let (state_path, _guard) = temp_state_path("save");

    assert!(!state_path.exists(), "precondition: no state file yet");

    let (service, block_hash, _rx) =
        drive_primary_to_own_commit(network, &keys, validators.clone(), Some(state_path.clone()))
            .await;

    // The node recorded its own commit...
    assert!(
        service.context().commits.contains_key(&0),
        "primary must have signed its own commit"
    );
    // ...and the recovery log now exists on disk.
    assert!(
        state_path.exists(),
        "recovery log must be persisted after signing own commit"
    );

    // The persisted state round-trips and records the commit at (block 0, view 0)
    // over the real proposed block hash.
    let reloaded = crate::context::ConsensusContext::load(
        &state_path,
        validators,
        Some(0),
    )
    .expect("recovery log loads");
    assert_eq!(reloaded.block_index, 0);
    assert_eq!(reloaded.view_number, 0);
    assert!(
        reloaded.commits.contains_key(&0),
        "persisted state must record our own commit"
    );
    assert_eq!(
        reloaded.proposed_block_hash,
        Some(block_hash),
        "persisted state must record the real proposed block hash"
    );
    assert!(
        reloaded.prepare_request_received,
        "persisted state must record that a PrepareRequest was established"
    );
}

/// P0 crash-safety (FIX 2): a fresh service that loads the persisted recovery log
/// resumes the SAME block and refuses to sign a different block at that
/// (height, view). Simulates a crash-restart after the commit was signed.
#[tokio::test]
async fn fresh_service_resumes_from_log_and_will_not_double_sign() {
    let network = 0x4E454F;
    let (validators, keys) = create_validators_with_keys(4);
    let (state_path, _guard) = temp_state_path("resume");

    // Round 1: original node signs its own commit and persists.
    let (_orig, original_block_hash, _rx0) =
        drive_primary_to_own_commit(network, &keys, validators.clone(), Some(state_path.clone()))
            .await;
    assert!(state_path.exists());

    // Round 2: a brand-new service (fresh in-memory state) restarts and loads the
    // recovery log for the same block 0.
    let (tx, mut rx) = mpsc::channel(100);
    let mut restarted =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);
    restarted.set_state_path(Some(state_path.clone()));

    let resumed = restarted
        .try_load_and_resume(0, 2_000, UInt256::zero(), UInt160::zero(), 0)
        .await
        .expect("resume must not error");
    assert!(resumed, "must resume from the recovery log for block 0");

    // The resumed node holds exactly the block it already committed to — it can
    // NOT sign a different block at (height=0, view=0).
    assert_eq!(
        restarted.context().proposed_block_hash,
        Some(original_block_hash),
        "resumed proposed block hash must equal the originally signed block"
    );
    assert!(
        restarted.context().commits.contains_key(&0),
        "resumed state must retain our original commit"
    );

    // Resuming must NOT emit a second/different Commit for the same (height, view).
    let mut extra_commit = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::Commit {
                extra_commit = Some(payload);
            }
        }
    }
    assert!(
        extra_commit.is_none(),
        "a resumed node must not broadcast a second Commit at the same (height, view)"
    );

    // Even if M PrepareResponses are re-delivered after resume, the node must not
    // re-sign a (different) commit — its own commit is already recorded.
    let preparation_hash = restarted
        .context()
        .preparation_hash
        .expect("preparation hash restored from log");
    for validator_index in 1..=2u8 {
        let response = PrepareResponseMessage::new(0, 0, validator_index, preparation_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::PrepareResponse,
            response.serialize(),
        );
        sign_payload(&restarted, &mut payload, &keys[validator_index as usize]);
        // May be a duplicate (AlreadyReceived) — that is fine; the point is no
        // new Commit is produced.
        let _ = restarted.process_message(payload).await;
    }
    let mut post_resume_commit = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::Commit {
                post_resume_commit = true;
            }
        }
    }
    assert!(
        !post_resume_commit,
        "resumed node must not sign a new Commit even when re-fed prepare responses"
    );
    // The block hash is still the original one — never replaced.
    assert_eq!(
        restarted.context().proposed_block_hash,
        Some(original_block_hash),
    );
}

/// A recovery log written for a *different* block index is stale and must be
/// ignored, so a fresh round starts normally (C# `Deserialize` rejects a
/// mismatched `Block.Index`).
#[tokio::test]
async fn stale_recovery_log_for_other_block_is_ignored() {
    let network = 0x4E454F;
    let (validators, keys) = create_validators_with_keys(4);
    let (state_path, _guard) = temp_state_path("stale");

    // Persist a log for block 0.
    let (_orig, _hash, _rx0) =
        drive_primary_to_own_commit(network, &keys, validators.clone(), Some(state_path.clone()))
            .await;
    assert!(state_path.exists());

    // A fresh service tries to resume block 5 — the log is for block 0, so it
    // must be ignored (resume returns false → caller does a normal fresh start).
    let (tx, _rx) = mpsc::channel(100);
    let mut svc = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);
    svc.set_state_path(Some(state_path));

    let resumed = svc
        .try_load_and_resume(5, 3_000, UInt256::zero(), UInt160::zero(), 0)
        .await
        .expect("resume must not error");
    assert!(!resumed, "stale log for a different block must not resume");
}
