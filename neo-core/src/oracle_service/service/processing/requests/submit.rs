use super::super::super::utils::{ledger_height, verify_oracle_signature};
use super::super::super::{OracleService, OracleServiceError};
use crate::cryptography::ECPoint;
use crate::smart_contract::native::{OracleContract, Role, RoleManagement};

impl OracleService {
    pub fn submit_oracle_response(
        &self,
        oracle_pub: ECPoint,
        request_id: u64,
        tx_sign: Vec<u8>,
        msg_sign: Vec<u8>,
    ) -> Result<(), OracleServiceError> {
        if !self.is_running() {
            return Err(OracleServiceError::Disabled);
        }

        if self.finished_cache.lock().contains_key(&request_id) {
            return Err(OracleServiceError::RequestFinished);
        }

        let snapshot = self.snapshot_cache();
        let height = ledger_height(&snapshot);
        let oracles = RoleManagement::new()
            .get_designated_by_role_at(&snapshot, Role::Oracle, height)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        if !oracles.iter().any(|key| key == &oracle_pub) {
            return Err(OracleServiceError::NotDesignated(format!(
                "{} isn't an oracle node.",
                oracle_pub
            )));
        }

        let request = OracleContract::new()
            .get_request(&snapshot, request_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if request.is_none() {
            return Err(OracleServiceError::RequestNotFound);
        }

        let mut message = Vec::with_capacity(oracle_pub.as_bytes().len() + 8 + tx_sign.len());
        message.extend_from_slice(oracle_pub.as_bytes());
        message.extend_from_slice(&request_id.to_le_bytes());
        message.extend_from_slice(&tx_sign);

        if !verify_oracle_signature(&oracle_pub, &message, &msg_sign) {
            return Err(OracleServiceError::InvalidSignature(format!(
                "Invalid oracle response transaction signature from '{}'.",
                oracle_pub
            )));
        }

        self.add_response_tx_sign(&snapshot, request_id, oracle_pub, tx_sign, None, None, None)
    }
}
