//! Deny-first authorization policy for signing operations.

use super::error::{SignError, SignErrorCode, SignResult};
use super::types::{
    AuthenticatedContext, KeyInfo, SecurityLevel, SignPurpose, SignRequest, SigningKeyResolver,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Allow and deny values for one policy dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Ord + serde::Deserialize<'de>"))]
pub struct PolicySelector<T> {
    /// Explicitly allowed values.
    pub allow: BTreeSet<T>,
    /// Explicitly denied values. Deny entries always take precedence.
    pub deny: BTreeSet<T>,
}

impl<T> Default for PolicySelector<T> {
    fn default() -> Self {
        Self {
            allow: BTreeSet::new(),
            deny: BTreeSet::new(),
        }
    }
}

impl<T: Ord> PolicySelector<T> {
    fn denies(&self, value: &T) -> bool {
        self.deny.contains(value)
    }

    fn allows(&self, value: &T) -> bool {
        self.allow.contains(value)
    }
}

/// Integer-only fee and quota controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyLimits {
    /// Maximum integer fee units accepted per request.
    pub max_fee_units: u64,
    /// Maximum integer quota units in one storage window.
    pub quota_limit_units: u64,
    /// Quota window duration in milliseconds.
    pub quota_window_ms: i64,
}

impl Default for PolicyLimits {
    fn default() -> Self {
        Self {
            max_fee_units: 0,
            quota_limit_units: 0,
            quota_window_ms: 0,
        }
    }
}

/// Versioned signing policy. Empty allow sets intentionally deny all requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningPolicy {
    /// Policy schema version.
    pub schema_version: u16,
    /// Stable policy version required by requests and audit records.
    pub version: String,
    /// Principal allow and deny entries.
    pub principals: PolicySelector<String>,
    /// Tenant allow and deny entries.
    pub tenants: PolicySelector<String>,
    /// Versioned key selectors in `provider:key_id:version` form.
    pub keys: PolicySelector<String>,
    /// Network allow and deny entries.
    pub networks: PolicySelector<u32>,
    /// Signature-purpose allow and deny entries.
    pub purposes: PolicySelector<SignPurpose>,
    /// Exact principal×tenant×key×network×purpose×scope authorization tuples.
    pub authorizations: PolicySelector<String>,
    /// Configured account/purpose key resolver and public-key bindings.
    #[serde(skip)]
    pub key_resolver: SigningKeyResolver,
    /// Key security-level allow and deny entries.
    pub security_levels: PolicySelector<SecurityLevel>,
    /// Optional account allow and deny entries.
    pub accounts: PolicySelector<String>,
    /// Target allow and deny entries.
    pub targets: PolicySelector<String>,
    /// Integer fee and quota controls.
    pub limits: PolicyLimits,
}

impl Default for SigningPolicy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            version: "deny-all-v1".to_string(),
            principals: PolicySelector::default(),
            tenants: PolicySelector::default(),
            keys: PolicySelector::default(),
            networks: PolicySelector::default(),
            purposes: PolicySelector::default(),
            authorizations: PolicySelector::default(),
            key_resolver: SigningKeyResolver::default(),
            security_levels: PolicySelector::default(),
            accounts: PolicySelector::default(),
            targets: PolicySelector::default(),
            limits: PolicyLimits::default(),
        }
    }
}

/// Quota reservation authorized by policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaReservation {
    /// Stable scope combining tenant, key, network, and purpose.
    pub scope: String,
    /// Units reserved by this request.
    pub units: u64,
    /// Maximum units permitted in the active window.
    pub limit: u64,
    /// Window duration in milliseconds.
    pub window_ms: i64,
}

/// Successful policy decision consumed by persistence and provider layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Policy version that authorized the request.
    pub policy_version: String,
    /// Atomic quota reservation to apply before signing.
    pub quota: QuotaReservation,
}

impl SigningPolicy {
    /// Evaluates all deny lists before any allow list and integer limits.
    pub fn evaluate(
        &self,
        request: &SignRequest,
        context: &AuthenticatedContext,
        key_info: &KeyInfo,
    ) -> SignResult<PolicyDecision> {
        if request.policy_version != self.version {
            return Err(self.denied(
                request,
                "request policy version does not match active policy",
            ));
        }
        if key_info.key_ref != request.key_ref {
            return Err(SignError::new(
                SignErrorCode::KeyNotFound,
                "provider returned metadata for a different key version",
            )
            .for_request(request.request_id.clone()));
        }
        if !key_info.enabled {
            return Err(
                SignError::new(SignErrorCode::KeyDisabled, "key version is disabled")
                    .for_request(request.request_id.clone()),
            );
        }
        self.key_resolver.verify(request, key_info)?;

        let key = request.key_ref.selector();
        let required_scope = request.purpose.required_scope();
        if context.scopes.len() != 1 || !context.scopes.contains(&required_scope) {
            return Err(self.denied(
                request,
                "authenticated context must contain exactly the scope required by the signing purpose",
            ));
        }
        let authorization = Self::authorization_selector(
            &context.principal,
            &context.tenant,
            &request.key_ref,
            request.network,
            &request.purpose,
            &required_scope,
        );
        let account_denied = request
            .account
            .as_ref()
            .is_some_and(|value| self.accounts.denies(value));
        let target_denied = request
            .targets
            .iter()
            .any(|value| self.targets.denies(value));
        if self.principals.denies(&context.principal)
            || self.tenants.denies(&context.tenant)
            || self.keys.denies(&key)
            || self.networks.denies(&request.network)
            || self.purposes.denies(&request.purpose)
            || self.authorizations.denies(&authorization)
            || self.security_levels.denies(&key_info.security_level)
            || account_denied
            || target_denied
        {
            return Err(self.denied(request, "request matched an explicit deny rule"));
        }

        let account_allowed = request
            .account
            .as_ref()
            .is_none_or(|value| self.accounts.allows(value));
        let targets_allowed = request
            .targets
            .iter()
            .all(|value| self.targets.allows(value));
        if !self.principals.allows(&context.principal)
            || !self.tenants.allows(&context.tenant)
            || !self.keys.allows(&key)
            || !self.networks.allows(&request.network)
            || !self.purposes.allows(&request.purpose)
            || !self.authorizations.allows(&authorization)
            || !self.security_levels.allows(&key_info.security_level)
            || !account_allowed
            || !targets_allowed
        {
            return Err(self.denied(request, "request lacks an explicit allow rule"));
        }

        if request.fee_units > self.limits.max_fee_units {
            return Err(SignError::new(
                SignErrorCode::LimitExceeded,
                "request fee exceeds policy maximum",
            )
            .for_request(request.request_id.clone())
            .with_detail("max_fee_units", self.limits.max_fee_units.to_string()));
        }
        if self.limits.quota_limit_units == 0 || self.limits.quota_window_ms <= 0 {
            return Err(self.denied(request, "quota policy is not configured"));
        }

        Ok(PolicyDecision {
            policy_version: self.version.clone(),
            quota: QuotaReservation {
                scope: authorization,
                units: request.quota_units,
                limit: self.limits.quota_limit_units,
                window_ms: self.limits.quota_window_ms,
            },
        })
    }

    /// Produces the canonical joint authorization selector.
    #[must_use]
    pub fn authorization_selector(
        principal: &str,
        tenant: &str,
        key_ref: &super::types::KeyRef,
        network: u32,
        purpose: &SignPurpose,
        scope: &str,
    ) -> String {
        format!(
            "{principal}|{tenant}|{}|{network}|{purpose:?}|{scope}",
            key_ref.selector()
        )
    }

    fn denied(&self, request: &SignRequest, message: &str) -> SignError {
        SignError::new(SignErrorCode::PolicyDenied, message)
            .for_request(request.request_id.clone())
            .with_detail("policy_version", self.version.clone())
    }
}
