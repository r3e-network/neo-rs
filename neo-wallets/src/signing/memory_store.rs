//! Test-only in-memory implementation of signing persistence.

use super::error::{SignError, SignResult};
use super::state::{BeginCheck, SigningState};
use super::store::{
    BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation, SigningAuditEvent,
    SigningRequestStatus, SigningStore,
};
use super::types::SignResponse;
use async_trait::async_trait;
use parking_lot::Mutex;

/// In-memory state store available only to this crate's unit tests.
///
/// Every rule lives in [`SigningState`]; this store only supplies the mutex and
/// the audit log, so tests exercise exactly the policy the durable store uses.
#[derive(Debug, Default)]
pub(super) struct InMemorySigningStore {
    state: Mutex<SigningState>,
    audits: Mutex<Vec<SigningAuditEvent>>,
}

impl InMemorySigningStore {
    pub(super) fn audits(&self) -> Vec<SigningAuditEvent> {
        self.audits.lock().clone()
    }
}

#[async_trait]
impl SigningStore for InMemorySigningStore {
    async fn begin(&self, request: BeginSigningRequest) -> SignResult<BeginSigningOutcome> {
        let mut state = self.state.lock();
        match state.check_begin(&request)? {
            BeginCheck::Existing(outcome) => Ok(outcome),
            BeginCheck::Reserve(plan) => {
                state.apply_begin(request, plan);
                Ok(BeginSigningOutcome::Acquired)
            }
        }
    }

    async fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus> {
        self.state
            .lock()
            .status(request_id, provider, request_digest)
    }

    async fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>> {
        self.state.lock().load(request_id)
    }

    async fn complete(&self, request_id: &str, response: SignResponse) -> SignResult<()> {
        let mut state = self.state.lock();
        state.check_completion(request_id, &response)?;
        state.apply_completion(request_id, response);
        Ok(())
    }

    async fn fail(&self, request_id: &str, error: SignError) -> SignResult<()> {
        let mut state = self.state.lock();
        state.check_failure(request_id)?;
        state.apply_failure(request_id, error);
        Ok(())
    }

    async fn append_audit(&self, event: SigningAuditEvent) -> SignResult<()> {
        self.audits.lock().push(event);
        Ok(())
    }
}
