//! Migration middleware adapting a committing/committed handler pair to the
//! canonical [`CommitParticipant`] lifecycle owned by `neo_ledger::persistence`.
//!
//! The legacy core split block collaboration across two independent hooks:
//!
//! * a *committing* handler, invoked while a block is being applied and its
//!   state is still staged, and
//! * a *committed* handler, invoked once the block has been durably persisted.
//!
//! The decomposed ledger unifies both roles behind a single participant trait
//! with three ordered phases (see [`CommitParticipant`]):
//!
//! ```text
//! committing handler  ->  CommitParticipant::precommit(&StageContext)  -> Result<(), String>
//! committed  handler  ->  CommitParticipant::committed(&CommitContext)
//! rollback   handler  ->  CommitParticipant::discard(&StageContext)
//! ```
//!
//! [`HandlerParticipantBridge`] is the adapter that lets an existing handler
//! pair speak that lifecycle without the ledger — or this crate — depending on
//! `neo-core`. It is deliberately **temporary**: once every collaborator owns a
//! direct participant implementation (e.g. the task-107 state-service
//! participant), this bridge is removed.
//!
//! # Type discipline
//!
//! This module names only `neo_ledger::persistence` participant types and `std`.
//! It never names `DataCache`, `StoreSnapshot`, `ExecutionReceipt`,
//! `ApplicationExecuted`, or any `neo-core` type: the concrete handler bodies
//! are supplied as closures by the wiring lane (task 89), which is the only place
//! allowed to translate a [`StageContext`] into the legacy
//! `(system, block, snapshot, application_executed_list)` arguments.

use neo_ledger::persistence::{CommitContext, CommitParticipant, StageContext};
use std::fmt;

/// Boxed *committing* handler body: fallible precommit vote (step 12).
///
/// The `String` is the participant's own failure message, lifted verbatim into
/// `LedgerError::ParticipantCommit` by the persistence operation. A wrapped
/// legacy handler preserves its exact error text by returning `err.to_string()`.
type PrecommitHandler = dyn for<'ctx> Fn(&StageContext<'ctx>) -> Result<(), String> + Send + Sync;

/// Boxed *committed* handler body: infallible post-commit finalization (step 15a).
type CommittedHandler = dyn for<'ctx> Fn(&CommitContext<'ctx>) + Send + Sync;

/// Boxed *rollback* handler body: infallible reverse-order unwind (steps 12/13/14).
type DiscardHandler = dyn for<'ctx> Fn(&StageContext<'ctx>) + Send + Sync;

/// Adapter turning a committing/committed handler pair into a [`CommitParticipant`].
///
/// The bridge stores the wrapped handler bodies as type-erased closures so it is
/// a single concrete type registrable as `Arc<dyn CommitParticipant>`, while
/// staying free of any `neo-core` dependency.
///
/// * [`precommit`](CommitParticipant::precommit) delegates to the *committing*
///   handler and returns its vote.
/// * [`committed`](CommitParticipant::committed) delegates to the *committed*
///   handler.
/// * [`discard`](CommitParticipant::discard) delegates to the *rollback* handler
///   when one is attached; because the legacy pair exposes **no** rollback, the
///   default is a no-op (see [`HandlerParticipantBridge::with_rollback`]).
/// * [`run_during_fast_sync`](CommitParticipant::run_during_fast_sync) mirrors
///   the wrapped handler's fast-sync opt-in (default `false`).
///
/// # Example
///
/// ```rust,ignore
/// // Constructed by the task-89 wiring lane, which is allowed to name neo-core:
/// let participant = HandlerParticipantBridge::new(
///     // committing -> precommit
///     |ctx: &StageContext<'_>| {
///         let app_executed = neo_system_adapter::project_receipts(ctx.receipts);
///         handler.blockchain_committing_handler(system, ctx.block, ctx.snapshot, &app_executed);
///         Ok(())
///     },
///     // committed -> committed
///     |ctx: &CommitContext<'_>| {
///         handler.blockchain_committed_handler(system, ctx.block);
///     },
/// )
/// .with_fast_sync(handler.run_during_fast_sync());
///
/// registry.register(Arc::new(participant) as Arc<dyn CommitParticipant>);
/// ```
pub struct HandlerParticipantBridge {
    committing: Box<PrecommitHandler>,
    committed: Box<CommittedHandler>,
    rollback: Option<Box<DiscardHandler>>,
    fast_sync: bool,
}

impl HandlerParticipantBridge {
    /// Wraps a committing/committed handler pair.
    ///
    /// * `committing` becomes the [`precommit`](CommitParticipant::precommit) body.
    /// * `committed` becomes the [`committed`](CommitParticipant::committed) body.
    /// * [`discard`](CommitParticipant::discard) is a no-op until a rollback body
    ///   is attached with [`with_rollback`](Self::with_rollback).
    /// * The fast-sync gate defaults to `false`.
    pub fn new<F, G>(committing: F, committed: G) -> Self
    where
        F: for<'ctx> Fn(&StageContext<'ctx>) -> Result<(), String> + Send + Sync + 'static,
        G: for<'ctx> Fn(&CommitContext<'ctx>) + Send + Sync + 'static,
    {
        Self {
            committing: Box::new(committing),
            committed: Box::new(committed),
            rollback: None,
            fast_sync: false,
        }
    }

    /// Attaches the *rollback* handler mapped to [`discard`](CommitParticipant::discard).
    ///
    /// Use this when the wrapped collaborator buffers staged state that must be
    /// released if the block is abandoned between `precommit` and `committed`
    /// (a failed vote, a failed store commit, or a failed cache update triggers
    /// reverse-order discard).
    pub fn with_rollback<D>(mut self, rollback: D) -> Self
    where
        D: for<'ctx> Fn(&StageContext<'ctx>) + Send + Sync + 'static,
    {
        self.rollback = Some(Box::new(rollback));
        self
    }

    /// Sets the fast-sync opt-in gate reported by
    /// [`run_during_fast_sync`](CommitParticipant::run_during_fast_sync).
    ///
    /// Mirrors the wrapped legacy handler's `run_during_fast_sync`. Returning
    /// `true` keeps the participant active during fast-sync; `false` (default)
    /// skips `precommit` under fast-sync while `committed` still runs.
    pub fn with_fast_sync(mut self, run_during_fast_sync: bool) -> Self {
        self.fast_sync = run_during_fast_sync;
        self
    }

    /// Returns `true` when a rollback body is attached (i.e. `discard` is not a
    /// no-op).
    pub fn has_rollback(&self) -> bool {
        self.rollback.is_some()
    }
}

impl CommitParticipant for HandlerParticipantBridge {
    fn run_during_fast_sync(&self) -> bool {
        self.fast_sync
    }

    fn precommit(&self, context: &StageContext<'_>) -> Result<(), String> {
        (self.committing)(context)
    }

    fn discard(&self, context: &StageContext<'_>) {
        // Absent rollback -> no-op; the legacy committing/committed pair has no
        // unwind hook, so staged state is simply dropped with the context.
        if let Some(rollback) = &self.rollback {
            rollback(context);
        }
    }

    fn committed(&self, context: &CommitContext<'_>) {
        (self.committed)(context);
    }
}

impl fmt::Debug for HandlerParticipantBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandlerParticipantBridge")
            .field("precommit", &"<committing handler>")
            .field("committed", &"<committed handler>")
            .field(
                "discard",
                &if self.has_rollback() {
                    "<rollback handler>"
                } else {
                    "<no-op>"
                },
            )
            .field("fast_sync", &self.fast_sync)
            .finish()
    }
}

// The bridge holds only `Send + Sync + 'static` boxed closures, so it satisfies
// the `CommitParticipant: Send + Sync + 'static` supertraits. Pin it here so a
// regression in the handler bounds fails to compile rather than at registration.
const _: fn() = || {
    fn assert_participant<T: CommitParticipant>() {}
    assert_participant::<HandlerParticipantBridge>();
};
