use super::super::helpers::{
    compute_header_hash, compute_merkle_root, compute_next_consensus_address,
};
use super::super::ConsensusService;
use crate::context::ConsensusState;
use crate::messages::{
    CommitMessage, ConsensusPayload, PrepareRequestMessage, PrepareResponseMessage,
};
use crate::{ConsensusError, ConsensusMessageType, ConsensusResult};
use tracing::{debug, info, warn};

impl ConsensusService {
    /// Handles PrepareRequest message
    pub(in crate::service) fn on_prepare_request(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        // Verify sender is the primary
        let expected_primary = self.context.primary_index();
        if payload.validator_index != expected_primary {
            return Err(ConsensusError::InvalidPrimary {
                expected: expected_primary,
                got: payload.validator_index,
            });
        }

        // Verify the primary's signature (security fix: matches C# DBFTPlugin)
        let sign_data = self.dbft_sign_data(payload)?;
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "PrepareRequest signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareRequest signature invalid",
            ));
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

        let expected_primary = self.context.primary_index();
        let prepare_request = PrepareRequestMessage::deserialize_body(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;
        prepare_request.validate(expected_primary)?;

        // Mark prepare request as received and store proposal fields.
        self.context.prepare_request_received = true;
        self.context.version = prepare_request.version;
        self.context.prev_hash = prepare_request.prev_hash;
        self.context.proposed_timestamp = prepare_request.timestamp;
        self.context.nonce = prepare_request.nonce;
        self.context.proposed_tx_hashes = prepare_request.transaction_hashes.clone();

        // Cache PrepareRequest payload hash (ExtensiblePayload.Hash) for PrepareResponse.
        self.context.preparation_hash = Some(self.dbft_payload_hash(payload)?);

        // Calculate block header hash from proposal data (for commit signatures).
        let merkle_root = compute_merkle_root(&self.context.proposed_tx_hashes);
        let next_consensus = compute_next_consensus_address(&self.context.validators);
        let block_hash = compute_header_hash(
            self.context.version,
            self.context.prev_hash,
            merkle_root,
            self.context.proposed_timestamp,
            self.context.nonce,
            self.context.block_index,
            self.context.primary_index(),
            next_consensus,
        );
        self.context.proposed_block_hash = Some(block_hash);

        // Send PrepareResponse
        let preparation_hash = self.context.preparation_hash.unwrap_or_default();
        let response = PrepareResponseMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
            preparation_hash,
        );

        let response_payload =
            self.create_payload(ConsensusMessageType::PrepareResponse, response.serialize())?;
        let my_witness = response_payload.witness.clone();
        self.broadcast(response_payload)?;

        // Add our own response
        self.context
            .add_prepare_response(self.my_index()?, my_witness)?;

        self.check_prepare_responses()?;

        Ok(())
    }

    /// Handles PrepareResponse message
    pub(in crate::service) fn on_prepare_response(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
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
        let sign_data = self.dbft_sign_data(payload)?;
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "PrepareResponse signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareResponse signature invalid",
            ));
        }

        // Verify PreparationHash matches the primary PrepareRequest payload hash (C# behavior).
        if let Some(expected) = self.context.preparation_hash {
            let msg = PrepareResponseMessage::deserialize_body(
                &payload.data,
                payload.block_index,
                payload.view_number,
                payload.validator_index,
            )?;
            msg.validate(&expected)?;
        }

        // Add the response
        self.context
            .add_prepare_response(payload.validator_index, payload.witness.clone())?;

        self.check_prepare_responses()?;

        Ok(())
    }

    /// Checks if we have enough prepare responses to send commit
    pub(in crate::service) fn check_prepare_responses(&mut self) -> ConsensusResult<()> {
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
            self.my_index()?,
            signature.clone(),
        );

        let payload = self.create_payload(ConsensusMessageType::Commit, commit.serialize())?;
        self.broadcast(payload)?;

        // Add our own commit
        self.context.add_commit(self.my_index()?, signature)?;

        self.check_commits()?;

        Ok(())
    }
}
