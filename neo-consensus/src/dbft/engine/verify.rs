use neo_crypto::Secp256r1Verify;

use crate::{error::ConsensusError, message::SignedMessage};

use super::DbftEngine;

impl DbftEngine {
    pub(super) fn verify_signature(&self, message: &SignedMessage) -> Result<(), ConsensusError> {
        let validator = self
            .state()
            .validators()
            .get(message.validator)
            .ok_or(ConsensusError::UnknownValidator(message.validator))?;
        let digest = message.digest();
        validator
            .public_key
            .secp256r1_verify(digest.as_ref(), &message.signature)
            .map_err(|_| ConsensusError::InvalidSignature(message.validator))
    }
}
