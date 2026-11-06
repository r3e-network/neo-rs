//! Canonical commit participants for the ledger persistence operation.
//!
//! A [`CommitParticipant`] unifies the two handler roles that the legacy core split
//! across `CommittingHandler` and `CommittedHandler`. Participants are registered in a
//! deterministic order and drive the commit gate:
//!
//! * **stage / precommit (step 12)** — each participant stages its derived state and
//!   votes on the commit in *registration order*. The first `Err` aborts persistence
//!   before the store commit.
//! * **reverse discard (steps 12/13/14 rollback)** — when a precommit vote, the store
//!   commit, or the runtime cache update fails, every *already-staged* participant is
//!   discarded in *reverse registration order*.
//! * **committed (step 15a)** — after a successful store commit and cache/tip update,
//!   every participant finalizes in registration order. This phase is infallible: it
//!   runs for all participants regardless of fast-sync gating and can never fail the
//!   pipeline.
//!
//! The canonical state-service participant (task-107) implements this trait by wrapping
//! `StateStore::update_local_state_root_from_cache` (precommit/stage),
//! `discard_staged_local_state_root` (discard) and `update_local_state_root`
//! (committed).

use super::receipt::ExecutionReceipt;
use neo_protocol::execution::Block;
use neo_storage::persistence::DataCache;

/// Pre-commit staging view handed to a participant during step 12.
///
/// The snapshot is the base overlay accumulating the block's committed state after all
/// transaction overlays have been merged (steps 10a/11). Receipts are the pure
/// execution projections; a participant that needs the full `ApplicationExecuted`
/// schema obtains it from the `neo-system` adapter, never from the ledger.
pub struct StageContext<'a> {
    /// The canonical block being persisted.
    pub block: &'a Block,
    /// Base overlay snapshot carrying the block's merged state changes.
    pub snapshot: &'a DataCache,
    /// Ordered pure execution receipts (OnPersist, per-transaction, PostPersist).
    pub receipts: &'a [ExecutionReceipt],
    /// Whether the pipeline is running in fast-sync mode.
    pub fast_sync: bool,
}

impl<'a> StageContext<'a> {
    /// Height of the block being staged.
    pub fn height(&self) -> u32 {
        self.block.index()
    }
}

/// Post-commit view handed to a participant during the infallible step 15a.
pub struct CommitContext<'a> {
    /// The canonical block that was committed.
    pub block: &'a Block,
    /// Whether the pipeline ran in fast-sync mode.
    pub fast_sync: bool,
}

impl<'a> CommitContext<'a> {
    /// Height of the committed block.
    pub fn height(&self) -> u32 {
        self.block.index()
    }
}

/// A canonical commit participant registered in a deterministic order.
///
/// # Error contract
///
/// [`CommitParticipant::precommit`] returns `Result<(), String>`: the `String` is the
/// participant's *own* failure message. The persistence operation lifts it verbatim
/// into [`super::error::LedgerError::ParticipantCommit`], whose `Display` reproduces the
/// message unchanged. A participant therefore preserves its exact legacy error text
/// (for example `"state service commit handler failed during committing: {err}"`) by
/// returning `err.to_string()`.
///
/// [`CommitParticipant::discard`] and [`CommitParticipant::committed`] are infallible.
pub trait CommitParticipant: Send + Sync + 'static {
    /// Fast-sync opt-in gate.
    ///
    /// Returning `true` keeps this participant active during fast-sync (and, when any
    /// participant opts in, forces detailed execution receipts — step 1). Returning
    /// `false` (the default) skips [`CommitParticipant::precommit`] during fast-sync;
    /// [`CommitParticipant::committed`] still runs unconditionally (step 15a).
    fn run_during_fast_sync(&self) -> bool {
        false
    }

    /// Registration-order stage + precommit vote (step 12).
    ///
    /// `Err(message)` aborts persistence before the store commit and triggers reverse
    /// discard of every already-staged participant.
    fn precommit(&self, context: &StageContext<'_>) -> Result<(), String>;

    /// Reverse-order rollback of a previously staged participant (steps 12/13/14).
    ///
    /// Infallible; the default is a no-op for participants with nothing to unwind.
    fn discard(&self, _context: &StageContext<'_>) {}

    /// Infallible post-commit finalization (step 15a).
    ///
    /// Runs for every participant in registration order regardless of fast-sync
    /// gating; the default is a no-op.
    fn committed(&self, _context: &CommitContext<'_>) {}
}
