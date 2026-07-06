//! Validator liveness and view-change acceptance checks.
//!
//! These helpers mirror C# DBFT's committed/failed-node accounting and the
//! `ViewChanging` / `NotAcceptingPayloadsDueToViewChanging` guards used before
//! processing proposal payloads.

use super::ConsensusContext;

impl ConsensusContext {
    /// Updates the last seen message for a validator
    pub fn update_last_seen_message(&mut self, validator_index: u8, block_index: u32) {
        self.last_seen_messages.insert(validator_index, block_index);
    }

    /// Returns the number of validators that have committed (sent Commit messages)
    #[must_use]
    pub fn count_committed(&self) -> usize {
        self.commits.len()
    }

    /// Returns the number of validators that have failed or are lost
    ///
    /// A validator is considered failed if:
    /// - We have no record of messages from them (not in `last_seen_messages`), OR
    /// - Their last seen message was for an old block (< current `block_index` - 1)
    ///
    /// This matches C# `DBFTPlugin`'s `CountFailed` logic:
    /// ```csharp
    /// Validators.Count(p => !LastSeenMessage.TryGetValue(p, out var value) || value < (Block.Index - 1))
    /// ```
    #[must_use]
    pub fn count_failed(&self) -> usize {
        if self.last_seen_messages.is_empty() {
            return 0;
        }

        let threshold = self.block_index.saturating_sub(1);
        self.validators
            .iter()
            .filter(|v| {
                match self.last_seen_messages.get(&v.index) {
                    None => true,                                // No message seen from this validator
                    Some(&last_block) => last_block < threshold, // Last message was too old
                }
            })
            .count()
    }

    /// Returns true if more than F nodes have committed or are lost
    ///
    /// This is a critical check for deciding between recovery and view change.
    /// When (`CountCommitted` + `CountFailed`) > F, it means:
    /// - Either enough nodes have already committed, OR
    /// - Enough nodes have failed that we need recovery to sync state
    ///
    /// In this case, we should request recovery instead of change view to avoid
    /// splitting the network across different views.
    ///
    /// Matches C# `DBFTPlugin`'s `MoreThanFNodesCommittedOrLost`:
    /// ```csharp
    /// public bool MoreThanFNodesCommittedOrLost => (CountCommitted + CountFailed) > F;
    /// ```
    #[must_use]
    pub fn more_than_f_nodes_committed_or_lost(&self) -> bool {
        (self.count_committed() + self.count_failed()) > self.f()
    }

    /// Returns true if this node has requested a view change.
    ///
    /// Mirrors C# `DBFTPlugin` `ViewChanging`:
    /// `!WatchOnly && ChangeViewPayloads[MyIndex]?.NewViewNumber > ViewNumber`.
    #[must_use]
    pub fn view_changing(&self) -> bool {
        let Some(my_index) = self.my_index else {
            return false;
        };
        self.change_views
            .get(&my_index)
            .is_some_and(|(new_view, _)| *new_view > self.view_number)
    }

    /// Returns true when we should not accept certain payloads due to an ongoing view change.
    ///
    /// Mirrors C# `DBFTPlugin` `NotAcceptingPayloadsDueToViewChanging`.
    #[must_use]
    pub fn not_accepting_payloads_due_to_view_changing(&self) -> bool {
        self.view_changing() && !self.more_than_f_nodes_committed_or_lost()
    }
}
