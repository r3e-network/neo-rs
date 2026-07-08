//! dBFT validator quorum and speaker role helpers.
//!
//! These methods keep the Neo N3 quorum formulas in one place: validator count,
//! `F`, `M`, primary selection, response thresholds, and the invalid
//! transaction skip threshold used by the primary while building a proposal.

use neo_primitives::UInt256;

use super::ConsensusContext;

impl ConsensusContext {
    /// Returns the number of validators in this consensus round.
    #[must_use]
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Returns the number of faulty nodes tolerated: `F = (N - 1) / 3`.
    #[must_use]
    pub fn f(&self) -> usize {
        self.validator_count().saturating_sub(1) / 3
    }

    /// Returns the number of nodes required for consensus: `M = N - F`.
    #[must_use]
    pub fn m(&self) -> usize {
        self.validator_count().saturating_sub(self.f())
    }

    /// Records that `validator_index` reported `hashes` as invalid.
    ///
    /// This mirrors C# `InvalidTransactions` population in
    /// `ConsensusService.OnMessage` for `TxRejectedByPolicy` / `TxInvalid`
    /// `ChangeView` reasons. Reports accumulate across views and reset on a new
    /// block.
    pub fn record_invalid_transactions(&mut self, validator_index: u8, hashes: &[UInt256]) {
        for hash in hashes {
            self.invalid_transactions
                .entry(*hash)
                .or_default()
                .insert(validator_index);
        }
    }

    /// Transaction hashes that more than `F` validators reported invalid.
    ///
    /// The primary must skip these while building the block, matching C#
    /// `EnsureMaxBlockLimitation`:
    /// `if (InvalidTransactions[hash].Count > F) continue`.
    #[must_use]
    pub fn invalid_tx_hashes_over_f(&self) -> Vec<UInt256> {
        let f = self.f();
        let mut hashes: Vec<_> = self
            .invalid_transactions
            .iter()
            .filter(|(_, reporters)| reporters.len() > f)
            .map(|(hash, _)| *hash)
            .collect();
        hashes.sort_by(|a, b| a.as_bytes().cmp(&b.as_bytes()));
        hashes
    }

    /// Returns the primary (speaker) index for the current view.
    #[must_use]
    pub fn primary_index(&self) -> u8 {
        // Matches C# DBFTPlugin:
        // `p = ((Block.Index - viewNumber) % Validators.Length + Validators.Length) % Validators.Length`.
        let n = self.validator_count() as i64;
        if n == 0 {
            return 0;
        }
        let p = (i64::from(self.block_index) - i64::from(self.view_number)).rem_euclid(n);
        p as u8
    }

    /// Returns true if this node is the primary for the current view.
    #[must_use]
    pub fn is_primary(&self) -> bool {
        self.my_index == Some(self.primary_index())
    }

    /// Returns true if this node is a non-primary validator.
    #[must_use]
    pub fn is_backup(&self) -> bool {
        match self.my_index {
            Some(idx) => idx != self.primary_index(),
            None => false,
        }
    }

    /// Returns true if the round has enough prepare responses for `M` signatures.
    #[must_use]
    pub fn has_enough_prepare_responses(&self) -> bool {
        // Count the primary's implicit response plus explicit backup responses.
        let count = usize::from(self.prepare_request_received) + self.prepare_responses.len();
        count >= self.m()
    }

    /// Returns true if the current view has enough commits for `M` signatures.
    #[must_use]
    pub fn has_enough_commits(&self) -> bool {
        let count = self
            .commits
            .keys()
            .filter(|idx| {
                self.commit_view_numbers
                    .get(idx)
                    .copied()
                    .unwrap_or(self.view_number)
                    == self.view_number
            })
            .count();
        count >= self.m()
    }

    /// Returns true if the round has enough change-view requests for `new_view`.
    ///
    /// Mirrors C# DBFT `CheckExpectedView`: validators with `NewViewNumber >=`
    /// the requested view count toward the threshold.
    #[must_use]
    pub fn has_enough_change_views(&self, new_view: u8) -> bool {
        let count = self
            .change_views
            .values()
            .filter(|(v, _)| *v >= new_view)
            .count();
        count >= self.m()
    }
}
