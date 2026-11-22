use super::*;
impl ConsensusContext {
    /// Returns the number of validators
    #[must_use]
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Returns the number of faulty nodes tolerated: f = (n-1)/3
    #[must_use]
    pub fn f(&self) -> usize {
        fault_tolerance(self.validator_count())
    }

    /// Returns the number of nodes required for consensus: M = n - f
    #[must_use]
    pub fn m(&self) -> usize {
        required_signatures(self.validator_count())
    }

    /// Returns the primary (speaker) index for the current view
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

    /// Returns true if this node is the primary for the current view
    #[must_use]
    pub fn is_primary(&self) -> bool {
        self.my_index == Some(self.primary_index())
    }

    /// Returns true if this node is a backup (non-primary validator)
    #[must_use]
    pub fn is_backup(&self) -> bool {
        self.my_index.is_some_and(|idx| idx != self.primary_index())
    }

    /// Returns true if we have enough prepare responses (M signatures)
    #[must_use]
    pub fn has_enough_prepare_responses(&self) -> bool {
        // The primary holds exactly one preparation vote: its PrepareRequest
        // is the implicit vote, and an explicit PrepareResponse from the
        // primary occupies the same validator slot (R01 — a malicious primary
        // must not double-count by sending both).
        // Only responses that agree on `preparation_hash` count toward M.
        // A primary PrepareResponse with a divergent hash must not fill the
        // primary slot when PrepareRequest has not been verified locally.
        let Some(expected_hash) = self.preparation_hash else {
            return false;
        };

        let primary_index = self.primary_index();
        let matching_responses = self
            .prepare_response_hashes
            .iter()
            .filter(|(_, hash)| **hash == expected_hash)
            .count();
        let primary_match_in_responses = self
            .prepare_response_hashes
            .get(&primary_index)
            .is_some_and(|hash| *hash == expected_hash);

        // PrepareRequest always votes for `expected_hash` (it is the source of
        // that hash). Add it only when the primary is not already counted via
        // a matching PrepareResponse.
        let count = matching_responses
            + usize::from(self.prepare_request_received && !primary_match_in_responses);
        count >= self.m()
    }

    /// Returns true if this node has verified a proposal and may sign a
    /// `Commit` for it.
    ///
    /// R02: a Commit binds this validator's signature to a specific block
    /// hash. Signing with a default/zero hash when the PrepareRequest has
    /// not been seen and verified would let this node legitimize an
    /// unverified proposal.
    #[must_use]
    pub fn can_sign_commit(&self) -> bool {
        self.proposed_block_hash.is_some()
            && self.preparation_hash.is_some()
            && (self.proposed_tx_hashes.is_empty() || self.transactions_available)
    }

    /// Returns hashes rejected by more than F validators.
    ///
    /// C# uses this set to avoid re-proposing transactions the network has
    /// already rejected, which otherwise burns views indefinitely.
    #[must_use]
    pub fn rejected_hashes_above_f(&self) -> Vec<UInt256> {
        let threshold = self.f();
        self.rejected_tx_validators
            .iter()
            .filter(|(_, validators)| validators.len() > threshold)
            .map(|(hash, _)| *hash)
            .collect()
    }

    /// Returns true when more than F validators rejected `hash`.
    #[must_use]
    pub fn is_rejected_transaction(&self, hash: &UInt256) -> bool {
        self.rejected_tx_validators
            .get(hash)
            .is_some_and(|validators| validators.len() > self.f())
    }

    /// Returns true if we have enough commits (M signatures)
    #[must_use]
    pub fn has_enough_commits(&self) -> bool {
        let count = self
            .commits
            .keys()
            .filter(|idx| {
                match self.commit_view_numbers.get(idx).copied() {
                    Some(v) => v == self.view_number,
                    None => {
                        // A commit without a stored view number is a data
                        // inconsistency — skip rather than silently miscounting.
                        debug_assert!(
                            false,
                            "commit for validator {idx} has no entry in commit_view_numbers"
                        );
                        false
                    }
                }
            })
            .count();
        count >= self.m()
    }

    /// Returns true if we have enough change view requests (M requests).
    /// Matches C# `DBFTPlugin`'s `CheckExpectedView` logic: counts `NewViewNumber` >= requested view.
    #[must_use]
    pub fn has_enough_change_views(&self, new_view: u8) -> bool {
        let count = self
            .change_views
            .values()
            .filter(|(v, _)| *v >= new_view)
            .count();
        count >= self.m()
    }

    /// Gets the timeout duration for the current view.
    #[must_use]
    pub fn get_timeout(&self) -> u64 {
        self.timer_timeout.unwrap_or_else(|| self.base_timeout())
    }

    /// Returns whether the primary proposal request was dispatched.
    #[must_use]
    pub fn proposal_requested(&self) -> bool {
        self.proposal_requested
    }

    /// Returns the timeout used after a primary sends `PrepareRequest`.
    #[must_use]
    pub fn prepare_request_timeout(&self) -> u64 {
        let base = if self.expected_block_time > 0 {
            self.expected_block_time
        } else {
            DEFAULT_BLOCK_TIME_MS
        };
        let shift = (u32::from(self.view_number) + 1).min(63);
        let timeout = base.saturating_mul(1u64 << shift);
        timeout.saturating_sub(if self.view_number == 0 { base } else { 0 })
    }

    /// Gets the timeout before any explicit timer rescheduling.
    fn base_timeout(&self) -> u64 {
        // Use configured expected_block_time when provided (mirrors C# TimePerBlock overrides).
        let base = if self.expected_block_time > 0 {
            self.expected_block_time
        } else {
            DEFAULT_BLOCK_TIME_MS
        };
        // C# dBFT InitializeConsensus: a primary at view 0 starts with a single
        // TimePerBlock after the previous block persisted; every other case
        // (backup, later views) uses TimePerBlock << (viewNumber + 1).
        if self.is_primary() && self.view_number == 0 {
            return base;
        }
        let shift = (self.view_number as u32 + 1).min(63);
        base.saturating_mul(1u64 << shift)
    }

    /// Checks if the current view has timed out.
    #[must_use]
    pub fn is_timed_out(&self, current_time: u64) -> bool {
        current_time > self.view_start_time.saturating_add(self.get_timeout())
    }

    /// Collects all commit signatures for block finalization
    #[must_use]
    pub fn collect_commit_signatures(&self) -> Vec<(u8, Vec<u8>)> {
        self.commits
            .iter()
            .filter(|(idx, _)| {
                self.commit_view_numbers
                    .get(idx)
                    .copied()
                    .unwrap_or(self.view_number)
                    == self.view_number
            })
            .map(|(idx, sig)| (*idx, sig.clone()))
            .collect()
    }

    /// Checks if a message hash has been seen before (replay attack prevention)
    ///
    /// This method is critical for preventing replay attacks where an attacker
    /// could retransmit valid consensus messages to disrupt the protocol.
    ///
    /// # Arguments
    /// * `hash` - The message hash to check
    ///
    /// # Returns
    /// * `true` if the message has been seen before
    /// * `false` if this is a new message
    #[must_use]
    pub fn has_seen_message(&self, hash: &UInt256) -> bool {
        self.seen_message_hashes.contains(hash)
    }

    /// Returns whether `hash` failed signature verification within
    /// [`REJECTED_MESSAGE_TTL_MS`] of `now`.
    ///
    /// Security: this is the CPU-amplification guard for replayed consensus
    /// messages that carry invalid signatures. `peek` (not `get`) is used so a
    /// lookup never refreshes LRU recency and eviction order stays insertion
    /// time (oldest rejection first).
    #[must_use]
    pub fn has_recently_rejected_message(&self, hash: &UInt256, now: u64) -> bool {
        self.rejected_message_hashes
            .peek(hash)
            .is_some_and(|&rejected_at| now.saturating_sub(rejected_at) < REJECTED_MESSAGE_TTL_MS)
    }

    /// Returns whether a recovery response was already sent for this payload.
    #[must_use]
    pub fn has_sent_recovery_response(&self, hash: &UInt256) -> bool {
        self.recovery_response_hashes.contains(hash)
    }

    /// Returns the number of validators that have committed (sent Commit messages)
    #[must_use]
    pub fn count_committed(&self) -> usize {
        self.commits.len()
    }

    /// Returns true once this validator has sent a `Commit` for the round.
    ///
    /// Mirrors C# `DBFTPlugin` `CommitSent`: `CommitPayloads[MyIndex] != null`.
    /// The flag is sticky for the whole round — Commit payloads from earlier
    /// views are retained as recovery evidence — so a node that has committed
    /// never agrees to advance the view, which is what keeps it from signing a
    /// second block at the same height.
    #[must_use]
    pub fn has_sent_commit(&self) -> bool {
        self.my_index
            .is_some_and(|index| self.commits.contains_key(&index))
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
        // A newly constructed context has not started a consensus round yet;
        // once a round has populated this map, missing validators count as failed.
        if self.last_seen_messages.is_empty() {
            return 0;
        }
        let threshold = self.block_index.saturating_sub(1);
        self.validators
            .iter()
            .filter(|v| {
                // No message seen from this validator, or the last message seen
                // was too old, both count as failed.
                self.last_seen_messages
                    .get(&v.index)
                    .is_none_or(|&last_block| last_block < threshold)
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

    /// Builds a publication permit for the local Commit. A sealed earlier-view
    /// permit remains usable for RecoveryMessage relay after a view change.
    pub fn commit_persistence_permit(&self) -> ConsensusResult<Option<CommitPersistencePermit>> {
        if let Some(permit) = self.current_commit_persistence_permit()? {
            return Ok(Some(permit));
        }
        if let Some(permit) = self.local_commit_persistence.as_ref() {
            permit.validate()?;
            return Ok(Some(permit.clone()));
        }
        Ok(None)
    }
}
