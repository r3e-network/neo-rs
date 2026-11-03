use super::super::ConsensusEvent;
use super::super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::{ConsensusError, ConsensusMessageType, ConsensusResult, ConsensusSignContext};
use tracing::warn;

impl ConsensusService {
    /// Creates a consensus payload
    pub(in crate::service) fn create_payload(
        &self,
        msg_type: ConsensusMessageType,
        data: Vec<u8>,
    ) -> ConsensusResult<ConsensusPayload> {
        let mut payload = ConsensusPayload::new(
            self.network,
            self.context.block_index,
            self.my_index()?,
            self.context.view_number,
            msg_type,
            data,
        );

        // Sign the payload as an ExtensiblePayload ("dBFT") Verifiable:
        // signature is over `[network:4][payload_hash:32]`.
        // Fail closed: never return an unsigned payload that callers would still
        // broadcast and count toward local quorum (audit A06).
        let sign_data = self.dbft_sign_data(&payload)?;
        let sign_context = ConsensusSignContext {
            block_index: self.context.block_index,
            view_number: self.context.view_number,
            message_type: msg_type,
            proposal_digest: self.context.proposed_block_hash,
            commit_block_hash: false,
        };
        let signature = self.sign(&sign_data, &sign_context).map_err(|err| {
            warn!(error = %err, "Consensus payload signing failed");
            err
        })?;
        payload.set_witness(signature);

        Ok(payload)
    }

    /// Broadcasts a consensus payload.
    ///
    /// Any payload that can expose this validator's Commit receives an
    /// out-of-band permit bound to the exact persisted recovery record.
    pub(in crate::service) fn broadcast(
        &self,
        mut payload: ConsensusPayload,
    ) -> ConsensusResult<()> {
        if matches!(
            payload.message_type,
            ConsensusMessageType::Commit | ConsensusMessageType::RecoveryMessage
        ) {
            payload.commit_persistence = self.context.commit_persistence_permit()?;
        }
        self.send_event(ConsensusEvent::BroadcastMessage(payload))
    }

    /// Sends an event
    pub(in crate::service) fn send_event(&self, event: ConsensusEvent) -> ConsensusResult<()> {
        self.event_tx
            .try_send(event)
            .map_err(|e| ConsensusError::ChannelSendError(Box::new(e)))
    }
}
