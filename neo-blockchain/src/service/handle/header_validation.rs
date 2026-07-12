//! Blockchain handle header-validation requests.
//!
//! Staged sync uses this typed request/reply boundary to ask the blockchain
//! service to validate and cache a header batch while reporting the accepted
//! prefix and resulting verified frontier.

use neo_payloads::Header;
use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::{BlockchainCommand, HeaderValidationOutcome};

impl BlockchainHandle {
    /// Validate and cache a header batch, returning the accepted prefix count
    /// and resulting verified frontier.
    pub async fn validate_headers(
        &self,
        headers: Vec<Header>,
    ) -> Result<HeaderValidationOutcome, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ValidateHeaders {
                headers,
                reply: reply_tx,
            })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::unavailable("blockchain header validation reply dropped"))
    }
}
