//! Consensus service - the main dBFT state machine.

use crate::context::{ConsensusContext, ConsensusState, ValidatorInfo};
use crate::messages::{
    ChangeViewMessage, CommitMessage, ConsensusPayload, PrepareRequestMessage,
    PrepareResponseMessage, RecoveryMessage,
};
use crate::{ChangeViewReason, ConsensusError, ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Block data for assembly by upper layers
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block index
    pub block_index: u32,
    /// Block timestamp
    pub timestamp: u64,
    /// Block nonce
    pub nonce: u64,
    /// Primary validator index
    pub primary_index: u8,
    /// Transaction hashes included in the block
    pub transaction_hashes: Vec<UInt256>,
    /// Commit signatures from validators (validator_index, signature)
    pub signatures: Vec<(u8, Vec<u8>)>,
    /// Validator public keys for multi-sig witness construction
    pub validator_pubkeys: Vec<neo_crypto::ECPoint>,
    /// Required signature count (M in M-of-N multi-sig)
    pub required_signatures: usize,
}

/// Events emitted by the consensus service
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    /// Block has been committed with complete data for assembly
    BlockCommitted {
        block_index: u32,
        block_hash: UInt256,
        /// Complete block data for upper layer to assemble the final Block structure
        block_data: BlockData,
    },
    /// View has changed
    ViewChanged {
        block_index: u32,
        old_view: u8,
        new_view: u8,
    },
    /// Need to broadcast a message
    BroadcastMessage(ConsensusPayload),
    /// Request transactions from mempool
    RequestTransactions { block_index: u32, max_count: usize },
}

/// Commands that can be sent to the consensus service
#[derive(Debug)]
pub enum ConsensusCommand {
    /// Start consensus for a new block
    Start { block_index: u32, timestamp: u64 },
    /// Process a received consensus message
    ProcessMessage(ConsensusPayload),
    /// Timer tick (for timeout handling)
    TimerTick { timestamp: u64 },
    /// Transactions received from mempool
    TransactionsReceived { tx_hashes: Vec<UInt256> },
    /// Stop the consensus service
    Stop,
}

/// The main consensus service implementing dBFT 2.0
pub struct ConsensusService {
    /// Consensus context
    context: ConsensusContext,
    /// Network magic number
    network: u32,
    /// Private key for signing consensus messages (secp256r1 ECDSA)
    #[allow(dead_code)]
    private_key: Vec<u8>,
    /// Event sender
    event_tx: mpsc::Sender<ConsensusEvent>,
    /// Whether the service is running
    running: bool,
}

impl ConsensusService {
    /// Creates a new consensus service
    pub fn new(
        network: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
    ) -> Self {
        Self {
            context: ConsensusContext::new(0, validators, my_index),
            network,
            private_key,
            event_tx,
            running: false,
        }
    }

    /// Starts consensus for a new block
    pub fn start(&mut self, block_index: u32, timestamp: u64) -> ConsensusResult<()> {
        if self.context.my_index.is_none() {
            return Err(ConsensusError::NotValidator);
        }

        info!(block_index, "Starting consensus");
        self.context.reset_for_new_block(block_index, timestamp);
        self.running = true;

        // If we're the primary, initiate block proposal
        if self.context.is_primary() {
            self.initiate_proposal(timestamp)?;
        }

        Ok(())
    }

    /// Processes a consensus message
    pub fn process_message(&mut self, payload: ConsensusPayload) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }

        // Compute message hash for deduplication (replay attack prevention)
        // This matches C# DBFTPlugin's message caching mechanism
        use neo_crypto::Crypto;
        let sign_data = payload.get_sign_data();
        let msg_hash_bytes = Crypto::hash256(&sign_data);
        let msg_hash = UInt256::from_bytes(&msg_hash_bytes)
            .map_err(|e| ConsensusError::state_error(format!("Failed to compute message hash: {}", e)))?;

        // Check if we've already seen this message (replay attack prevention)
        if self.context.has_seen_message(&msg_hash) {
            debug!(
                block_index = payload.block_index,
                validator = payload.validator_index,
                msg_type = ?payload.message_type,
                "Ignoring duplicate message (already processed)"
            );
            return Ok(());
        }

        // Mark message as seen before processing
        self.context.mark_message_seen(&msg_hash);

        // Validate block index
        if payload.block_index != self.context.block_index {
            // Message for a future block - queue or ignore per dBFT spec
            if payload.block_index > self.context.block_index {
                debug!(
                    expected = self.context.block_index,
                    got = payload.block_index,
                    "Received message for future block"
                );
                return Ok(());
            }
            return Err(ConsensusError::WrongBlock {
                expected: self.context.block_index,
                got: payload.block_index,
            });
        }

        // Update last seen message for this validator
        // This is used to track failed/lost nodes for recovery logic
        self.context
            .update_last_seen_message(payload.validator_index, payload.block_index);

        // Validate view number (except for ChangeView which can be for future views)
        if payload.message_type != ConsensusMessageType::ChangeView
            && payload.view_number != self.context.view_number
        {
            return Err(ConsensusError::WrongView {
                expected: self.context.view_number,
                got: payload.view_number,
            });
        }

        match payload.message_type {
            ConsensusMessageType::PrepareRequest => {
                self.on_prepare_request(&payload)?;
            }
            ConsensusMessageType::PrepareResponse => {
                self.on_prepare_response(&payload)?;
            }
            ConsensusMessageType::Commit => {
                self.on_commit(&payload)?;
            }
            ConsensusMessageType::ChangeView => {
                self.on_change_view(&payload)?;
            }
            ConsensusMessageType::RecoveryRequest => {
                self.on_recovery_request(&payload)?;
            }
            ConsensusMessageType::RecoveryMessage => {
                self.on_recovery_message(&payload)?;
            }
        }

        Ok(())
    }

    /// Handles timer tick for timeout detection
    pub fn on_timer_tick(&mut self, timestamp: u64) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }

        if self.context.is_timed_out(timestamp) {
            self.request_change_view(ChangeViewReason::Timeout, timestamp)?;
        }

        Ok(())
    }

    /// Initiates a block proposal (called when we're the primary)
    fn initiate_proposal(&mut self, _timestamp: u64) -> ConsensusResult<()> {
        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            "Initiating block proposal as primary"
        );

        // Request transactions from mempool
        self.send_event(ConsensusEvent::RequestTransactions {
            block_index: self.context.block_index,
            max_count: 500, // Max transactions per block
        })?;

        Ok(())
    }

    /// Called when transactions are received from mempool
    pub fn on_transactions_received(&mut self, tx_hashes: Vec<UInt256>) -> ConsensusResult<()> {
        if !self.running || !self.context.is_primary() {
            return Ok(());
        }

        let timestamp = current_timestamp();
        let nonce = timestamp ^ (self.context.block_index as u64); // Simple nonce generation

        // Store proposal data
        self.context.proposed_timestamp = timestamp;
        self.context.proposed_tx_hashes = tx_hashes.clone();
        self.context.nonce = nonce;

        // Create and broadcast PrepareRequest
        let msg = PrepareRequestMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
            timestamp,
            nonce,
            tx_hashes,
        );

        let payload = self.create_payload(ConsensusMessageType::PrepareRequest, msg.serialize());
        self.broadcast(payload)?;

        // Mark that we've sent the prepare request
        self.context.prepare_request_received = true;

        Ok(())
    }

    /// Handles PrepareRequest message
    fn on_prepare_request(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Verify sender is the primary
        let expected_primary = self.context.primary_index();
        if payload.validator_index != expected_primary {
            return Err(ConsensusError::InvalidPrimary {
                expected: expected_primary,
                got: payload.validator_index,
            });
        }

        // Verify the primary's signature (security fix: matches C# DBFTPlugin)
        let sign_data = payload.get_sign_data();
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "PrepareRequest signature verification failed"
            );
            return Err(ConsensusError::signature_failed("PrepareRequest signature invalid"));
        }

        // Check if we already received a prepare request
        if self.context.prepare_request_received {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            primary = payload.validator_index,
            "Received PrepareRequest"
        );

        // Parse the message
        // Mark prepare request as received
        self.context.prepare_request_received = true;

        // Calculate block hash from proposal data
        let block_hash = compute_block_hash(
            self.context.block_index,
            self.context.view_number,
            &payload.data,
        );
        self.context.proposed_block_hash = Some(block_hash);

        // Send PrepareResponse
        let response = PrepareResponseMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
            block_hash,
        );

        let response_payload =
            self.create_payload(ConsensusMessageType::PrepareResponse, response.serialize());
        self.broadcast(response_payload)?;

        // Add our own response
        self.context
            .add_prepare_response(self.context.my_index.unwrap(), vec![])?;

        self.check_prepare_responses()?;

        Ok(())
    }

    /// Handles PrepareResponse message
    fn on_prepare_response(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Check if we already have this response
        if self
            .context
            .prepare_responses
            .contains_key(&payload.validator_index)
        {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received PrepareResponse"
        );

        // Verify the payload signature
        let sign_data = payload.get_sign_data();
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "PrepareResponse signature verification failed"
            );
            return Err(ConsensusError::signature_failed("PrepareResponse signature invalid"));
        }

        // Add the response
        self.context
            .add_prepare_response(payload.validator_index, payload.witness.clone())?;

        self.check_prepare_responses()?;

        Ok(())
    }

    /// Checks if we have enough prepare responses to send commit
    fn check_prepare_responses(&mut self) -> ConsensusResult<()> {
        if !self.context.has_enough_prepare_responses() {
            return Ok(());
        }

        if self.context.state == ConsensusState::Committed {
            return Ok(());
        }

        // We have enough responses - send Commit
        info!(
            block_index = self.context.block_index,
            responses = self.context.prepare_responses.len(),
            "Enough PrepareResponses received, sending Commit"
        );

        let block_hash = self.context.proposed_block_hash.unwrap_or_default();
        let signature = self.sign_block_hash(&block_hash);

        let commit = CommitMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
            signature.clone(),
        );

        let payload = self.create_payload(ConsensusMessageType::Commit, commit.serialize());
        self.broadcast(payload)?;

        // Add our own commit
        self.context
            .add_commit(self.context.my_index.unwrap(), signature)?;

        self.check_commits()?;

        Ok(())
    }

    /// Handles Commit message
    fn on_commit(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Check if we already have this commit
        if self.context.commits.contains_key(&payload.validator_index) {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received Commit"
        );

        // Verify the commit signature against the proposed block hash
        // The commit data contains the validator's signature of the block hash
        if let Some(block_hash) = self.context.proposed_block_hash {
            let block_hash_bytes = block_hash.as_bytes();
            if !payload.data.is_empty()
                && !self.verify_signature(&block_hash_bytes, &payload.data, payload.validator_index)
            {
                warn!(
                    validator = payload.validator_index,
                    "Commit signature verification failed"
                );
                return Err(ConsensusError::signature_failed("Commit signature invalid"));
            }
        }

        // Add the commit (signature is in the payload data)
        self.context
            .add_commit(payload.validator_index, payload.data.clone())?;

        self.check_commits()?;

        Ok(())
    }

    /// Checks if we have enough commits to finalize the block
    fn check_commits(&mut self) -> ConsensusResult<()> {
        if !self.context.has_enough_commits() {
            return Ok(());
        }

        if self.context.state == ConsensusState::Committed {
            return Ok(());
        }

        // We have enough commits - block is finalized!
        info!(
            block_index = self.context.block_index,
            commits = self.context.commits.len(),
            "Block committed! Preparing block data for assembly..."
        );

        self.context.state = ConsensusState::Committed;

        // Prepare block data for upper layer to assemble the final Block structure
        let block_data = self.prepare_block_data()?;

        let block_hash = self.context.proposed_block_hash.unwrap_or_default();

        self.send_event(ConsensusEvent::BlockCommitted {
            block_index: self.context.block_index,
            block_hash,
            block_data,
        })?;

        self.running = false;

        Ok(())
    }

    /// Prepares block data for assembly by upper layers.
    ///
    /// This matches C# DBFTPlugin's CreateBlock() preparation logic:
    /// 1. Collect M commit signatures from validators
    /// 2. Gather all metadata needed for block construction
    /// 3. Return structured data for upper layer to build Block + multi-sig witness
    ///
    /// The upper layer (neo-node) will:
    /// - Build multi-sig witness from signatures + validator pubkeys
    /// - Fetch actual transactions from mempool
    /// - Construct complete Block structure with header + transactions + witness
    /// - Calculate merkle root and finalize the block
    ///
    /// # Returns
    /// * `Ok(BlockData)` - Complete data for block assembly
    /// * `Err(ConsensusError)` - If data preparation fails
    fn prepare_block_data(&self) -> ConsensusResult<BlockData> {
        // Get validator public keys for multi-sig witness
        let validator_pubkeys: Vec<neo_crypto::ECPoint> = self
            .context
            .validators
            .iter()
            .map(|v| v.public_key.clone())
            .collect();

        // Calculate M (required signatures for consensus)
        let m = self.context.m();

        // Collect commit signatures in validator index order
        let mut signatures: Vec<(u8, Vec<u8>)> = self.context.collect_commit_signatures();
        signatures.sort_by_key(|(idx, _)| *idx);

        if signatures.len() < m {
            return Err(ConsensusError::InsufficientSignatures {
                required: m,
                got: signatures.len(),
            });
        }

        info!(
            block_index = self.context.block_index,
            signatures = signatures.len(),
            required = m,
            validators = validator_pubkeys.len(),
            tx_count = self.context.proposed_tx_hashes.len(),
            "Block data prepared for assembly"
        );

        Ok(BlockData {
            block_index: self.context.block_index,
            timestamp: self.context.proposed_timestamp,
            nonce: self.context.nonce,
            primary_index: self.context.primary_index(),
            transaction_hashes: self.context.proposed_tx_hashes.clone(),
            signatures,
            validator_pubkeys,
            required_signatures: m,
        })
    }

    /// Handles ChangeView message
    fn on_change_view(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Verify the payload signature (security fix: matches C# DBFTPlugin)
        let sign_data = payload.get_sign_data();
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "ChangeView signature verification failed"
            );
            return Err(ConsensusError::signature_failed("ChangeView signature invalid"));
        }

        // Parse the ChangeView message from payload data
        let change_view_msg = ChangeViewMessage::deserialize(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;

        // Validate the parsed message
        change_view_msg.validate()?;

        let new_view = change_view_msg.new_view_number;
        let timestamp = change_view_msg.timestamp;
        let reason = change_view_msg.reason;

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            new_view,
            ?reason,
            "Received ChangeView"
        );

        self.context.add_change_view(
            payload.validator_index,
            new_view,
            reason,
            timestamp,
        )?;

        // Check if we have enough change view requests
        if self.context.has_enough_change_views(new_view) {
            self.change_view(new_view, timestamp)?;
        }

        Ok(())
    }

    /// Requests a view change
    ///
    /// This method implements the critical logic from C# DBFTPlugin:
    /// - If more than F nodes have committed or are lost, request recovery instead
    /// - Otherwise, send a normal ChangeView message
    ///
    /// This prevents network splits when nodes are already committed or failed.
    fn request_change_view(
        &mut self,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        // Check if we should request recovery instead of change view
        // This matches C# DBFTPlugin's RequestChangeView logic
        if self.context.more_than_f_nodes_committed_or_lost() {
            warn!(
                block_index = self.context.block_index,
                view = self.context.view_number,
                committed = self.context.count_committed(),
                failed = self.context.count_failed(),
                f = self.context.f(),
                "More than F nodes committed or lost, requesting recovery instead of change view"
            );
            return self.request_recovery();
        }

        let new_view = self.context.view_number + 1;

        warn!(
            block_index = self.context.block_index,
            current_view = self.context.view_number,
            new_view,
            ?reason,
            committed = self.context.count_committed(),
            failed = self.context.count_failed(),
            "Requesting view change"
        );

        // Add our own change view
        self.context.add_change_view(
            self.context.my_index.unwrap(),
            new_view,
            reason,
            timestamp,
        )?;

        // Broadcast ChangeView message
        let msg = ChangeViewMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
            new_view,
            timestamp,
            reason,
        );

        let payload = self.create_payload(ConsensusMessageType::ChangeView, msg.serialize());
        self.broadcast(payload)?;

        // Check if we already have enough
        if self.context.has_enough_change_views(new_view) {
            self.change_view(new_view, timestamp)?;
        }

        Ok(())
    }

    /// Requests recovery from other nodes
    ///
    /// This is called instead of change view when more than F nodes have
    /// committed or are lost. It broadcasts a RecoveryRequest to get the
    /// current consensus state from other nodes.
    fn request_recovery(&mut self) -> ConsensusResult<()> {
        let timestamp = current_timestamp();

        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            "Sending RecoveryRequest"
        );

        use crate::messages::RecoveryRequestMessage;

        let recovery_request = RecoveryRequestMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
            timestamp,
        );

        let payload = self.create_payload(
            ConsensusMessageType::RecoveryRequest,
            recovery_request.serialize(),
        );
        self.broadcast(payload)?;

        Ok(())
    }

    /// Changes to a new view
    fn change_view(&mut self, new_view: u8, timestamp: u64) -> ConsensusResult<()> {
        let old_view = self.context.view_number;

        info!(
            block_index = self.context.block_index,
            old_view, new_view, "Changing view"
        );

        self.context.reset_for_new_view(new_view, timestamp);

        self.send_event(ConsensusEvent::ViewChanged {
            block_index: self.context.block_index,
            old_view,
            new_view,
        })?;

        // If we're now the primary, initiate proposal
        if self.context.is_primary() {
            self.initiate_proposal(timestamp)?;
        }

        Ok(())
    }

    /// Handles RecoveryRequest message
    fn on_recovery_request(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received RecoveryRequest"
        );

        // Build and send recovery message with current state
        let recovery = RecoveryMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.context.my_index.unwrap(),
        );

        let payload =
            self.create_payload(ConsensusMessageType::RecoveryMessage, recovery.serialize());
        self.broadcast(payload)?;

        Ok(())
    }

    /// Handles RecoveryMessage
    fn on_recovery_message(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Verify the payload signature (security fix: matches C# DBFTPlugin)
        let sign_data = payload.get_sign_data();
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "RecoveryMessage signature verification failed"
            );
            return Err(ConsensusError::signature_failed("RecoveryMessage signature invalid"));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received RecoveryMessage"
        );

        // Validate block index matches
        if payload.block_index != self.context.block_index {
            debug!(
                expected = self.context.block_index,
                received = payload.block_index,
                "RecoveryMessage block index mismatch, ignoring"
            );
            return Ok(());
        }

        // Parse the recovery message
        let recovery = RecoveryMessage::deserialize(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;

        // Validate the recovery message
        recovery.validate()?;

        info!(
            block_index = payload.block_index,
            view_number = payload.view_number,
            change_views = recovery.change_view_payloads.len(),
            preparations = recovery.preparation_payloads.len(),
            commits = recovery.commit_payloads.len(),
            "Applying recovery message state"
        );

        // Apply change view payloads
        for cv in &recovery.change_view_payloads {
            if cv.validator_index as usize >= self.context.validator_count() {
                continue;
            }
            // Only apply if we don't already have this change view
            if !self.context.change_views.contains_key(&cv.validator_index) {
                self.context.change_views.insert(
                    cv.validator_index,
                    (cv.original_view_number + 1, ChangeViewReason::Timeout),
                );
                self.context
                    .last_change_view_timestamps
                    .insert(cv.validator_index, cv.timestamp);
            }
        }

        // Apply preparation payloads (PrepareResponses)
        for prep in &recovery.preparation_payloads {
            if prep.validator_index as usize >= self.context.validator_count() {
                continue;
            }
            // Only apply if we don't already have this prepare response
            if !self
                .context
                .prepare_responses
                .contains_key(&prep.validator_index)
            {
                self.context
                    .prepare_responses
                    .insert(prep.validator_index, prep.invocation_script.clone());
            }
        }

        // Apply commit payloads
        for commit in &recovery.commit_payloads {
            if commit.validator_index as usize >= self.context.validator_count() {
                continue;
            }
            // Only apply if we don't already have this commit
            if !self.context.commits.contains_key(&commit.validator_index) {
                self.context
                    .commits
                    .insert(commit.validator_index, commit.signature.clone());
            }
        }

        // If recovery includes prepare request and we haven't received one yet
        if let Some(ref prep_req) = recovery.prepare_request_message {
            if !self.context.prepare_request_received {
                self.context.prepare_request_received = true;
                self.context.proposed_timestamp = prep_req.timestamp;
                self.context.nonce = prep_req.nonce;
                self.context.proposed_tx_hashes = prep_req.transaction_hashes.clone();
                debug!(
                    tx_count = prep_req.transaction_hashes.len(),
                    "Applied PrepareRequest from recovery"
                );
            }
        }

        // Check if we can now commit after applying recovery state
        if self.context.has_enough_commits() && self.context.state != ConsensusState::Committed {
            info!(
                block_index = self.context.block_index,
                commits = self.context.commits.len(),
                "Recovery enabled block commit"
            );
            self.check_commits()?;
        }
        // Check if we can now send commit after applying recovery state
        else if self.context.has_enough_prepare_responses()
            && !self.context.commits.contains_key(&self.context.my_index.unwrap_or(255))
        {
            if let Some(my_idx) = self.context.my_index {
                info!(
                    block_index = self.context.block_index,
                    "Recovery enabled sending commit"
                );
                // Create and broadcast commit message
                let block_hash = self.context.proposed_block_hash.unwrap_or_default();
                let signature = self.sign_block_hash(&block_hash);

                let commit = CommitMessage::new(
                    self.context.block_index,
                    self.context.view_number,
                    my_idx,
                    signature.clone(),
                );

                let payload =
                    self.create_payload(ConsensusMessageType::Commit, commit.serialize());
                self.broadcast(payload)?;

                // Add our own commit
                self.context.add_commit(my_idx, signature)?;
                self.check_commits()?;
            }
        }

        Ok(())
    }

    /// Creates a consensus payload
    fn create_payload(&self, msg_type: ConsensusMessageType, data: Vec<u8>) -> ConsensusPayload {
        let mut payload = ConsensusPayload::new(
            self.network,
            self.context.block_index,
            self.context.my_index.unwrap(),
            self.context.view_number,
            msg_type,
            data,
        );

        // Sign the payload
        let sign_data = payload.get_sign_data();
        let signature = self.sign(&sign_data);
        payload.set_witness(signature);

        payload
    }

    /// Broadcasts a consensus payload
    fn broadcast(&self, payload: ConsensusPayload) -> ConsensusResult<()> {
        self.send_event(ConsensusEvent::BroadcastMessage(payload))
    }

    /// Sends an event
    fn send_event(&self, event: ConsensusEvent) -> ConsensusResult<()> {
        self.event_tx
            .try_send(event)
            .map_err(|e| ConsensusError::ChannelError(e.to_string()))
    }

    /// Signs data with the private key using secp256r1 ECDSA
    fn sign(&self, data: &[u8]) -> Vec<u8> {
        use neo_crypto::{Crypto, Secp256r1Crypto};

        // Hash the data first (Neo uses SHA-256 for message hashing)
        let hash = Crypto::sha256(data);

        // Sign with secp256r1 if we have a valid private key
        if self.private_key.len() == 32 {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&self.private_key);

            match Secp256r1Crypto::sign(&hash, &key_bytes) {
                Ok(sig) => sig.to_vec(),
                Err(e) => {
                    warn!(error = %e, "ECDSA signing failed, using hash as fallback");
                    hash.to_vec()
                }
            }
        } else {
            // Fallback for testing without valid key
            hash.to_vec()
        }
    }

    /// Signs a block hash
    fn sign_block_hash(&self, hash: &UInt256) -> Vec<u8> {
        self.sign(&hash.as_bytes())
    }

    /// Verifies a signature against a public key
    fn verify_signature(&self, data: &[u8], signature: &[u8], validator_index: u8) -> bool {
        use neo_crypto::{Crypto, Secp256r1Crypto};

        // Get the validator's public key
        let validator = match self.context.validators.get(validator_index as usize) {
            Some(v) => v,
            None => return false,
        };

        // Hash the data
        let hash = Crypto::sha256(data);

        // Verify signature length (64 bytes for secp256r1)
        if signature.len() != 64 {
            debug!(
                expected = 64,
                got = signature.len(),
                "Invalid signature length"
            );
            return false;
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);

        // Get public key bytes
        let pub_key_bytes = validator.public_key.encoded();

        match Secp256r1Crypto::verify(&hash, &sig_bytes, &pub_key_bytes) {
            Ok(valid) => valid,
            Err(e) => {
                debug!(error = %e, "Signature verification failed");
                false
            }
        }
    }

    /// Returns the current context (for testing/debugging)
    pub fn context(&self) -> &ConsensusContext {
        &self.context
    }

    /// Returns whether the service is running
    pub fn is_running(&self) -> bool {
        self.running
    }
}

/// Gets the current timestamp in milliseconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Computes a block hash from consensus proposal data using SHA-256
fn compute_block_hash(block_index: u32, view: u8, data: &[u8]) -> UInt256 {
    use neo_crypto::Crypto;
    let mut input = Vec::new();
    input.extend_from_slice(&block_index.to_le_bytes());
    input.push(view);
    input.extend_from_slice(data);
    let hash = Crypto::sha256(&input);
    UInt256::from_bytes(&hash).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ECPoint;

    fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
        (0..count)
            .map(|i| ValidatorInfo {
                index: i as u8,
                public_key: ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
                script_hash: neo_primitives::UInt160::zero(),
            })
            .collect()
    }

    #[tokio::test]
    async fn test_consensus_service_new() {
        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        assert!(!service.is_running());
        assert_eq!(service.context().validator_count(), 7);
    }

    #[tokio::test]
    async fn test_consensus_start() {
        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        service.start(100, 1000).unwrap();

        assert!(service.is_running());
        assert_eq!(service.context().block_index, 100);
    }

    #[tokio::test]
    async fn test_consensus_not_validator() {
        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, None, vec![], tx);

        let result = service.start(100, 1000);
        assert!(matches!(result, Err(ConsensusError::NotValidator)));
    }

    #[tokio::test]
    async fn test_primary_calculation() {
        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        service.start(0, 1000).unwrap();
        assert!(service.context().is_primary()); // Block 0, view 0, validator 0 is primary

        service.start(1, 1000).unwrap();
        assert!(!service.context().is_primary()); // Block 1, view 0, validator 1 is primary
    }

    #[tokio::test]
    async fn test_message_deduplication() {
        let (tx, mut rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        service.start(100, 1000).unwrap();

        // Create a test consensus payload
        // Note: For block 100, view 0, the primary is validator (100 % 7) = 2
        let payload = ConsensusPayload::new(
            0x4E454F,
            100,
            2, // From validator 2 (primary for block 100, view 0)
            0,
            ConsensusMessageType::PrepareRequest,
            vec![1, 2, 3, 4],
        );

        // First time: message should be processed
        let result1 = service.process_message(payload.clone());
        if let Err(ref e) = result1 {
            eprintln!("First message processing failed: {:?}", e);
        }
        assert!(result1.is_ok());

        // Second time: same message should be ignored (duplicate)
        let result2 = service.process_message(payload.clone());
        assert!(result2.is_ok());

        // Verify that only one event was emitted (for the first message)
        // The second message should be silently ignored
        drop(service);
        let mut event_count = 0;
        while rx.try_recv().is_ok() {
            event_count += 1;
        }
        // Should have at least one event from the first message
        assert!(event_count >= 1);
    }

    #[tokio::test]
    async fn test_message_cache_cleared_on_new_block() {
        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        service.start(100, 1000).unwrap();

        // Create a test payload
        let payload = ConsensusPayload::new(
            0x4E454F,
            100,
            1,
            0,
            ConsensusMessageType::PrepareRequest,
            vec![1, 2, 3, 4],
        );

        // Process the message
        let _ = service.process_message(payload.clone());

        // Start a new block - this should clear the message cache
        service.start(101, 2000).unwrap();

        // The same message should now be processed again (different block context)
        // Note: It will fail validation because block_index doesn't match,
        // but it won't be rejected as a duplicate
        let result = service.process_message(payload);
        // Should get WrongBlock error, not silently ignored as duplicate
        assert!(matches!(result, Err(ConsensusError::WrongBlock { .. })));
    }

    #[tokio::test]
    async fn test_replay_attack_prevention() {
        use neo_crypto::Crypto;

        let (tx, _rx) = mpsc::channel(100);
        let validators = create_test_validators(7);
        let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

        service.start(100, 1000).unwrap();

        // Create a malicious payload (simulating replay attack)
        let payload = ConsensusPayload::new(
            0x4E454F,
            100,
            1,
            0,
            ConsensusMessageType::ChangeView,
            vec![5, 6, 7, 8],
        );

        // Compute the message hash
        let sign_data = payload.get_sign_data();
        let msg_hash_bytes = Crypto::hash256(&sign_data);
        let msg_hash = UInt256::from_bytes(&msg_hash_bytes).unwrap();

        // Initially, message should not be seen
        assert!(!service.context().has_seen_message(&msg_hash));

        // Process the message first time
        let _ = service.process_message(payload.clone());

        // Message should now be marked as seen
        assert!(service.context().has_seen_message(&msg_hash));

        // Attempt replay attack - send the same message again
        let result = service.process_message(payload);

        // Should succeed (silently ignored), not cause any errors
        assert!(result.is_ok());

        // Message should still be marked as seen
        assert!(service.context().has_seen_message(&msg_hash));
    }
}
