use crate::evidence::{
    EvidenceAuthority, EvidenceItem, EvidenceKind, EvidenceRecord, EvidenceSource,
};
use crate::store_files::{hex_sha256, valid_hash};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;
use tokio::sync::mpsc;

pub const PROBE_SCHEMA_VERSION: u16 = 1;
pub const MAX_PROBE_DURATION_MS: u64 = 30_000;
pub const MAX_PROBE_OUTPUT_BYTES: u64 = 262_144;
pub const MAX_PROBE_OBSERVATIONS: u16 = 256;
pub const MAX_PROBE_TRANSACTION_BYTES: usize = 262_144;
pub const PROBE_DETERMINISTIC_RULE_VERSION: &str = "restricted-probe-result-v1";
pub const MAX_PROBE_RESULT_FRESHNESS_MS: u64 = 30_000;

macro_rules! typed_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProbeContractError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 48
                    || !value.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                    })
                {
                    return Err(ProbeContractError::InvalidId($label));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(OperationId, "operation_id");
typed_id!(RunId, "run_id");

/// Compatibility name retained for existing callers.
pub type ProbeRunId = RunId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeJob {
    pub schema_version: u16,
    pub operation_id: OperationId,
    pub run_id: RunId,
    pub action: ProbeAction,
    pub transaction: ProbeTransaction,
    pub context: ProbeContext,
    pub build: ProbeBuild,
    pub budget: ProbeBudget,
    pub authorizer: AuthorizerMetadata,
}

impl ProbeJob {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        run_id: RunId,
        action: ProbeAction,
        transaction: ProbeTransaction,
        context: ProbeContext,
        build: ProbeBuild,
        budget: ProbeBudget,
        authorizer: AuthorizerMetadata,
    ) -> Result<Self, ProbeContractError> {
        let job = Self {
            schema_version: PROBE_SCHEMA_VERSION,
            operation_id,
            run_id,
            action,
            transaction,
            context,
            build,
            budget,
            authorizer,
        };
        job.validate()?;
        Ok(job)
    }

    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if self.schema_version != PROBE_SCHEMA_VERSION {
            return Err(ProbeContractError::UnsupportedVersion(self.schema_version));
        }
        self.transaction.validate()?;
        self.context.validate()?;
        self.build.validate()?;
        self.budget.validate_for(self.action)?;
        self.authorizer.validate()?;
        if self.authorizer.authorized_at_unix_ms > self.context.requested_at_unix_ms {
            return Err(ProbeContractError::InvalidAuthorizer);
        }
        Ok(())
    }

    pub fn binding_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

fn hash_serializable(value: &impl Serialize) -> Result<String, ProbeContractError> {
    let encoded = serde_json::to_vec(value).map_err(|_| ProbeContractError::Serialization)?;
    let hash = hex_sha256(&encoded);
    if !valid_hash(&hash) {
        return Err(ProbeContractError::ZeroContentHash);
    }
    Ok(hash)
}

#[derive(Debug, Clone)]
pub struct ProbeAdmissionSender {
    sender: mpsc::Sender<ProbeJob>,
}

#[derive(Debug)]
pub struct ProbeAdmissionReceiver {
    receiver: mpsc::Receiver<ProbeJob>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAdmissionResult {
    Enqueued,
    RejectedFull,
    RejectedClosed,
    RejectedInvalid,
}

impl ProbeAdmissionSender {
    pub fn bounded(capacity: usize) -> Result<(Self, ProbeAdmissionReceiver), ProbeContractError> {
        if capacity == 0 {
            return Err(ProbeContractError::ZeroCapacity);
        }
        let (sender, receiver) = mpsc::channel(capacity);
        Ok((Self { sender }, ProbeAdmissionReceiver { receiver }))
    }

    #[must_use]
    pub fn try_submit(&self, job: ProbeJob) -> ProbeAdmissionResult {
        if job.validate().is_err() {
            return ProbeAdmissionResult::RejectedInvalid;
        }
        match self.sender.try_send(job) {
            Ok(()) => ProbeAdmissionResult::Enqueued,
            Err(mpsc::error::TrySendError::Full(_)) => ProbeAdmissionResult::RejectedFull,
            Err(mpsc::error::TrySendError::Closed(_)) => ProbeAdmissionResult::RejectedClosed,
        }
    }

    #[must_use]
    pub fn try_admit(&self, job: ProbeJob) -> ProbeAdmissionResult {
        self.try_submit(job)
    }
}

impl ProbeAdmissionReceiver {
    pub async fn recv(&mut self) -> Option<ProbeJob> {
        self.receiver.recv().await
    }
}

#[async_trait]
pub trait RestrictedProbe: Send + Sync {
    async fn request(&self, request: ProbeRequest) -> Result<Vec<(String, String)>, ProbeError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FailClosedProbe;

#[async_trait]
impl RestrictedProbe for FailClosedProbe {
    async fn request(&self, _request: ProbeRequest) -> Result<Vec<(String, String)>, ProbeError> {
        Err(ProbeError::Unavailable)
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    #[error("restricted probe worker is unavailable")]
    Unavailable,
    #[error("restricted probe request was rejected")]
    Rejected,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProbeContractError {
    #[error("invalid {0}")]
    InvalidId(&'static str),
    #[error("unsupported probe schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("probe transaction binding is invalid")]
    InvalidTransaction,
    #[error("probe context is invalid")]
    InvalidContext,
    #[error("probe build binding is invalid")]
    InvalidBuild,
    #[error("probe budget exceeds the per-action catalog")]
    BudgetExceeded,
    #[error("probe authorizer is invalid")]
    InvalidAuthorizer,
    #[error("probe authorizer is not a deterministic rule")]
    UntrustedAuthorizer,
    #[error("probe result did not come from the trusted local diagnostics identity")]
    UntrustedDiagnosticsIdentity,
    #[error("probe outcome claimed model or deterministic authority")]
    UntrustedOutcomeAuthority,
    #[error("probe outcome is not bound to its admitted job")]
    BindingMismatch,
    #[error("the two fresh probe replays did not match exactly")]
    ReplayMismatch,
    #[error("the probe replay outcome is stale or unsuccessful")]
    StaleOutcome,
    #[error("probe outcome exceeds its budget")]
    OutcomeOutsideBudget,
    #[error("probe outcome is invalid")]
    InvalidOutcome,
    #[error("validated probe result is invalid")]
    InvalidValidatedResult,
    #[error("probe contract serialization failed")]
    Serialization,
    #[error("probe binding produced an invalid zero content hash")]
    ZeroContentHash,
    #[error("probe admission capacity must be non-zero")]
    ZeroCapacity,
    #[error(transparent)]
    Evidence(#[from] crate::evidence::EvidenceError),
}

mod outcome;
mod spec;

pub use outcome::{
    AuthorizerMetadata, ProbeAuthorizerMetadata, ProbeDeterministicRule, ProbeOutcome,
    ProbeOutcomeStatus, ProbeResult, ProbeResultValidator, ProbeRuleFinding, ProbeRuleFindingKind,
    TrustedLocalDiagnosticsIdentity, ValidatedProbeResult,
};
pub use spec::{
    PROBE_ACTION_CATALOG, ProbeAction, ProbeActionSpec, ProbeBudget, ProbeBuild, ProbeContext,
    ProbeRequest, ProbeTransaction,
};

#[cfg(test)]
mod tests;
