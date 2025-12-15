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

/// Events emitted by the consensus service
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    /// Block has been committed
    BlockCommitted {
        block_index: u32,
        block_hash: UInt256,
        signatures: Vec<(u8, Vec<u8>)>,
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
    RequestTransactions {
        block_index: u32,
        max_count: usize,
    },
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
    /// Private key for signing (simplified - in production use secure key management)
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

        // Validate block index
        if payload.block_index != self.context.block_index {
            // Could be a message for a future block - ignore for now
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
        // In production, properly deserialize the payload.data
        self.context.prepare_request_received = true;

        // Calculate block hash (simplified - in production compute actual hash)
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
        if self.context.prepare_responses.contains_key(&payload.validator_index) {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received PrepareResponse"
        );

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
            "Block committed!"
        );

        self.context.state = ConsensusState::Committed;

        let block_hash = self.context.proposed_block_hash.unwrap_or_default();
        let signatures = self.context.collect_commit_signatures();

        self.send_event(ConsensusEvent::BlockCommitted {
            block_index: self.context.block_index,
            block_hash,
            signatures,
        })?;

        self.running = false;

        Ok(())
    }

    /// Handles ChangeView message
    fn on_change_view(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        // Parse the new view from the message
        // In production, properly deserialize
        let new_view = payload.view_number + 1;
        let timestamp = current_timestamp();

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            new_view,
            "Received ChangeView"
        );

        self.context.add_change_view(
            payload.validator_index,
            new_view,
            ChangeViewReason::Timeout,
            timestamp,
        )?;

        // Check if we have enough change view requests
        if self.context.has_enough_change_views(new_view) {
            self.change_view(new_view, timestamp)?;
        }

        Ok(())
    }

    /// Requests a view change
    fn request_change_view(
        &mut self,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        let new_view = self.context.view_number + 1;

        warn!(
            block_index = self.context.block_index,
            current_view = self.context.view_number,
            new_view,
            ?reason,
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

    /// Changes to a new view
    fn change_view(&mut self, new_view: u8, timestamp: u64) -> ConsensusResult<()> {
        let old_view = self.context.view_number;

        info!(
            block_index = self.context.block_index,
            old_view,
            new_view,
            "Changing view"
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

        let payload = self.create_payload(ConsensusMessageType::RecoveryMessage, recovery.serialize());
        self.broadcast(payload)?;

        Ok(())
    }

    /// Handles RecoveryMessage
    fn on_recovery_message(&mut self, payload: &ConsensusPayload) -> ConsensusResult<()> {
        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received RecoveryMessage"
        );

        // In production, parse and apply the recovery state
        // This is simplified for now

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

    /// Signs data with the private key
    fn sign(&self, data: &[u8]) -> Vec<u8> {
        // Simplified - in production use proper ECDSA signing
        use neo_crypto::Crypto;
        let hash = Crypto::sha256(data);
        // Return hash as placeholder signature
        hash.to_vec()
    }

    /// Signs a block hash
    fn sign_block_hash(&self, hash: &UInt256) -> Vec<u8> {
        self.sign(&hash.as_bytes())
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

/// Computes a block hash (simplified)
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
}
