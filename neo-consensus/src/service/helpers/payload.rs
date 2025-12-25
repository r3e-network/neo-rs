use super::super::ConsensusEvent;
use super::super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::{ConsensusError, ConsensusMessageType, ConsensusResult};

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

        // Sign the payload as an ExtensiblePayload ("dBFT") IVerifiable:
        // signature is over `[network:4][payload_hash:32]`.
        if let Ok(sign_data) = self.dbft_sign_data(&payload) {
            let signature = self.sign(&sign_data);
            payload.set_witness(signature);
        }

        Ok(payload)
    }

    /// Broadcasts a consensus payload
    pub(in crate::service) fn broadcast(&self, payload: ConsensusPayload) -> ConsensusResult<()> {
        self.send_event(ConsensusEvent::BroadcastMessage(payload))
    }

    /// Sends an event
    pub(in crate::service) fn send_event(&self, event: ConsensusEvent) -> ConsensusResult<()> {
        self.event_tx
            .try_send(event)
            .map_err(|e| ConsensusError::ChannelError(e.to_string()))
    }
}
