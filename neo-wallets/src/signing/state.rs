//! Signing state machine shared by every [`SigningStore`](super::store::SigningStore).
//!
//! Idempotency, replay protection, quota accounting and completion-binding are
//! security-critical rules that must behave identically whether the journal
//! lives in memory (unit tests) or in an append-only file (production). They are
//! implemented once here; a store implementation only decides how a reservation
//! is made durable.
//!
//! Reads never mutate and writes never validate: [`SigningState`] separates the
//! checks (`check_begin`, `check_completion`, `check_failure`) from the matching
//! transitions (`apply_begin`, `apply_completion`, `apply_failure`). A durable
//! store needs that gap to write its journal record before mutating memory,
//! while the journal replay applies transitions without re-running the checks.

use super::error::{SignError, SignErrorCode, SignResult};
use super::store::{
    BeginSigningOutcome, BeginSigningRequest, PersistedSigningOperation, SigningRequestState,
    SigningRequestStatus,
};
use super::types::{AuthenticatedContext, KeyInfo, SignRequest, SignResponse};
use std::collections::HashMap;

/// One reserved signing operation and its current result.
#[derive(Debug, Clone)]
pub(super) struct SigningRecord {
    pub(super) request: SignRequest,
    pub(super) context: AuthenticatedContext,
    pub(super) provider: String,
    pub(super) digest: [u8; 32],
    pub(super) key_info: KeyInfo,
    pub(super) state: SigningRequestState,
    pub(super) response: Option<SignResponse>,
    pub(super) error: Option<SignError>,
}

impl SigningRecord {
    /// Resolves a repeated reservation for an already-known request.
    pub(super) fn existing_outcome(
        &self,
        digest: [u8; 32],
        request_id: &str,
    ) -> SignResult<BeginSigningOutcome> {
        if self.digest != digest {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "request identifier or idempotency key was reused for different content",
            )
            .for_request(request_id.to_string()));
        }
        match self.state {
            SigningRequestState::Succeeded => self
                .response
                .clone()
                .map(BeginSigningOutcome::Cached)
                .ok_or_else(|| corrupt(request_id, "successful signing record has no response")),
            SigningRequestState::Pending => Err(SignError::new(
                SignErrorCode::RequestInProgress,
                "matching signing request is pending or has an unknown result",
            )
            .for_request(request_id.to_string())
            .retryable(true)),
            SigningRequestState::Failed => Err(self
                .error
                .clone()
                .unwrap_or_else(|| corrupt(request_id, "failed signing record has no error"))),
        }
    }

    /// Verifies that a provider response is bound to this exact reservation.
    pub(super) fn response_matches(&self, response: &SignResponse) -> bool {
        response.schema_version == self.request.schema_version
            && response.request_id == self.request.request_id
            && response.key_ref == self.request.key_ref
            && response.algorithm == self.key_info.algorithm
            && response.signed_digest == self.request.payload.signing_digest()
            && response.request_digest == self.digest
            && response.policy_version == self.request.policy_version
    }

    /// Projects the persisted status, rejecting impossible record shapes.
    pub(super) fn status(&self, request_id: &str) -> SignResult<SigningRequestStatus> {
        match self.state {
            SigningRequestState::Pending => Ok(SigningRequestStatus::Pending),
            SigningRequestState::Succeeded => self
                .response
                .clone()
                .map(SigningRequestStatus::Succeeded)
                .ok_or_else(|| corrupt(request_id, "successful signing record has no response")),
            SigningRequestState::Failed => self
                .error
                .clone()
                .map(SigningRequestStatus::Failed)
                .ok_or_else(|| corrupt(request_id, "failed signing record has no error")),
        }
    }

    /// Projects the complete operation for query-only restart recovery.
    pub(super) fn snapshot(&self, request_id: &str) -> SignResult<PersistedSigningOperation> {
        Ok(PersistedSigningOperation {
            request: self.request.clone(),
            context: self.context.clone(),
            request_digest: self.digest,
            key_info: self.key_info.clone(),
            status: self.status(request_id)?,
        })
    }
}

/// The reservation a successful [`SigningState::check_begin`] authorizes.
#[derive(Debug)]
pub(super) struct BeginPlan {
    pub(super) quota_key: (String, i64),
    pub(super) quota_value: u64,
}

/// Outcome of the reservation checks.
pub(super) enum BeginCheck {
    /// The caller owns a new pending reservation and may invoke the provider.
    Reserve(BeginPlan),
    /// A previous reservation for the same request already answers this call.
    Existing(BeginSigningOutcome),
}

/// Records, indexes and counters behind one signing store.
#[derive(Debug, Default)]
pub(super) struct SigningState {
    records: HashMap<String, SigningRecord>,
    idempotency: HashMap<(String, String), String>,
    nonces: HashMap<(String, String), String>,
    quotas: HashMap<(String, i64), u64>,
}

impl SigningState {
    /// Checks deadline, request ID, tenant-scoped idempotency key, replay nonce
    /// and quota, returning either the reservation to apply once it is durable or
    /// the answer an earlier reservation already provides.
    pub(super) fn check_begin(&self, request: &BeginSigningRequest) -> SignResult<BeginCheck> {
        let request_id = request.request.request_id.clone();
        let tenant = request.context.tenant.clone();

        if request.request.key_ref.provider.trim().is_empty() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing request provider must not be empty",
            )
            .for_request(request_id));
        }
        if request.request.deadline_unix_ms <= request.now_unix_ms {
            return Err(SignError::new(
                SignErrorCode::DeadlineExceeded,
                "signing request expired before storage reservation",
            )
            .for_request(request_id));
        }
        // Validated before the window arithmetic below, which divides by it.
        if request.quota.window_ms <= 0 {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing quota window must be positive",
            )
            .for_request(request_id));
        }

        if let Some(record) = self.records.get(&request_id) {
            return record
                .existing_outcome(request.request_digest, &request_id)
                .map(BeginCheck::Existing);
        }

        let idempotency_scope = (tenant.clone(), request.request.idempotency.key.clone());
        if let Some(existing_id) = self.idempotency.get(&idempotency_scope) {
            let record = self.records.get(existing_id).ok_or_else(|| {
                corrupt(
                    &request_id,
                    "idempotency index references a missing signing request",
                )
            })?;
            return record
                .existing_outcome(request.request_digest, &request_id)
                .map(BeginCheck::Existing);
        }

        let nonce_scope = (tenant, request.request.idempotency.replay_nonce.clone());
        if self.nonces.contains_key(&nonce_scope) {
            return Err(SignError::new(
                SignErrorCode::ReplayDetected,
                "tenant replay nonce has already been consumed",
            )
            .for_request(request_id));
        }

        let window_start =
            request.now_unix_ms.div_euclid(request.quota.window_ms) * request.quota.window_ms;
        let quota_key = (request.quota.scope.clone(), window_start);
        let used = self.quotas.get(&quota_key).copied().unwrap_or(0);
        let next = used.checked_add(request.quota.units).ok_or_else(|| {
            SignError::new(
                SignErrorCode::LimitExceeded,
                "signing quota counter overflow",
            )
            .for_request(request_id.clone())
        })?;
        if next > request.quota.limit {
            return Err(SignError::new(
                SignErrorCode::LimitExceeded,
                "signing quota would be exceeded",
            )
            .for_request(request_id)
            .with_detail("quota_limit_units", request.quota.limit.to_string()));
        }

        Ok(BeginCheck::Reserve(BeginPlan {
            quota_key,
            quota_value: next,
        }))
    }

    /// Records a reservation whose checks already passed.
    pub(super) fn apply_begin(&mut self, request: BeginSigningRequest, plan: BeginPlan) {
        self.quotas.insert(plan.quota_key, plan.quota_value);
        self.idempotency.insert(
            (
                request.context.tenant.clone(),
                request.request.idempotency.key.clone(),
            ),
            request.request.request_id.clone(),
        );
        self.nonces.insert(
            (
                request.context.tenant.clone(),
                request.request.idempotency.replay_nonce.clone(),
            ),
            request.request.request_id.clone(),
        );
        self.records.insert(
            request.request.request_id.clone(),
            SigningRecord {
                provider: request.request.key_ref.provider.clone(),
                request: request.request,
                context: request.context,
                digest: request.request_digest,
                key_info: request.key_info,
                state: SigningRequestState::Pending,
                response: None,
                error: None,
            },
        );
    }

    /// Computes the reservation a journal replay records.
    ///
    /// Replay trusts the journal — every rule was enforced when the record was
    /// written — so only the window arithmetic and counter accumulation run
    /// here, and an overflow is reported as absent rather than as a policy error.
    pub(super) fn replay_plan(&self, request: &BeginSigningRequest) -> Option<BeginPlan> {
        let window_start =
            request.now_unix_ms.div_euclid(request.quota.window_ms) * request.quota.window_ms;
        let quota_key = (request.quota.scope.clone(), window_start);
        let quota_value = self
            .quotas
            .get(&quota_key)
            .copied()
            .unwrap_or(0)
            .checked_add(request.quota.units)?;
        Some(BeginPlan {
            quota_key,
            quota_value,
        })
    }

    /// Reads a persisted status after verifying the caller's provider and digest.
    pub(super) fn status(
        &self,
        request_id: &str,
        provider: &str,
        request_digest: [u8; 32],
    ) -> SignResult<SigningRequestStatus> {
        let Some(record) = self.records.get(request_id) else {
            return Ok(SigningRequestStatus::NotFound);
        };
        if record.provider != provider {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "recovery provider does not match persisted signing state",
            )
            .for_request(request_id.to_string()));
        }
        if record.digest != request_digest {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "recovery request digest does not match persisted signing state",
            )
            .for_request(request_id.to_string()));
        }
        record.status(request_id)
    }

    /// Loads the original operation for query-only restart recovery.
    pub(super) fn load(&self, request_id: &str) -> SignResult<Option<PersistedSigningOperation>> {
        match self.records.get(request_id) {
            Some(record) => record.snapshot(request_id).map(Some),
            None => Ok(None),
        }
    }

    /// Borrows one reservation so a journal can report its own corruption
    /// messages while reusing the binding predicate.
    pub(super) fn record(&self, request_id: &str) -> Option<&SigningRecord> {
        self.records.get(request_id)
    }

    /// Verifies that a completion may transition this reservation.
    pub(super) fn check_completion(
        &self,
        request_id: &str,
        response: &SignResponse,
    ) -> SignResult<()> {
        let record = self
            .records
            .get(request_id)
            .ok_or_else(|| corrupt(request_id, "cannot complete an unreserved signing request"))?;
        if record.state != SigningRequestState::Pending {
            return Err(corrupt(
                request_id,
                "only pending signing requests may complete",
            ));
        }
        if !record.response_matches(response) {
            return Err(corrupt(
                request_id,
                "signing response does not match the complete persisted request binding",
            ));
        }
        Ok(())
    }

    /// Marks a reservation successful; [`Self::check_completion`] must have run.
    pub(super) fn apply_completion(&mut self, request_id: &str, response: SignResponse) {
        let record = self
            .records
            .get_mut(request_id)
            .expect("completion is applied only after its checks passed");
        record.state = SigningRequestState::Succeeded;
        record.response = Some(response);
    }

    /// Verifies that a failure may be recorded for this reservation.
    pub(super) fn check_failure(&self, request_id: &str) -> SignResult<()> {
        if self.records.contains_key(request_id) {
            return Ok(());
        }
        Err(corrupt(
            request_id,
            "cannot fail an unreserved signing request",
        ))
    }

    /// Marks a reservation failed; [`Self::check_failure`] must have run.
    pub(super) fn apply_failure(&mut self, request_id: &str, error: SignError) {
        let record = self
            .records
            .get_mut(request_id)
            .expect("failure is applied only after its checks passed");
        record.state = SigningRequestState::Failed;
        record.error = Some(error);
    }
}

/// Builds a corrupt-state error bound to a request identifier.
pub(super) fn corrupt(request_id: &str, message: &str) -> SignError {
    SignError::new(SignErrorCode::CorruptState, message).for_request(request_id.to_string())
}
