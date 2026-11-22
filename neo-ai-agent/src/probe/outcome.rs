use super::*;
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
