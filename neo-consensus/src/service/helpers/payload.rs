use super::super::ConsensusEvent;
use super::super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::{ConsensusError, ConsensusMessageType, ConsensusResult};
use tracing::debug;

impl<S> ConsensusService<S>
where
    S: crate::ConsensusSigner,
{
    /// Creates a consensus payload.
    ///
    /// This method is `async` because it calls `self.sign()` which may perform
    /// a blocking HSM/network round-trip.
    pub(in crate::service) async fn create_payload(
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
        if let Ok(sign_data) = self.dbft_sign_data(&payload) {
            match self.sign(&sign_data).await {
                Ok(signature) => payload.set_witness(signature),
                Err(err) => {
                    debug!(error = %err, "Consensus payload signing failed");
                }
            }
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
            .map_err(ConsensusError::ChannelSendError)
    }
}
