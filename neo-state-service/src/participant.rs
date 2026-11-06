//! Canonical state-service commit participant.
//!
//! [`StateServiceCommitParticipant`] is the single, cohesive integration point
//! between local state root calculation and the canonical block-persistence
//! commit gate. It replaces the legacy `neo-core` pair
//! `StateServiceCommitHandlers` (`CommittingHandler` + `CommittedHandler`).
//!
//! # Phase mapping
//!
//! * **stage / precommit** — [`StateServiceCommitParticipant::try_stage`] applies
//!   the block's storage change set to the state Merkle-Patricia-trie and stages
//!   the new root (`StateStore::update_local_state_root_from_cache`). Staging
//!   never advances the durable local root index and never commits the state
//!   backend.
//! * **discard** — [`StateServiceCommitParticipant::discard_staged`] drops the
//!   staged snapshot (`StateStore::discard_staged_local_state_root`). The
//!   pipeline calls it for every *already-staged* participant in **reverse
//!   registration order** when a precommit vote, the store commit or the runtime
//!   cache update fails. It is infallible and idempotent, and it never rolls
//!   back an already-committed root.
//! * **commit** — [`StateServiceCommitParticipant::commit`] persists the staged
//!   trie changes and advances the current local root index
//!   (`StateStore::update_local_state_root`).
//!
//! # Guarantees inherited from `StateStore` (not re-implemented here)
//!
//! Sorted change-set application, trie put/delete, trie flush, the **fail-closed**
//! reference-root comparison, `StateRoot::new_current`, snapshot staging and
//! retention, `stage_commit`/`commit_backend` with
//! `discard_pending + node_cache.clear + take + Err` on failure, the
//! **2000-block** trie cache recycle, `update_current_snapshot` and
//! `check_validated_state_root`.
//!
//! # Guarantees owned by this participant
//!
//! * `disabled: AtomicBool` latching with the five `UnhandledExceptionPolicy`
//!   dispositions (`Ignore`, `Continue`, `StopPlugin`, `StopNode`, `Terminate`).
//! * `std::panic::catch_unwind` protection of the staging path.
//! * The exact legacy panic/error messages.
//! * Fast-sync opt-in: [`StateServiceCommitParticipant::run_during_fast_sync`]
//!   returns `true`, so state root calculation stays active during fast-sync
//!   (which in turn forces detailed execution receipts in step 1 of the
//!   pipeline).
//!
//! # Error contract
//!
//! Every fallible entry point returns `Result<(), String>` carrying the
//! participant's **own** message. The persistence pipeline lifts that string
//! verbatim into `LedgerError::ParticipantCommit`, whose `Display` reproduces it
//! unchanged — this participant therefore never constructs a ledger error type
//! and never needs one.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::state_store::StateStore;
use neo_primitives::{UnhandledExceptionPolicy, panic_message};
use neo_storage::persistence::DataCache;
use tracing::error;

/// Message emitted when the participant refuses to stage because a previous
/// failure latched it under a plugin-stopping policy.
const DISABLED_MESSAGE: &str =
    "state service committing handler is disabled after a previous failure";

/// Tracing target preserved verbatim from the legacy handlers.
const TRACING_TARGET: &str = "neo::state_service";

/// State root calculation as a canonical commit participant.
///
/// Cheap to clone-by-`Arc`; the participant itself is registered once in the
/// persistence pipeline and shared across all blocks.
pub struct StateServiceCommitParticipant {
    state_store: Arc<StateStore>,
    exception_policy: UnhandledExceptionPolicy,
    disabled: AtomicBool,
}

impl StateServiceCommitParticipant {
    /// Creates a participant for the given state store.
    ///
    /// The exception policy is read from the store's authoritative settings —
    /// it is never duplicated configuration.
    pub fn new(state_store: Arc<StateStore>) -> Self {
        let exception_policy = state_store.exception_policy();
        Self {
            state_store,
            exception_policy,
            disabled: AtomicBool::new(false),
        }
    }

    /// The state store this participant stages into.
    pub fn state_store(&self) -> &Arc<StateStore> {
        &self.state_store
    }

    /// The authoritative unhandled-exception policy of the backing store.
    pub fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    /// Whether a previous failure latched this participant off.
    pub fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    /// Fast-sync opt-in.
    ///
    /// State root calculation must run during fast-sync too: the local root is
    /// the only durable record that the node computed the state itself.
    pub fn run_during_fast_sync(&self) -> bool {
        true
    }

    /// Stages the state root derived from `snapshot` for `height` (precommit).
    ///
    /// Returns the participant's own failure message on error. On success the
    /// root is *staged only*: the durable local root index does not advance and
    /// the state backend is not committed until [`Self::commit`].
    ///
    /// The staging path is panic-safe: a panic inside the store is caught,
    /// reported through [`Self::handle_panic`] (which applies the exception
    /// policy) and converted into an error message.
    pub fn try_stage(&self, height: u32, snapshot: &DataCache) -> Result<(), String> {
        if self.disabled.load(Ordering::Relaxed) {
            return Err(DISABLED_MESSAGE.to_string());
        }

        let state_store = Arc::clone(&self.state_store);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state_store.update_local_state_root_from_cache(height, snapshot)
        }));

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(format!(
                "state service commit handler failed during committing: {err}"
            )),
            Err(payload) => {
                let message = panic_message(payload.as_ref(), "unknown panic payload");
                self.handle_panic(payload, "committing");
                Err(format!(
                    "state service commit handler panicked during committing: {message}"
                ))
            }
        }
    }

    /// Infallible staging entry point (legacy `blockchain_committing_handler`).
    ///
    /// Failures are reported and routed through the exception policy instead of
    /// being propagated.
    pub fn stage(&self, height: u32, snapshot: &DataCache) {
        if let Err(err) = self.try_stage(height, snapshot) {
            self.handle_error(&err, "committing");
        }
    }

    /// Persists the staged state root for `height` (committed).
    ///
    /// On failure the store has already discarded its pending changes and
    /// dropped the staged snapshot; this participant additionally reports the
    /// error through the exception policy.
    pub fn try_commit(&self, height: u32) -> Result<(), String> {
        self.state_store
            .update_local_state_root(height)
            .map_err(|err| format!("state service commit failed for block {height}: {err}"))
    }

    /// Infallible commit entry point (legacy `blockchain_committed_handler`).
    ///
    /// A latched participant skips the commit entirely, exactly as the legacy
    /// handler did.
    pub fn commit(&self, height: u32) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        if let Err(err) = self.try_commit(height) {
            self.handle_error(&err, "committed");
        }
    }

    /// Discards the staged state root, if any (reverse-order rollback).
    ///
    /// Infallible and idempotent: calling it without a staged snapshot is a
    /// no-op, and an already-committed root is never rolled back.
    pub fn discard_staged(&self) {
        self.state_store.discard_staged_local_state_root();
    }

    /// Reports a caught panic and applies the exception policy.
    ///
    /// The `error` tracing field carries exactly [`panic_message`] with the
    /// `"unknown panic payload"` fallback, preserving the legacy diagnostics.
    pub(crate) fn handle_panic(&self, payload: Box<dyn Any + Send>, phase: &'static str) {
        error!(
            target: TRACING_TARGET,
            phase,
            error = panic_message(payload.as_ref(), "unknown panic payload"),
            "state service handler panicked"
        );
        self.apply_exception_policy();
    }

    /// Reports a failure message and applies the exception policy.
    pub(crate) fn handle_error(&self, err: &str, phase: &'static str) {
        error!(
            target: TRACING_TARGET,
            phase,
            error = %err,
            "state service handler failed"
        );
        self.apply_exception_policy();
    }

    /// Applies the authoritative exception policy.
    ///
    /// `StopPlugin` latches [`Self::is_disabled`]; `StopNode` exits the process
    /// and `Terminate` aborts it (both owned by `neo-primitives`); `Ignore` and
    /// `Continue` keep the participant running.
    pub(crate) fn apply_exception_policy(&self) {
        self.exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }
}

/// Canonical `CommitParticipant` implementation (task-102 Part C / task-106
/// contract).
///
/// Gated behind `cfg(feature = "ledger-participant")` because the trait is owned by
/// `neo-ledger`. The default build of this crate is therefore completely
/// ledger-free; integration (task-89) enables the gate by adding
/// `neo-ledger = { workspace = true, optional = true }` plus
/// `ledger-participant = ["dep:neo-ledger"]` to this crate's manifest and the
/// `("neo-state-service", "neo-ledger")` Services→ProtocolExecution edge to the
/// layer guard's `ALLOWED_EDGES`.
///
/// The block is a pure delegation to the inherent API above — it adds no
/// behaviour of its own, so there is exactly one implementation of every
/// guarantee.
#[cfg(feature = "ledger-participant")]
mod ledger_participant_impl {
    use super::StateServiceCommitParticipant;
    use neo_ledger::persistence::{CommitContext, CommitParticipant, StageContext};

    impl CommitParticipant for StateServiceCommitParticipant {
        /// Fast-sync opt-in (step 12 gate + step 1 detailed-receipt forcing).
        fn run_during_fast_sync(&self) -> bool {
            StateServiceCommitParticipant::run_during_fast_sync(self)
        }

        /// Step 12 — registration-order stage + precommit vote.
        ///
        /// `Err(message)` is the participant's own text and is lifted verbatim
        /// into `LedgerError::ParticipantCommit`.
        fn precommit(&self, context: &StageContext<'_>) -> Result<(), String> {
            self.try_stage(context.height(), context.snapshot)
        }

        /// Steps 12/13/14 rollback — invoked in reverse registration order.
        fn discard(&self, _context: &StageContext<'_>) {
            self.discard_staged();
        }

        /// Step 15a — infallible finalization for every participant.
        fn committed(&self, context: &CommitContext<'_>) {
            self.commit(context.height());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::{StateServiceSettings, StateStoreBackend};
    use neo_storage::{StorageItem, StorageKey};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct ErrorMessages(Arc<std::sync::Mutex<Vec<String>>>);

    impl tracing::field::Visit for ErrorMessages {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "error" {
                self.0.lock().unwrap().push(format!("{value:?}"));
            }
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if field.name() == "error" {
                self.0.lock().unwrap().push(value.to_string());
            }
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ErrorMessages {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if event.metadata().target() == TRACING_TARGET {
                event.record(&mut self.clone());
            }
        }
    }

    struct FailingStateStoreBackend;

    impl StateStoreBackend for FailingStateStoreBackend {
        fn try_get(&self, _key: &[u8]) -> Option<Vec<u8>> {
            None
        }

        fn put(&self, _key: Vec<u8>, _value: Vec<u8>) {}

        fn delete(&self, _key: &[u8]) {}

        fn commit(&self) -> Result<(), String> {
            Err("injected state root commit failure".to_string())
        }
    }

    fn state_store_with_policy(exception_policy: UnhandledExceptionPolicy) -> Arc<StateStore> {
        let settings = StateServiceSettings {
            exception_policy,
            ..StateServiceSettings::default()
        };
        Arc::new(StateStore::new(
            Arc::new(FailingStateStoreBackend),
            settings,
        ))
    }

    #[test]
    fn committing_stages_root_without_advancing_until_block_is_committed() {
        let state_store = state_store_with_policy(UnhandledExceptionPolicy::StopPlugin);
        let handler = StateServiceCommitParticipant::new(Arc::clone(&state_store));
        let snapshot = DataCache::new(false);

        handler
            .try_stage(1, &snapshot)
            .expect("staging must not commit the state backend");
        assert_eq!(state_store.local_root_index(), None);

        handler.commit(1);
        assert!(handler.disabled.load(Ordering::Relaxed));
        assert_eq!(state_store.local_root_index(), None);
    }

    #[test]
    fn committing_handler_disables_after_error_when_policy_stops_plugin() {
        let handler = StateServiceCommitParticipant::new(state_store_with_policy(
            UnhandledExceptionPolicy::StopPlugin,
        ));
        let snapshot = DataCache::new(false);

        handler
            .try_stage(1, &snapshot)
            .expect("state root should be staged");
        handler.commit(1);

        let err = handler
            .try_stage(2, &snapshot)
            .expect_err("stop-plugin policy should disable future state root commits");

        assert!(err.contains("disabled after a previous failure"));
    }

    #[test]
    fn committing_handler_keeps_running_after_error_when_policy_continues() {
        let handler = StateServiceCommitParticipant::new(state_store_with_policy(
            UnhandledExceptionPolicy::Continue,
        ));
        let snapshot = DataCache::new(false);

        handler
            .try_stage(1, &snapshot)
            .expect("state root should be staged");
        handler.commit(1);

        handler
            .try_stage(2, &snapshot)
            .expect("continue policy should keep staging available");
        handler.commit(2);
        assert!(!handler.disabled.load(Ordering::Relaxed));

        handler
            .try_stage(3, &snapshot)
            .expect("continue policy should allow subsequent staging");
    }

    #[test]
    fn handler_panic_messages_and_policy_effects_remain_unchanged() {
        for policy in [
            UnhandledExceptionPolicy::Ignore,
            UnhandledExceptionPolicy::Continue,
            UnhandledExceptionPolicy::StopPlugin,
        ] {
            let payloads: [(Box<dyn Any + Send>, &str); 3] = [
                (Box::new("borrowed state panic"), "borrowed state panic"),
                (
                    Box::new("owned state panic".to_string()),
                    "owned state panic",
                ),
                (Box::new(7_u8), "unknown panic payload"),
            ];
            for (payload, expected_message) in payloads {
                let handler = StateServiceCommitParticipant::new(state_store_with_policy(policy));
                let messages = ErrorMessages::default();
                let subscriber = tracing_subscriber::registry().with(messages.clone());

                tracing::subscriber::with_default(subscriber, || {
                    handler.handle_panic(payload, "committing");
                });

                assert_eq!(
                    *messages.0.lock().unwrap(),
                    vec![expected_message.to_string()]
                );
                assert_eq!(
                    handler.disabled.load(Ordering::Relaxed),
                    policy == UnhandledExceptionPolicy::StopPlugin
                );
            }
        }
    }

    #[test]
    fn committing_handler_keeps_running_after_error_when_policy_ignores() {
        let handler = StateServiceCommitParticipant::new(state_store_with_policy(
            UnhandledExceptionPolicy::Ignore,
        ));
        let snapshot = DataCache::new(false);

        handler
            .try_stage(1, &snapshot)
            .expect("state root should be staged");
        handler.commit(1);
        assert!(!handler.disabled.load(Ordering::Relaxed));
        handler
            .try_stage(2, &snapshot)
            .expect("ignore policy should keep staging available");
    }

    #[test]
    fn try_committing_handler_stages_storage_changes_before_commit_gate() {
        let state_store = Arc::new(StateStore::new_in_memory());
        let handler = StateServiceCommitParticipant::new(Arc::clone(&state_store));
        let snapshot = DataCache::new(false);
        snapshot.add(
            StorageKey::new(123, b"state-key".to_vec()),
            StorageItem::from_bytes(b"state-value".to_vec()),
        );

        handler
            .try_stage(1, &snapshot)
            .expect("state root should be staged");

        assert_eq!(state_store.local_root_index(), None);
        handler.commit(1);
        assert_eq!(state_store.local_root_index(), Some(1));
    }

    // ------------------------------------------------------------------
    // New: reverse-order multi-participant discard (steps 12/13/14 rollback)
    // ------------------------------------------------------------------

    /// Local stand-in for `neo_ledger::persistence::CommitParticipant` so the
    /// rollback ordering can be asserted without depending on the ledger crate.
    /// The signatures mirror the canonical trait exactly (`Result<(), String>`
    /// precommit, infallible discard/committed).
    trait TestParticipant: Send + Sync {
        fn precommit(&self, height: u32, snapshot: &DataCache) -> Result<(), String>;
        fn discard(&self, _height: u32, _snapshot: &DataCache) {}
        fn committed(&self, _height: u32) {}
    }

    impl TestParticipant for StateServiceCommitParticipant {
        fn precommit(&self, height: u32, snapshot: &DataCache) -> Result<(), String> {
            self.try_stage(height, snapshot)
        }

        fn discard(&self, _height: u32, _snapshot: &DataCache) {
            self.discard_staged();
        }

        fn committed(&self, height: u32) {
            self.commit(height);
        }
    }

    /// State-service participant that records its discard position, used to
    /// prove that rollback visits state participants in reverse order too.
    struct LoggingStateParticipant {
        name: &'static str,
        log: Arc<std::sync::Mutex<Vec<String>>>,
        inner: StateServiceCommitParticipant,
    }

    impl LoggingStateParticipant {
        fn new(
            name: &'static str,
            log: Arc<std::sync::Mutex<Vec<String>>>,
            store: Arc<StateStore>,
        ) -> Self {
            Self {
                name,
                log,
                inner: StateServiceCommitParticipant::new(store),
            }
        }

        fn store(&self) -> &Arc<StateStore> {
            self.inner.state_store()
        }

        fn is_disabled(&self) -> bool {
            self.inner.is_disabled()
        }

        fn discard_staged(&self) {
            self.inner.discard_staged();
        }

        fn commit(&self, height: u32) {
            self.inner.commit(height);
        }
    }

    impl TestParticipant for LoggingStateParticipant {
        fn precommit(&self, height: u32, snapshot: &DataCache) -> Result<(), String> {
            self.inner.precommit(height, snapshot)
        }

        fn discard(&self, height: u32, snapshot: &DataCache) {
            self.log.lock().unwrap().push(self.name.to_string());
            self.inner.discard(height, snapshot);
        }

        fn committed(&self, height: u32) {
            self.inner.committed(height);
        }
    }

    /// Records its discard order without owning a state store.
    struct RecordingParticipant {
        name: &'static str,
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl TestParticipant for RecordingParticipant {
        fn precommit(&self, _height: u32, _snapshot: &DataCache) -> Result<(), String> {
            Ok(())
        }

        fn discard(&self, _height: u32, _snapshot: &DataCache) {
            self.log.lock().unwrap().push(self.name.to_string());
        }
    }

    /// Mirrors `neo_ledger::persistence::discard_staged_reverse`: every
    /// already-staged participant is discarded in **reverse registration order**.
    fn discard_staged_reverse(
        participants: &[Arc<dyn TestParticipant>],
        staged: &[usize],
        height: u32,
        snapshot: &DataCache,
    ) {
        for &index in staged.iter().rev() {
            participants[index].discard(height, snapshot);
        }
    }

    /// Mirrors the step-12 registration-order precommit loop: the first `Err`
    /// aborts the gate and triggers the reverse discard.
    fn stage_all(
        participants: &[Arc<dyn TestParticipant>],
        height: u32,
        snapshot: &DataCache,
    ) -> Result<Vec<usize>, String> {
        let mut staged: Vec<usize> = Vec::new();
        for (index, participant) in participants.iter().enumerate() {
            if let Err(message) = participant.precommit(height, snapshot) {
                discard_staged_reverse(participants, &staged, height, snapshot);
                return Err(message);
            }
            staged.push(index);
        }
        Ok(staged)
    }

    #[test]
    fn reverse_order_discard_unwinds_multi_participant_staging_after_commit_failure() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));

        // Registration order (index == position):
        //   0 recorder-a, 1 state service over a failing backend,
        //   2 state service over a healthy in-memory store, 3 recorder-b.
        let failing_store = state_store_with_policy(UnhandledExceptionPolicy::Ignore);
        let failing = Arc::new(LoggingStateParticipant::new(
            "state-failing",
            Arc::clone(&log),
            Arc::clone(&failing_store),
        ));
        let healthy_store = Arc::new(StateStore::new_in_memory());
        let healthy = Arc::new(LoggingStateParticipant::new(
            "state-healthy",
            Arc::clone(&log),
            Arc::clone(&healthy_store),
        ));

        let participants: Vec<Arc<dyn TestParticipant>> = vec![
            Arc::new(RecordingParticipant {
                name: "recorder-a",
                log: Arc::clone(&log),
            }),
            Arc::clone(&failing) as Arc<dyn TestParticipant>,
            Arc::clone(&healthy) as Arc<dyn TestParticipant>,
            Arc::new(RecordingParticipant {
                name: "recorder-b",
                log: Arc::clone(&log),
            }),
        ];

        let snapshot = DataCache::new(false);
        snapshot.add(
            StorageKey::new(123, b"state-key".to_vec()),
            StorageItem::from_bytes(b"state-value".to_vec()),
        );

        // Step 12 succeeds for the whole registration order — staging never
        // advances the durable root index on either store.
        let staged = stage_all(&participants, 1, &snapshot).expect("all participants stage");
        assert_eq!(staged, vec![0, 1, 2, 3]);
        assert_eq!(failing_store.local_root_index(), None);
        assert_eq!(healthy_store.local_root_index(), None);

        // Step 13 equivalent: the state backend commit fails for one participant
        // after every participant has already staged.
        let commit_error = failing
            .store()
            .update_local_state_root(1)
            .map_err(|err| format!("state service commit failed for block 1: {err}"))
            .expect_err("injected backend failure must surface verbatim");
        assert_eq!(
            commit_error,
            "state service commit failed for block 1: injected state root commit failure"
        );

        // Rollback discards every already-staged participant in reverse order.
        discard_staged_reverse(&participants, &staged, 1, &snapshot);
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "recorder-b".to_string(),
                "state-healthy".to_string(),
                "state-failing".to_string(),
                "recorder-a".to_string(),
            ],
            "participants must be discarded in reverse registration order"
        );

        // Both stores dropped their staged snapshot: the root index stays
        // un-advanced and a later commit is a no-op instead of resurrecting
        // staged trie writes.
        assert_eq!(failing_store.local_root_index(), None);
        assert_eq!(healthy_store.local_root_index(), None);
        healthy.commit(1);
        assert_eq!(healthy_store.local_root_index(), None);

        // Discard is idempotent for a participant with nothing staged.
        failing.discard_staged();
        healthy.discard_staged();
        assert_eq!(failing_store.local_root_index(), None);
        assert_eq!(healthy_store.local_root_index(), None);
        assert!(!failing.is_disabled());

        // A committed root is never rolled back by a later discard.
        let durable_store = Arc::new(StateStore::new_in_memory());
        let durable = StateServiceCommitParticipant::new(Arc::clone(&durable_store));
        durable
            .try_stage(1, &snapshot)
            .expect("durable staging must succeed");
        durable.commit(1);
        assert_eq!(durable_store.local_root_index(), Some(1));
        durable.discard_staged();
        assert_eq!(
            durable_store.local_root_index(),
            Some(1),
            "discard must not roll back an already-committed state root"
        );
        assert!(!durable.is_disabled());
    }

    #[test]
    fn precommit_failure_aborts_gate_and_discards_only_already_staged_participants() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));

        struct RefusingParticipant;

        impl TestParticipant for RefusingParticipant {
            fn precommit(&self, _height: u32, _snapshot: &DataCache) -> Result<(), String> {
                Err(
                    "state service commit handler failed during committing: injected staging failure"
                        .to_string(),
                )
            }
        }

        let store = Arc::new(StateStore::new_in_memory());
        let state = Arc::new(LoggingStateParticipant::new(
            "state-healthy",
            Arc::clone(&log),
            Arc::clone(&store),
        ));
        let participants: Vec<Arc<dyn TestParticipant>> = vec![
            Arc::new(RecordingParticipant {
                name: "recorder-a",
                log: Arc::clone(&log),
            }),
            Arc::clone(&state) as Arc<dyn TestParticipant>,
            Arc::new(RefusingParticipant),
            Arc::new(RecordingParticipant {
                name: "recorder-b-never-staged",
                log: Arc::clone(&log),
            }),
        ];

        let snapshot = DataCache::new(false);
        let err = stage_all(&participants, 1, &snapshot)
            .expect_err("the refusing participant must abort the commit gate");
        assert_eq!(
            err,
            "state service commit handler failed during committing: injected staging failure"
        );

        // Only indices 0 and 1 staged, so the refusing participant and the
        // never-reached recorder are not discarded; the staged pair is, in reverse.
        assert_eq!(
            *log.lock().unwrap(),
            vec!["state-healthy".to_string(), "recorder-a".to_string()]
        );

        // The state service participant unwound its staged root.
        assert_eq!(store.local_root_index(), None);
        assert!(!state.is_disabled());
    }
}
