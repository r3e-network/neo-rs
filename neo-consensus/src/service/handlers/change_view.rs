use super::super::helpers::InvocationScript;
use super::super::helpers::current_timestamp;
use super::super::{ConsensusEvent, ConsensusService};
use crate::messages::{ChangeViewMessage, ConsensusPayload, RecoveryRequestMessage};
use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use tracing::{debug, info, warn};

impl ConsensusService {
    /// Handles `ChangeView` message
    pub(in crate::service) async fn on_change_view(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        // Verify the payload signature
        // SECURITY: Require non-empty witness and valid signature
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "ChangeView missing witness"
            );
            return Err(crate::ConsensusError::signature_failed(
                "ChangeView missing witness",
            ));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
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

        // C# `DBFTPlugin` ChangeView carries a `RejectedHashes` UInt256[] for the
        // TxRejectedByPolicy/TxInvalid reasons (see messages/change_view.rs). The
        // parsed hashes are available in `change_view_msg.rejected_hashes`.
        // TODO(dBFT): feed those into context.record_invalid_transactions so the
        // primary can skip over-F-rejected txs (context.invalid_tx_hashes_over_f).

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            new_view,
            ?reason,
            "Received ChangeView"
        );

        // If the ChangeView targets a view we already passed, treat it as a recovery request.
        if new_view <= self.context.view_number {
            self.maybe_send_recovery_response(payload.validator_index)
                .await?;
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
                InvocationScript::invocation_script_from_signature(&payload.witness),
            );
        }

        // Check if we have enough change view requests
        if self.context.has_enough_change_views(new_view) {
            self.change_view(new_view, timestamp).await?;
        }

        Ok(())
    }

    /// Requests a view change
    ///
    /// This method implements the critical logic from C# `DBFTPlugin`:
    /// - If more than F nodes have committed or are lost, request recovery instead
    /// - Otherwise, send a normal `ChangeView` message
    ///
    /// This prevents network splits when nodes are already committed or failed.
    pub async fn request_change_view(
        &mut self,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        let new_view = self.context.view_number + 1;
        self.context.change_view_retry_at =
            Some(timestamp.saturating_add(self.context.change_view_retry_delay()));

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
            return self.request_recovery().await;
        }

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
        // TODO(dBFT): populate rejected hashes from context.invalid_transactions
        // when reason is TxRejectedByPolicy/TxInvalid. An empty array is
        // wire-valid and C#-parseable; the WIRE FORMAT is the P0, population is a
        // follow-up.
        let msg = ChangeViewMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
            timestamp,
            reason,
            Vec::new(),
        );

        let payload = self
            .create_payload(ConsensusMessageType::ChangeView, msg.serialize())
            .await?;
        if !payload.witness.is_empty() {
            self.context.change_view_invocations.insert(
                self.my_index()?,
                InvocationScript::invocation_script_from_signature(&payload.witness),
            );
        }
        self.broadcast(payload)?;

        // Check if we already have enough
        if self.context.has_enough_change_views(new_view) {
            self.change_view(new_view, timestamp).await?;
        }

        Ok(())
    }

    /// Requests recovery from other nodes
    ///
    /// This is called instead of change view when more than F nodes have
    /// committed or are lost. It broadcasts a `RecoveryRequest` to get the
    /// current consensus state from other nodes.
    pub async fn request_recovery(&mut self) -> ConsensusResult<()> {
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

        let payload = self
            .create_payload(
                ConsensusMessageType::RecoveryRequest,
                recovery_request.serialize(),
            )
            .await?;
        self.broadcast(payload)?;

        Ok(())
    }

    /// Changes to a new view
    async fn change_view(&mut self, new_view: u8, timestamp: u64) -> ConsensusResult<()> {
        let old_view = self.context.view_number;

        // Never move the view backward (or re-enter the current view). C#
        // `CheckExpectedView` begins with `if (context.ViewNumber >= viewNumber)
        // return;`. A ChangeView is exempt from the payload view-equality filter,
        // so a stale ChangeView from a lagging node (new_view <= our current view)
        // could otherwise reset us to a past view and re-open a decided round.
        if new_view <= old_view {
            return Ok(());
        }

        info!(
            block_index = self.context.block_index,
            old_view, new_view, "Changing view"
        );

        // C# `CheckExpectedView`: once M agreements are reached, a validating
        // (non-watch-only) node broadcasts its OWN ChangeView(ChangeAgreement)
        // before moving views, UNLESS it already sent a ChangeView for >= new_view
        // (e.g. it initiated this change via request_change_view). Without this a
        // lagging validator jumps views silently and peers never observe its
        // agreement, stalling convergence.
        if let Ok(my_index) = self.my_index() {
            let already_agreed = self
                .context
                .change_views
                .get(&my_index)
                .is_some_and(|(agreed_view, _)| *agreed_view >= new_view);
            if !already_agreed {
                self.broadcast_change_agreement(timestamp).await?;
            }
        }

        self.context.reset_for_new_view(new_view, timestamp);

        self.send_event(ConsensusEvent::ViewChanged {
            block_index: self.context.block_index,
            old_view,
            new_view,
        })?;

        Ok(())
    }

    /// Broadcasts this node's own `ChangeView(ChangeAgreement)` for the current
    /// view (`NewViewNumber = ViewNumber + 1`), mirroring C#
    /// `MakeChangeView(ChangeViewReason.ChangeAgreement)` in `CheckExpectedView`.
    async fn broadcast_change_agreement(&mut self, timestamp: u64) -> ConsensusResult<()> {
        let my_index = self.my_index()?;
        let new_view = self.context.view_number.saturating_add(1);
        self.context.add_change_view(
            my_index,
            new_view,
            ChangeViewReason::ChangeAgreement,
            timestamp,
        )?;
        // ChangeAgreement never carries RejectedHashes (only reasons 0x3/0x4 do).
        let msg = ChangeViewMessage::new(
            self.context.block_index,
            self.context.view_number,
            my_index,
            timestamp,
            ChangeViewReason::ChangeAgreement,
            Vec::new(),
        );
        let payload = self
            .create_payload(ConsensusMessageType::ChangeView, msg.serialize())
            .await?;
        if !payload.witness.is_empty() {
            self.context.change_view_invocations.insert(
                my_index,
                InvocationScript::invocation_script_from_signature(&payload.witness),
            );
        }
        self.broadcast(payload)?;
        Ok(())
    }
}
