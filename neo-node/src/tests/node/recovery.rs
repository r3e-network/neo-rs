use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::node::recovery::{LocalReplayGuard, refuse_local_replay_marker};

#[test]
fn replay_guard_poison_marker_is_durable_and_requests_shutdown() {
    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = CancellationToken::new();
    let guard = LocalReplayGuard::new(Some(marker.clone()), shutdown.clone());
    assert!(guard.begin_observer_commit());

    assert!(marker.exists(), "observer entry must arm recovery first");
    refuse_local_replay_marker(Some(&marker))
        .expect_err("a crash before the canonical fence must block restart");

    guard.canonical_commit_failed("canonical ledger commit failed");

    assert!(shutdown.is_cancelled());
    let contents = std::fs::read_to_string(&marker).expect("read poison marker");
    assert!(contents.contains("canonical ledger commit failed"));
    let error = refuse_local_replay_marker(Some(&marker))
        .expect_err("startup must reject a poisoned replay domain");
    assert!(error.to_string().contains("local replay is poisoned"));
}

#[test]
fn replay_guard_stops_without_poison_before_observer_publication() {
    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = CancellationToken::new();
    let guard = LocalReplayGuard::new(Some(marker.clone()), shutdown.clone());

    guard.canonical_commit_failed("ordinary canonical commit failure");

    assert!(
        shutdown.is_cancelled(),
        "canonical storage failure is fatal even without a cross-store hazard"
    );
    assert!(!marker.exists());
}

#[test]
fn canonical_commit_clears_pending_poison_marker_hazard() {
    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = CancellationToken::new();
    let guard = Arc::new(LocalReplayGuard::new(
        Some(marker.clone()),
        shutdown.clone(),
    ));
    assert!(guard.begin_observer_commit());
    assert!(marker.exists());
    guard.canonical_commit_succeeded();

    assert!(!marker.exists());

    guard.canonical_commit_failed("failure from a later unrelated operation");

    assert!(shutdown.is_cancelled());
    assert!(!marker.exists());
}

#[test]
fn canonical_success_does_not_clear_an_already_poisoned_marker() {
    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = CancellationToken::new();
    let guard = LocalReplayGuard::new(Some(marker.clone()), shutdown);
    assert!(guard.begin_observer_commit());

    guard.canonical_commit_failed("pre-commit observer failed");
    guard.canonical_commit_succeeded();

    assert!(marker.exists());
    refuse_local_replay_marker(Some(&marker))
        .expect_err("later prefix finalization must not erase recovery evidence");
}

#[test]
fn observer_commit_is_rejected_when_recovery_marker_cannot_be_armed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let blocked_parent = temp.path().join("not-a-directory");
    std::fs::write(&blocked_parent, b"file").expect("create path blocker");
    let marker = blocked_parent.join(".neo-local-replay-poisoned");
    let shutdown = CancellationToken::new();
    let guard = LocalReplayGuard::new(Some(marker), shutdown.clone());

    assert!(!guard.begin_observer_commit());
    assert!(shutdown.is_cancelled());
}
