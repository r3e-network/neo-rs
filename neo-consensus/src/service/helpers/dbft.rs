use super::super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::{ConsensusError, ConsensusResult};
use neo_primitives::{UInt160, UInt256};

impl ConsensusService {
    pub(in crate::service) fn dbft_sender(&self, validator_index: u8) -> ConsensusResult<UInt160> {
        self.context
            .validators
            .get(validator_index as usize)
            .map(|v| v.script_hash)
            .ok_or(ConsensusError::InvalidValidatorIndex(validator_index))
    }

    pub(in crate::service) fn dbft_unsigned_extensible_bytes(
        &self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<Vec<u8>> {
        use neo_io::BinaryWriter;

        let sender = self.dbft_sender(payload.validator_index)?;
        let message_bytes = payload.to_message_bytes();

        let mut writer = BinaryWriter::new();
        writer
            .write_var_string("dBFT")
            .map_err(|e| ConsensusError::state_error(e.to_string()))?;
        writer
            .write_u32(0)
            .map_err(|e| ConsensusError::state_error(e.to_string()))?;
        writer
            .write_u32(payload.block_index)
            .map_err(|e| ConsensusError::state_error(e.to_string()))?;
        writer
            .write_bytes(&sender.to_bytes())
            .map_err(|e| ConsensusError::state_error(e.to_string()))?;
        writer
            .write_var_bytes(&message_bytes)
            .map_err(|e| ConsensusError::state_error(e.to_string()))?;

        Ok(writer.into_bytes())
    }

    pub(in crate::service) fn dbft_payload_hash(
        &self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<UInt256> {
        let unsigned = self.dbft_unsigned_extensible_bytes(payload)?;
        let hash_bytes = neo_crypto::Crypto::sha256(&unsigned);
        UInt256::from_bytes(&hash_bytes).map_err(|e| {
            ConsensusError::state_error(format!("Failed to compute dBFT payload hash: {e}"))
        })
    }

    pub(in crate::service) fn dbft_sign_data(
        &self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<Vec<u8>> {
        let hash = self.dbft_payload_hash(payload)?;
        let mut data = Vec::with_capacity(4 + 32);
        data.extend_from_slice(&self.network.to_le_bytes());
        data.extend_from_slice(&hash.as_bytes());
        Ok(data)
    }
}
