use super::super::helpers::current_timestamp;
use super::super::helpers::invocation_script_from_signature;
use super::super::{ConsensusEvent, ConsensusService};
use crate::messages::{ChangeViewMessage, ConsensusPayload, RecoveryRequestMessage};
use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use tracing::{debug, info, warn};

impl ConsensusService {
    /// Handles ChangeView message
    pub(in crate::service) fn on_change_view(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        // Verify the payload signature (security fix: matches C# DBFTPlugin)
        let sign_data = self.dbft_sign_data(payload)?;
        if !payload.witness.is_empty()
            && !self.verify_signature(&sign_data, &payload.witness, payload.validator_index)
        {
            warn!(
                validator = payload.validator_index,
                "ChangeView signature verification failed"
            );
            return Err(crate::ConsensusError::signature_failed(
                "ChangeView signature invalid",
            ));
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

        let new_view = change_view_msg.new_view_number()?;
        let timestamp = change_view_msg.timestamp;
        let reason = change_view_msg.reason;

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            new_view,
            ?reason,
            "Received ChangeView"
        );

        // If the ChangeView targets a view we already passed, treat it as a recovery request.
        if new_view <= self.context.view_number {
            self.maybe_send_recovery_response(payload.validator_index)?;
        }

        let commit_sent = self
            .context
            .my_index
            .and_then(|idx| self.context.commits.get(&idx))
            .is_some();
        if commit_sent {
            return Ok(());
        }

        if let Some((expected_view, _)) = self.context.change_views.get(&payload.validator_index) {
            if new_view <= *expected_view {
                return Ok(());
            }
        }

        self.context
            .add_change_view(payload.validator_index, new_view, reason, timestamp)?;
        if !payload.witness.is_empty() {
            self.context.change_view_invocations.insert(
                payload.validator_index,
                invocation_script_from_signature(&payload.witness),
            );
        }

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
    pub fn request_change_view(
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
        self.context
            .add_change_view(self.my_index()?, new_view, reason, timestamp)?;

        // Broadcast ChangeView message
        let msg = ChangeViewMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
            timestamp,
            reason,
        );

        let payload = self.create_payload(ConsensusMessageType::ChangeView, msg.serialize())?;
        if !payload.witness.is_empty() {
            self.context.change_view_invocations.insert(
                self.my_index()?,
                invocation_script_from_signature(&payload.witness),
            );
        }
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
    pub fn request_recovery(&mut self) -> ConsensusResult<()> {
        let timestamp = current_timestamp();

        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            "Sending RecoveryRequest"
        );

        let recovery_request = RecoveryRequestMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
            timestamp,
        );

        let payload = self.create_payload(
            ConsensusMessageType::RecoveryRequest,
            recovery_request.serialize(),
        )?;
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
}
