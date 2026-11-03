use crate::evidence::{
    EvidenceAuthority, EvidenceItem, EvidenceKind, EvidenceRecord, EvidenceSource,
};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeAction {
    ConsensusSnapshot,
    MempoolSnapshot,
    TransactionPreflight,
}

pub type ProbeRequest = ProbeAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeActionSpec {
    pub action: ProbeAction,
    pub maximum_budget: ProbeBudget,
    pub observation_kind: &'static str,
}

pub const PROBE_ACTION_CATALOG: [ProbeActionSpec; 3] = [
    ProbeActionSpec {
        action: ProbeAction::ConsensusSnapshot,
        maximum_budget: ProbeBudget {
            max_duration_ms: 5_000,
            max_output_bytes: 65_536,
            max_observations: 64,
        },
        observation_kind: "consensus_snapshot",
    },
    ProbeActionSpec {
        action: ProbeAction::MempoolSnapshot,
        maximum_budget: ProbeBudget {
            max_duration_ms: 3_000,
            max_output_bytes: 32_768,
            max_observations: 32,
        },
        observation_kind: "mempool_snapshot",
    },
    ProbeActionSpec {
        action: ProbeAction::TransactionPreflight,
        maximum_budget: ProbeBudget {
            max_duration_ms: 3_000,
            max_output_bytes: 65_536,
            max_observations: 64,
        },
        observation_kind: "transaction_preflight",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTransaction {
    pub transaction_hash: String,
    pub transaction_bytes: Vec<u8>,
}

impl ProbeTransaction {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if self.transaction_bytes.is_empty()
            || self.transaction_bytes.len() > MAX_PROBE_TRANSACTION_BYTES
            || !valid_hash(&self.transaction_hash)
            || hex_sha256(&self.transaction_bytes) != self.transaction_hash.to_ascii_lowercase()
        {
            return Err(ProbeContractError::InvalidTransaction);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeContext {
    pub observed_height: u32,
    pub tip_hash: String,
    pub requested_at_unix_ms: u64,
    pub node_instance_id: String,
}

impl ProbeContext {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if !valid_hash(&self.tip_hash)
            || self.requested_at_unix_ms == 0
            || self.node_instance_id.is_empty()
            || self.node_instance_id.len() > 128
        {
            return Err(ProbeContractError::InvalidContext);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBuild {
    pub binary_hash: String,
    pub version: String,
}

impl ProbeBuild {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if !valid_hash(&self.binary_hash)
            || self.version.trim().is_empty()
            || self.version.len() > 128
        {
            return Err(ProbeContractError::InvalidBuild);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBudget {
    pub max_duration_ms: u64,
    pub max_output_bytes: u64,
    pub max_observations: u16,
}

impl ProbeBudget {
    pub fn validate_for(self, action: ProbeAction) -> Result<(), ProbeContractError> {
        let maximum = PROBE_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == action)
            .expect("complete probe action catalog")
            .maximum_budget;
        if self.max_duration_ms == 0
            || self.max_output_bytes == 0
            || self.max_observations == 0
            || self.max_duration_ms > maximum.max_duration_ms
            || self.max_output_bytes > maximum.max_output_bytes
            || self.max_observations > maximum.max_observations
            || self.max_duration_ms > MAX_PROBE_DURATION_MS
            || self.max_output_bytes > MAX_PROBE_OUTPUT_BYTES
            || self.max_observations > MAX_PROBE_OBSERVATIONS
        {
            return Err(ProbeContractError::BudgetExceeded);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        if self.max_duration_ms == 0 || self.max_output_bytes == 0 || self.max_observations == 0 {
            return Err(ProbeContractError::BudgetExceeded);
        }
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizerMetadata {
    pub evidence_id: String,
    pub evidence_hash: String,
    pub authority: EvidenceAuthority,
    pub rule_version: String,
    pub authorized_at_unix_ms: u64,
}

impl AuthorizerMetadata {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if self.authority != EvidenceAuthority::DeterministicRule {
            return Err(ProbeContractError::UntrustedAuthorizer);
        }
        if self.evidence_id.is_empty()
            || self.evidence_id.len() > 128
            || !valid_hash(&self.evidence_hash)
            || self.rule_version.trim().is_empty()
            || self.rule_version.len() > 128
            || self.authorized_at_unix_ms == 0
        {
            return Err(ProbeContractError::InvalidAuthorizer);
        }
        Ok(())
    }
}

/// Compatibility name retained for existing callers.
pub type ProbeAuthorizerMetadata = AuthorizerMetadata;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcomeStatus {
    Succeeded,
    Rejected,
    TimedOut,
    Failed,
}

/// Untrusted worker output. Authority claims are checked only by the parent validator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeOutcome {
    pub schema_version: u16,
    pub operation_id: OperationId,
    pub run_id: RunId,
    pub action: ProbeAction,
    pub job_hash: String,
    pub reported_authority: EvidenceAuthority,
    pub status: ProbeOutcomeStatus,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
    pub observations: BTreeMap<String, Value>,
}

impl ProbeOutcome {
    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        hash_serializable(self)
    }
}

/// Opaque identity established by the parent from a validated, locally authorized job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedLocalDiagnosticsIdentity {
    node_instance_id: String,
    process_id: u32,
    build_binding_hash: String,
}

impl TrustedLocalDiagnosticsIdentity {
    pub fn from_job(job: &ProbeJob) -> Result<Self, ProbeContractError> {
        job.validate()?;
        Ok(Self {
            node_instance_id: job.context.node_instance_id.clone(),
            process_id: std::process::id(),
            build_binding_hash: job.build.content_hash()?,
        })
    }

    fn matches(&self, job: &ProbeJob) -> Result<bool, ProbeContractError> {
        Ok(self.process_id == std::process::id()
            && self.node_instance_id == job.context.node_instance_id
            && self.build_binding_hash == job.build.content_hash()?)
    }
}

/// Opaque parent-validated probe result. It cannot be deserialized or assigned by a worker.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    record: EvidenceRecord,
}

impl ProbeResult {
    #[must_use]
    pub fn record(&self) -> &EvidenceRecord {
        &self.record
    }

    #[must_use]
    pub fn into_record(self) -> EvidenceRecord {
        self.record
    }
}

pub type ValidatedProbeResult = ProbeResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeRuleFindingKind {
    ConsensusStallObserved,
    MempoolPressureObserved,
    TransactionIsolationCandidate,
}

/// Fixed deterministic finding. It deliberately contains no executable action.
#[derive(Debug, Clone)]
pub struct ProbeRuleFinding {
    record: EvidenceRecord,
    finding: ProbeRuleFindingKind,
}

impl ProbeRuleFinding {
    #[must_use]
    pub fn record(&self) -> &EvidenceRecord {
        &self.record
    }

    #[must_use]
    pub const fn finding(&self) -> ProbeRuleFindingKind {
        self.finding
    }

    #[must_use]
    pub fn into_parts(self) -> (EvidenceRecord, ProbeRuleFindingKind) {
        (self.record, self.finding)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProbeDeterministicRule;

impl ProbeDeterministicRule {
    pub fn evaluate(
        &self,
        validated: &ProbeResult,
    ) -> Result<Option<ProbeRuleFinding>, ProbeContractError> {
        let parent = validated.record();
        parent.verify_integrity()?;
        if parent.authority != EvidenceAuthority::Observation
            || parent.kind != EvidenceKind::ProbeResult
            || parent.source != EvidenceSource::RestrictedProbe
        {
            return Err(ProbeContractError::InvalidValidatedResult);
        }
        let fields = parent.item.fields();
        if fields.get("status").and_then(Value::as_str) != Some("succeeded") {
            return Ok(None);
        }
        let observations = fields
            .get("observations")
            .and_then(Value::as_object)
            .ok_or(ProbeContractError::InvalidValidatedResult)?;
        let finding = match fields.get("action").and_then(Value::as_str) {
            Some("consensus_snapshot")
                if observations
                    .get("consensus_stalled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                ProbeRuleFindingKind::ConsensusStallObserved
            }
            Some("mempool_snapshot")
                if observations
                    .get("mempool_pressure")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) =>
            {
                ProbeRuleFindingKind::MempoolPressureObserved
            }
            Some("transaction_preflight") => {
                let Some(preflight) = observations.get("preflight").and_then(Value::as_object)
                else {
                    return Err(ProbeContractError::InvalidValidatedResult);
                };
                let ordinary_fault =
                    preflight.get("vm_state").and_then(Value::as_str) == Some("FAULT");
                if ordinary_fault {
                    return Ok(None);
                }
                if preflight.get("accepted").and_then(Value::as_bool) == Some(false)
                    && preflight.get("repeatable").and_then(Value::as_bool) == Some(true)
                    && preflight.get("classification").and_then(Value::as_str)
                        == Some("deterministic_attack_transaction")
                {
                    ProbeRuleFindingKind::TransactionIsolationCandidate
                } else {
                    return Ok(None);
                }
            }
            Some("consensus_snapshot" | "mempool_snapshot") => return Ok(None),
            _ => return Err(ProbeContractError::InvalidValidatedResult),
        };
        let item = EvidenceItem::new(
            format!("{}:probe-rule", parent.id),
            EvidenceKind::ProbeRuleFinding,
            EvidenceSource::DeterministicDetector,
            parent.observed_height,
            BTreeMap::from([
                ("parent_evidence_id".to_owned(), json!(parent.id)),
                (
                    "parent_evidence_hash".to_owned(),
                    json!(parent.content_hash),
                ),
                (
                    "transaction_hash".to_owned(),
                    json!(parent.transaction_hash),
                ),
                (
                    "context_hash".to_owned(),
                    fields
                        .get("context_hash")
                        .cloned()
                        .ok_or(ProbeContractError::InvalidValidatedResult)?,
                ),
                ("snapshot_height".to_owned(), json!(parent.tip_height)),
                (
                    "snapshot_root".to_owned(),
                    json!(
                        parent
                            .tip_hash
                            .as_deref()
                            .ok_or(ProbeContractError::InvalidValidatedResult)?
                    ),
                ),
                ("finding".to_owned(), json!(finding)),
                (
                    "rule_version".to_owned(),
                    json!(PROBE_DETERMINISTIC_RULE_VERSION),
                ),
            ]),
        )?;
        let record = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::DeterministicRule,
            parent.observed_at_unix_ms,
            parent.transaction_hash.clone(),
            parent.tip_height,
            parent.tip_hash.clone(),
            PROBE_DETERMINISTIC_RULE_VERSION,
        )?;
        Ok(Some(ProbeRuleFinding { record, finding }))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProbeResultValidator;

impl ProbeResultValidator {
    pub fn validate_double_replay(
        &self,
        identity: &TrustedLocalDiagnosticsIdentity,
        job: &ProbeJob,
        first: ProbeOutcome,
        second: ProbeOutcome,
        validated_at_unix_ms: u64,
    ) -> Result<ProbeResult, ProbeContractError> {
        if first != second {
            return Err(ProbeContractError::ReplayMismatch);
        }
        if first.status != ProbeOutcomeStatus::Succeeded
            || first.completed_at_unix_ms > validated_at_unix_ms
            || validated_at_unix_ms.saturating_sub(first.completed_at_unix_ms)
                > MAX_PROBE_RESULT_FRESHNESS_MS
        {
            return Err(ProbeContractError::StaleOutcome);
        }
        self.validate_single(identity, job, first)
    }

    fn validate_single(
        &self,
        identity: &TrustedLocalDiagnosticsIdentity,
        job: &ProbeJob,
        outcome: ProbeOutcome,
    ) -> Result<ProbeResult, ProbeContractError> {
        job.validate()?;
        if !identity.matches(job)? {
            return Err(ProbeContractError::UntrustedDiagnosticsIdentity);
        }
        if outcome.reported_authority != EvidenceAuthority::Observation {
            return Err(ProbeContractError::UntrustedOutcomeAuthority);
        }
        if outcome.schema_version != PROBE_SCHEMA_VERSION {
            return Err(ProbeContractError::UnsupportedVersion(
                outcome.schema_version,
            ));
        }
        let expected_job_hash = job.binding_hash()?;
        if outcome.operation_id != job.operation_id
            || outcome.run_id != job.run_id
            || outcome.action != job.action
            || outcome.job_hash != expected_job_hash
        {
            return Err(ProbeContractError::BindingMismatch);
        }
        if outcome.started_at_unix_ms < job.context.requested_at_unix_ms
            || outcome.completed_at_unix_ms < outcome.started_at_unix_ms
            || outcome
                .completed_at_unix_ms
                .saturating_sub(outcome.started_at_unix_ms)
                > job.budget.max_duration_ms
            || outcome.observations.len() > usize::from(job.budget.max_observations)
            || serde_json::to_vec(&outcome.observations)
                .map_err(|_| ProbeContractError::Serialization)?
                .len()
                > usize::try_from(job.budget.max_output_bytes).unwrap_or(usize::MAX)
        {
            return Err(ProbeContractError::OutcomeOutsideBudget);
        }
        if outcome.observations.keys().any(|key| {
            key.is_empty()
                || key.len() > 128
                || !key.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        }) {
            return Err(ProbeContractError::InvalidOutcome);
        }
        let fields = BTreeMap::from([
            ("operation_id".to_owned(), json!(job.operation_id)),
            ("run_id".to_owned(), json!(job.run_id)),
            ("action".to_owned(), json!(job.action)),
            ("job_hash".to_owned(), json!(expected_job_hash)),
            (
                "transaction_hash".to_owned(),
                json!(job.transaction.transaction_hash),
            ),
            (
                "context_hash".to_owned(),
                json!(job.context.content_hash()?),
            ),
            (
                "snapshot_height".to_owned(),
                json!(job.context.observed_height),
            ),
            ("snapshot_root".to_owned(), json!(job.context.tip_hash)),
            ("build_hash".to_owned(), json!(job.build.content_hash()?)),
            ("budget_hash".to_owned(), json!(job.budget.content_hash()?)),
            ("status".to_owned(), json!(outcome.status)),
            ("observations".to_owned(), json!(outcome.observations)),
            (
                "authorizer_evidence_id".to_owned(),
                json!(job.authorizer.evidence_id),
            ),
            (
                "authorizer_evidence_hash".to_owned(),
                json!(job.authorizer.evidence_hash),
            ),
        ]);
        let completed_at_unix_ms = outcome.completed_at_unix_ms;
        let item = EvidenceItem::new(
            format!("probe:{}:{}", job.operation_id, job.run_id),
            EvidenceKind::ProbeResult,
            EvidenceSource::RestrictedProbe,
            job.context.observed_height,
            fields,
        )?;
        let record = EvidenceRecord::new_validated_probe_result(
            item,
            completed_at_unix_ms,
            Some(job.transaction.transaction_hash.clone()),
            job.context.observed_height,
            Some(job.context.tip_hash.clone()),
            job.authorizer.rule_version.clone(),
        )?;
        Ok(ProbeResult { record })
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

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionEvidence, OperationalAction, OperationalActionRequest, OperationalDecision,
        OperationalPolicy, OperationalPolicyConfig, OperationalSafetyState,
    };

    fn job() -> ProbeJob {
        ProbeJob::new(
            OperationId::new("operation-23").unwrap(),
            RunId::new("run-1").unwrap(),
            ProbeAction::ConsensusSnapshot,
            ProbeTransaction {
                transaction_hash: hex_sha256(b"task-23-transaction"),
                transaction_bytes: b"task-23-transaction".to_vec(),
            },
            ProbeContext {
                observed_height: 42,
                tip_hash: "ab".repeat(32),
                requested_at_unix_ms: 100,
                node_instance_id: "local-node".to_owned(),
            },
            ProbeBuild {
                binary_hash: "bc".repeat(32),
                version: "neo-node-0.20.0".to_owned(),
            },
            ProbeBudget {
                max_duration_ms: 1_000,
                max_output_bytes: 4_096,
                max_observations: 8,
            },
            AuthorizerMetadata {
                evidence_id: "local-rule".to_owned(),
                evidence_hash: "cd".repeat(32),
                authority: EvidenceAuthority::DeterministicRule,
                rule_version: "task-23-v1".to_owned(),
                authorized_at_unix_ms: 99,
            },
        )
        .unwrap()
    }

    fn outcome(job: &ProbeJob) -> ProbeOutcome {
        ProbeOutcome {
            schema_version: PROBE_SCHEMA_VERSION,
            operation_id: job.operation_id.clone(),
            run_id: job.run_id.clone(),
            action: job.action,
            job_hash: job.binding_hash().unwrap(),
            reported_authority: EvidenceAuthority::Observation,
            status: ProbeOutcomeStatus::Succeeded,
            started_at_unix_ms: 101,
            completed_at_unix_ms: 102,
            observations: BTreeMap::from([("view".to_owned(), json!(3))]),
        }
    }

    #[test]
    fn contract_types_round_trip_with_strict_serde() {
        let job = job();
        let encoded = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<ProbeJob>(&encoded).unwrap(), job);
        let encoded = serde_json::to_string(&outcome(&job)).unwrap();
        assert_eq!(
            serde_json::from_str::<ProbeOutcome>(&encoded).unwrap(),
            outcome(&job)
        );
        let mut value = serde_json::to_value(job).unwrap();
        value["shell"] = json!("forbidden");
        assert!(serde_json::from_value::<ProbeJob>(value).is_err());
        assert!(serde_json::from_str::<OperationId>(r#""bad/id""#).is_err());
    }

    #[test]
    fn every_job_binding_hash_is_nonzero_and_content_bound() {
        let job = job();
        let hashes = [
            job.transaction.content_hash().unwrap(),
            job.context.content_hash().unwrap(),
            job.build.content_hash().unwrap(),
            job.budget.content_hash().unwrap(),
            job.binding_hash().unwrap(),
        ];
        assert!(hashes.iter().all(|hash| valid_hash(hash)));
        let mut changed = job.clone();
        changed.context.observed_height += 1;
        assert_ne!(
            job.context.content_hash().unwrap(),
            changed.context.content_hash().unwrap()
        );
        assert_ne!(job.binding_hash().unwrap(), changed.binding_hash().unwrap());
    }

    #[test]
    fn transaction_bytes_are_hash_bound_and_bounded() {
        let mut changed = job();
        changed.transaction.transaction_bytes.push(0xff);
        assert_eq!(
            changed.validate(),
            Err(ProbeContractError::InvalidTransaction)
        );

        changed.transaction.transaction_bytes = vec![1; MAX_PROBE_TRANSACTION_BYTES + 1];
        changed.transaction.transaction_hash = hex_sha256(&changed.transaction.transaction_bytes);
        assert_eq!(
            changed.validate(),
            Err(ProbeContractError::InvalidTransaction)
        );
    }

    #[test]
    fn trusted_parent_validation_creates_observation_probe_result() {
        let job = job();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        let replay = outcome(&job);
        let result = ProbeResultValidator
            .validate_double_replay(&identity, &job, replay.clone(), replay, 102)
            .unwrap();
        assert_eq!(result.record().authority, EvidenceAuthority::Observation);
        assert_eq!(result.record().kind, EvidenceKind::ProbeResult);
        assert_ne!(result.record().content_hash, "00".repeat(32));
    }

    #[test]
    fn model_and_forged_authority_are_rejected() {
        let job = job();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        for authority in [
            EvidenceAuthority::Model,
            EvidenceAuthority::DeterministicRule,
        ] {
            let mut outcome = outcome(&job);
            outcome.reported_authority = authority;
            assert_eq!(
                ProbeResultValidator.validate_double_replay(
                    &identity,
                    &job,
                    outcome.clone(),
                    outcome,
                    102,
                ),
                Err(ProbeContractError::UntrustedOutcomeAuthority)
            );
        }
    }

    #[test]
    fn identity_from_another_local_binding_is_rejected() {
        let job = job();
        let mut other = job.clone();
        other.context.node_instance_id = "other-node".to_owned();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&other).unwrap();
        assert_eq!(
            ProbeResultValidator.validate_double_replay(
                &identity,
                &job,
                outcome(&job),
                outcome(&job),
                102,
            ),
            Err(ProbeContractError::UntrustedDiagnosticsIdentity)
        );
    }

    #[test]
    fn outcome_binding_mismatch_is_rejected() {
        let job = job();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        let mut wrong = outcome(&job);
        wrong.run_id = RunId::new("other-run").unwrap();
        assert_eq!(
            ProbeResultValidator
                .validate_double_replay(&identity, &job, wrong.clone(), wrong, 102,),
            Err(ProbeContractError::BindingMismatch)
        );
    }

    #[test]
    fn fixed_rule_maps_to_finding_but_not_an_action() {
        let job = job();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        let mut outcome = outcome(&job);
        outcome
            .observations
            .insert("consensus_stalled".to_owned(), json!(true));
        let result = ProbeResultValidator
            .validate_double_replay(&identity, &job, outcome.clone(), outcome, 102)
            .unwrap();
        let finding = ProbeDeterministicRule
            .evaluate(&result)
            .unwrap()
            .expect("fixed finding");
        assert_eq!(
            finding.finding(),
            ProbeRuleFindingKind::ConsensusStallObserved
        );
        assert_eq!(finding.record().kind, EvidenceKind::ProbeRuleFinding);

        let record = finding.record().clone();
        let request = OperationalActionRequest {
            action: OperationalAction::IncreaseSampling,
            target: None,
            evidence: vec![ActionEvidence {
                id: record.id.clone(),
                content_hash: record.content_hash.clone(),
                authority: record.authority,
            }],
            model_recommended: false,
        };
        let policy = OperationalPolicy::new(OperationalPolicyConfig {
            enabled: true,
            observe_only: false,
            ..OperationalPolicyConfig::default()
        });
        assert_eq!(
            policy.evaluate(&request, OperationalSafetyState::default(), &[record]),
            OperationalDecision::InvalidEvidence
        );
    }

    #[test]
    fn mismatched_or_stale_double_replay_is_rejected() {
        let job = job();
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        let first = outcome(&job);
        let mut second = first.clone();
        second
            .observations
            .insert("changed".to_owned(), json!(true));
        assert_eq!(
            ProbeResultValidator.validate_double_replay(
                &identity,
                &job,
                first.clone(),
                second,
                102,
            ),
            Err(ProbeContractError::ReplayMismatch)
        );
        assert_eq!(
            ProbeResultValidator.validate_double_replay(
                &identity,
                &job,
                first.clone(),
                first,
                30_103
            ),
            Err(ProbeContractError::StaleOutcome)
        );
    }

    #[test]
    fn ordinary_fault_is_not_an_isolation_finding() {
        let job = job();
        let mut replay = outcome(&job);
        replay.action = ProbeAction::TransactionPreflight;
        let mut matching_job = job.clone();
        matching_job.action = ProbeAction::TransactionPreflight;
        replay.job_hash = matching_job.binding_hash().unwrap();
        replay.observations = BTreeMap::from([(
            "preflight".to_owned(),
            json!({
                "accepted": false,
                "repeatable": true,
                "classification": "deterministic_attack_transaction",
                "vm_state": "FAULT"
            }),
        )]);
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&matching_job).unwrap();
        let result = ProbeResultValidator
            .validate_double_replay(&identity, &matching_job, replay.clone(), replay, 102)
            .unwrap();
        assert!(ProbeDeterministicRule.evaluate(&result).unwrap().is_none());
    }

    #[test]
    fn fixed_transaction_rule_produces_isolation_finding() {
        let mut job = job();
        job.action = ProbeAction::TransactionPreflight;
        let identity = TrustedLocalDiagnosticsIdentity::from_job(&job).unwrap();
        let mut replay = outcome(&job);
        replay.action = ProbeAction::TransactionPreflight;
        replay.job_hash = job.binding_hash().unwrap();
        replay.observations = BTreeMap::from([(
            "preflight".to_owned(),
            json!({
                "accepted": false,
                "repeatable": true,
                "classification": "deterministic_attack_transaction"
            }),
        )]);
        let result = ProbeResultValidator
            .validate_double_replay(&identity, &job, replay.clone(), replay, 102)
            .unwrap();
        let finding = ProbeDeterministicRule.evaluate(&result).unwrap().unwrap();
        assert_eq!(
            finding.finding(),
            ProbeRuleFindingKind::TransactionIsolationCandidate
        );
        assert_eq!(
            finding.record().authority,
            EvidenceAuthority::DeterministicRule
        );
    }

    #[test]
    fn model_authorizer_cannot_create_a_probe_job() {
        let mut authorizer = job().authorizer;
        authorizer.authority = EvidenceAuthority::Model;
        assert_eq!(
            authorizer.validate(),
            Err(ProbeContractError::UntrustedAuthorizer)
        );
    }

    #[tokio::test]
    async fn try_submit_is_bounded_nonblocking_and_never_spawns() {
        let (sender, mut receiver) = ProbeAdmissionSender::bounded(1).unwrap();
        assert_eq!(sender.try_submit(job()), ProbeAdmissionResult::Enqueued);
        assert_eq!(sender.try_submit(job()), ProbeAdmissionResult::RejectedFull);
        assert!(receiver.recv().await.is_some());
        drop(receiver);
        assert_eq!(
            sender.try_submit(job()),
            ProbeAdmissionResult::RejectedClosed
        );
    }
}
