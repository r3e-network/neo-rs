use super::*;
impl ConsensusContext {
    /// Marks the proposed transaction set as locally present and verified.
    ///
    /// The upper layer must call this only after validating every hash in
    /// `proposed_tx_hashes` (C# checks `Transactions.ContainsKey` for all of
    /// them before signing or assembling a block).
    pub fn mark_transactions_available(&mut self) {
        self.transactions_available = true;
    }

    /// Records transactions rejected by a `ChangeView` sender.
    ///
    /// SECURITY: a single validator may contribute at most
    /// [`MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK`] distinct hashes per
    /// block. Without that budget a Byzantine validator could keep sending
    /// `ChangeView` messages carrying fresh hashes to grow
    /// [`Self::rejected_tx_validators`] without bound. Re-reporting a hash that
    /// is already attributed to the sender is free and does not consume budget.
    ///
    /// Returns the number of hashes newly attributed to `validator_index` and
    /// whether the sender's budget was exhausted before every offered hash was
    /// recorded.
    pub fn record_rejected_hashes(
        &mut self,
        validator_index: u8,
        hashes: &[UInt256],
    ) -> (usize, bool) {
        let mut recorded = 0;
        let mut exhausted = false;
        for hash in hashes {
            let already_attributed = self
                .rejected_tx_validators
                .get(hash)
                .is_some_and(|validators| validators.contains(&validator_index));
            if already_attributed {
                continue;
            }
            let used = self
                .rejected_hash_counts
                .get(&validator_index)
                .copied()
                .unwrap_or(0);
            if used >= MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK {
                exhausted = true;
                break;
            }
            self.rejected_tx_validators
                .entry(*hash)
                .or_default()
                .insert(validator_index);
            self.rejected_hash_counts.insert(validator_index, used + 1);
            recorded += 1;
        }
        (recorded, exhausted)
    }

    /// Resets the context for a new view
    pub fn reset_for_new_view(&mut self, new_view: u8, timestamp: u64) {
        self.view_number = new_view;
        self.view_start_time = timestamp;
        self.timer_timeout = None;
        self.proposal_requested = false;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear proposal data
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.transactions_available = false;
        self.nonce = 0;

        // Clear signatures
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.prepare_request_invocation = None;
        // Keep change_views for recovery
    }

    /// Resets the context for a new block
    pub fn reset_for_new_block(&mut self, block_index: u32, timestamp: u64) {
        self.block_index = block_index;
        self.view_number = 0;
        self.view_start_time = timestamp;
        self.timer_timeout = None;
        self.proposal_requested = false;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear all data
        self.version = 0;
        self.prev_hash = UInt256::zero();
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.transactions_available = false;
        self.rejected_tx_validators.clear();
        self.rejected_hash_counts.clear();
        self.nonce = 0;
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.commits.clear();
        self.commit_view_numbers.clear();
        self.change_views.clear();
        self.prepare_request_invocation = None;
        self.change_view_invocations.clear();
        self.commit_invocations.clear();
        self.last_change_view_timestamps.clear();
        // Preserve C# LastSeenMessage history across blocks. On first setup,
        // initialize every validator to the previous block so they are not
        // treated as failed before the round has received any messages.
        let previous_block = block_index.saturating_sub(1);
        for validator in &self.validators {
            self.last_seen_messages
                .entry(validator.index)
                .or_insert(previous_block);
        }

        // Clear message hash caches at the block boundary.
        self.seen_message_hashes.clear();
        self.recovery_response_hashes.clear();
        self.local_commit_persistence = None;
    }

    /// Adds a prepare response invocation script
    pub fn add_prepare_response(
        &mut self,
        validator_index: u8,
        invocation_script: Vec<u8>,
        preparation_hash: Option<UInt256>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.prepare_responses
            .insert(validator_index, invocation_script);
        if let Some(hash) = preparation_hash {
            self.prepare_response_hashes.insert(validator_index, hash);
        }
        Ok(())
    }

    /// Adds a commit signature
    pub fn add_commit(
        &mut self,
        validator_index: u8,
        view_number: u8,
        signature: Vec<u8>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        if let Some(existing_view) = self.commit_view_numbers.get(&validator_index)
            && *existing_view == self.view_number
        {
            return Err(ConsensusError::AlreadyReceived(validator_index));
        }
        self.commits.insert(validator_index, signature);
        self.commit_view_numbers
            .insert(validator_index, view_number);
        Ok(())
    }

    /// Adds a change view request
    pub fn add_change_view(
        &mut self,
        validator_index: u8,
        new_view: u8,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.change_views
            .insert(validator_index, (new_view, reason));
        self.last_change_view_timestamps
            .insert(validator_index, timestamp);
        Ok(())
    }

    /// Changes the active timer, matching C# `ChangeTimer`.
    ///
    /// The timer is represented by its start timestamp and duration so callers
    /// can deterministically reschedule it without relying on wall-clock state.
    pub fn change_timer(&mut self, timestamp: u64, timeout: u64) {
        self.view_start_time = timestamp;
        self.timer_timeout = Some(timeout);
    }

    /// Marks the primary proposal request as dispatched after the initial timer.
    pub fn mark_proposal_requested(&mut self) {
        self.proposal_requested = true;
    }

    /// Reschedules the timer for a prospective view during a view change.
    pub fn change_timer_for_view(&mut self, timestamp: u64, view_number: u8) {
        let base = if self.expected_block_time > 0 {
            self.expected_block_time
        } else {
            DEFAULT_BLOCK_TIME_MS
        };
        let shift = (u32::from(view_number) + 1).min(63);
        self.change_timer(timestamp, base.saturating_mul(1u64 << shift));
    }

    /// Extends the active timer by a fraction of a block interval.
    ///
    /// This matches C# `ExtendTimerByFactor`: elapsed time is removed from the
    /// existing delay, then `factor * block_time / M` is added. A timer is never
    /// shortened by this operation.
    pub fn extend_timer_by_factor(&mut self, current_time: u64, factor: u64) {
        // C# only extends an active normal-round timer. Once this node has
        // requested a view change or sent Commit, the deadline is governed by
        // the recovery/view-change path and must remain unchanged.
        if self.view_changing()
            || self
                .my_index
                .is_some_and(|idx| self.commits.contains_key(&idx))
        {
            return;
        }
        let elapsed = current_time.saturating_sub(self.view_start_time);
        let current_timeout = self.get_timeout();
        let extension = self
            .expected_block_time
            .saturating_mul(factor)
            .checked_div(self.m().max(1) as u64)
            .unwrap_or(0);
        // Keep the original deadline and move it forward by the extension.
        // If that deadline has already elapsed, C# leaves the timer untouched.
        let next_deadline = current_timeout.saturating_add(extension);
        if next_deadline > elapsed {
            self.change_timer(current_time, next_deadline - elapsed);
        }
    }

    /// Updates the last seen message for a validator
    pub fn update_last_seen_message(&mut self, validator_index: u8, block_index: u32) {
        self.last_seen_messages.insert(validator_index, block_index);
    }

    /// Marks a message hash as seen (replay attack prevention)
    ///
    /// This method adds the message hash to the cache to prevent duplicate processing.
    /// The cache is automatically cleared when starting a new block via `reset_for_new_block()`.
    ///
    /// Security: uses a bounded LRU cache (`MAX_MESSAGE_CACHE_SIZE`) to prevent memory
    /// exhaustion attacks while avoiding a clear-all window for recently seen messages.
    ///
    /// # Arguments
    /// * `hash` - The message hash to mark as seen
    pub fn mark_message_seen(&mut self, hash: &UInt256) {
        if self.seen_message_hashes.contains(hash) {
            return;
        }
        self.seen_message_hashes.put(*hash, ());
    }

    /// Marks `hash` as verification-rejected at timestamp `now` (ms).
    ///
    /// Fail-closed capacity behavior: the cache is hard-bounded by LRU
    /// eviction at [`MAX_REJECTED_MESSAGE_CACHE_SIZE`]. An evicted hash merely
    /// degrades to the pre-fix behavior of one extra secp256r1 verification
    /// per replay — an attacker can never turn eviction into acceptance of an
    /// invalid message, and memory remains bounded under flood.
    pub fn mark_message_rejected(&mut self, hash: &UInt256, now: u64) {
        if let Some(timestamp) = self.rejected_message_hashes.get_mut(hash) {
            // Rejected again after the TTL expired: slide the window forward.
            *timestamp = now;
        } else {
            self.rejected_message_hashes.put(*hash, now);
        }
    }

    /// Records a recovery response for this request payload.
    pub fn mark_recovery_response_sent(&mut self, hash: &UInt256) {
        self.recovery_response_hashes.put(*hash, ());
    }

    pub(super) fn current_commit_persistence_permit(
        &self,
    ) -> ConsensusResult<Option<CommitPersistencePermit>> {
        let Some(validator_index) = self.my_index else {
            return Ok(None);
        };
        let Some(signature) = self.commits.get(&validator_index) else {
            return Ok(None);
        };
        if self.commit_view_numbers.get(&validator_index).copied() != Some(self.view_number) {
            return Ok(None);
        }
        let Some(proposal_hash) = self.proposed_block_hash else {
            return Err(ConsensusError::Persistence(
                "local Commit has no verified proposal hash".to_string(),
            ));
        };
        let permit = CommitPersistencePermit {
            block_index: self.block_index,
            view_number: self.view_number,
            validator_index,
            proposal_hash,
            signature: signature.clone(),
            recovery_state: self.to_bytes()?,
        };
        permit.validate()?;
        Ok(Some(permit))
    }

    /// Seals the exact post-signing recovery record for later egress checks.
    pub fn seal_commit_persistence_permit(&mut self) -> ConsensusResult<()> {
        self.local_commit_persistence = self.current_commit_persistence_permit()?;
        if self.local_commit_persistence.is_none() {
            return Err(ConsensusError::Persistence(
                "no current-view local Commit is available to seal".to_string(),
            ));
        }
        Ok(())
    }

    /// Saves exact recovery bytes durably using a synchronized temporary file.
    pub fn save_bytes_durable(path: &Path, encoded: &[u8]) -> ConsensusResult<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension("tmp");
        let mut file = File::create(&temp_path)?;
        file.write_all(encoded)?;
        file.sync_all()?;
        drop(file);
        Self::replace_file_durable(&temp_path, path)?;
        let persisted = fs::read(path)?;
        if persisted != encoded {
            return Err(ConsensusError::Persistence(
                "durable recovery file verification failed".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn replace_file_durable(temp_path: &Path, path: &Path) -> std::io::Result<()> {
        fs::rename(temp_path, path)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    #[cfg(windows)]
    fn replace_file_durable(temp_path: &Path, path: &Path) -> std::io::Result<()> {
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(
                existing_file_name: *const u16,
                new_file_name: *const u16,
                flags: u32,
            ) -> i32;
        }

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        let source: Vec<u16> = temp_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let target: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: both paths are valid, NUL-terminated UTF-16 buffers for the
        // duration of the call, and the final pointer is not retained.
        let replaced = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Saves the consensus state to disk for crash recovery
    ///
    /// Uses atomic write (write to temp file + rename) to prevent corruption.
    /// Only saves the essential state needed to resume consensus after a restart.
    ///
    /// # Arguments
    /// * `path` - Path to save the state file
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(ConsensusError)` on IO or serialization failure
    pub fn save(&self, path: &Path) -> ConsensusResult<()> {
        // Create the persisted state from current context
        let state = PersistedConsensusState {
            block_index: self.block_index,
            view_number: self.view_number,
            proposed_block_hash: self.proposed_block_hash,
            preparation_hash: self.preparation_hash,
            proposed_timestamp: self.proposed_timestamp,
            proposed_tx_hashes: self.proposed_tx_hashes.clone(),
            nonce: self.nonce,
            prepare_request_received: self.prepare_request_received,
            prepare_responses: self.prepare_responses.clone(),
            prepare_response_hashes: self.prepare_response_hashes.clone(),
            commits: self.commits.clone(),
            commit_view_numbers: self.commit_view_numbers.clone(),
            change_views: self.change_views.clone(),
            prepare_request_invocation: self.prepare_request_invocation.clone(),
            change_view_invocations: self.change_view_invocations.clone(),
            change_view_timestamps: self.last_change_view_timestamps.clone(),
            commit_invocations: self.commit_invocations.clone(),
        };

        let encoded = Self::encode_state(&state)?;

        Self::save_bytes_durable(path, &encoded)?;

        tracing::debug!(
            "Saved consensus state: block={}, view={}, size={} bytes",
            self.block_index,
            self.view_number,
            encoded.len()
        );

        Ok(())
    }

    /// Serializes the persisted consensus state to bytes.
    pub fn to_bytes(&self) -> ConsensusResult<Vec<u8>> {
        let state = PersistedConsensusState {
            block_index: self.block_index,
            view_number: self.view_number,
            proposed_block_hash: self.proposed_block_hash,
            preparation_hash: self.preparation_hash,
            proposed_timestamp: self.proposed_timestamp,
            proposed_tx_hashes: self.proposed_tx_hashes.clone(),
            nonce: self.nonce,
            prepare_request_received: self.prepare_request_received,
            prepare_responses: self.prepare_responses.clone(),
            prepare_response_hashes: self.prepare_response_hashes.clone(),
            commits: self.commits.clone(),
            commit_view_numbers: self.commit_view_numbers.clone(),
            change_views: self.change_views.clone(),
            prepare_request_invocation: self.prepare_request_invocation.clone(),
            change_view_invocations: self.change_view_invocations.clone(),
            change_view_timestamps: self.last_change_view_timestamps.clone(),
            commit_invocations: self.commit_invocations.clone(),
        };
        Self::encode_state(&state)
    }

    /// Loads the consensus state from disk for crash recovery
    ///
    /// This method loads only the persisted state. The caller must provide:
    /// - `validators`: Current validator list (from chain state)
    /// - `my_index`: This node's validator index (from config)
    ///
    /// The loaded context will have:
    /// - `state`: Set to `Initial` (caller should update based on role)
    /// - `view_start_time`: Set to 0 (caller should update to current time)
    /// - `expected_block_time`: Set to 0 (caller should update)
    /// - `last_change_view_timestamps`: Empty (not persisted)
    ///
    /// # Arguments
    /// * `path` - Path to load the state file from
    /// * `validators` - Current validator list
    /// * `my_index` - This node's validator index
    ///
    /// # Returns
    /// * `Ok(ConsensusContext)` on success
    /// * `Err(ConsensusError)` on IO or deserialization failure
    pub fn load(
        path: &Path,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> ConsensusResult<Self> {
        // Read the file
        let encoded = fs::read(path)?;
        Self::from_bytes(&encoded, validators, my_index)
    }

    /// Deserializes a consensus context from raw bytes.
    pub fn from_bytes(
        encoded: &[u8],
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> ConsensusResult<Self> {
        let state = Self::decode_state(encoded)?;

        tracing::info!(
            "Loaded consensus state: block={}, view={}, prepare_responses={}, commits={}, change_views={}",
            state.block_index,
            state.view_number,
            state.prepare_responses.len(),
            state.commits.len(),
            state.change_views.len()
        );

        let local_commit_persistence = my_index.and_then(|validator_index| {
            let signature = state.commits.get(&validator_index)?.clone();
            let view_number = *state.commit_view_numbers.get(&validator_index)?;
            let proposal_hash = state.proposed_block_hash?;
            (view_number == state.view_number).then(|| CommitPersistencePermit {
                block_index: state.block_index,
                view_number,
                validator_index,
                proposal_hash,
                signature,
                recovery_state: encoded.to_vec(),
            })
        });

        // Reconstruct the full context
        Ok(Self {
            block_index: state.block_index,
            view_number: state.view_number,
            validators,
            my_index,
            state: ConsensusState::Initial, // Caller should update based on role
            // SAFETY: caller MUST call set_view_start_time(current_time) (or
            // equivalent timer initialisation) immediately after from_bytes.
            // Leaving view_start_time at 0 will cause the timeout logic to fire
            // instantly on the first on_timer_tick call.
            view_start_time: 0,
            expected_block_time: 0, // Caller should update
            timer_timeout: None,
            proposal_requested: state.prepare_request_received,
            version: 0,
            prev_hash: UInt256::zero(),
            prev_timestamp: 0,
            max_transactions_per_block: DEFAULT_MAX_TRANSACTIONS_PER_BLOCK,
            proposed_block_hash: state.proposed_block_hash,
            preparation_hash: state.preparation_hash,
            proposed_timestamp: state.proposed_timestamp,
            proposed_tx_hashes: state.proposed_tx_hashes,
            transactions_available: false,
            rejected_tx_validators: HashMap::new(),
            rejected_hash_counts: HashMap::new(),
            nonce: state.nonce,
            prepare_request_received: state.prepare_request_received,
            prepare_responses: state.prepare_responses,
            prepare_response_hashes: state.prepare_response_hashes,
            commits: state.commits,
            commit_view_numbers: state.commit_view_numbers,
            change_views: state.change_views,
            prepare_request_invocation: state.prepare_request_invocation,
            change_view_invocations: state.change_view_invocations,
            commit_invocations: state.commit_invocations,
            last_change_view_timestamps: state.change_view_timestamps,
            last_seen_messages: HashMap::new(), // Not persisted
            seen_message_hashes: Self::new_seen_message_cache(), // Not persisted
            rejected_message_hashes: Self::new_rejected_message_cache(), // Not persisted
            recovery_response_hashes: Self::new_seen_message_cache(), // Not persisted
            local_commit_persistence,
        })
    }

    fn encode_state(state: &PersistedConsensusState) -> ConsensusResult<Vec<u8>> {
        Ok(bincode::serialize(state)?)
    }

    pub(super) fn decode_state(encoded: &[u8]) -> ConsensusResult<PersistedConsensusState> {
        Ok(bincode::deserialize(encoded)?)
    }
}
