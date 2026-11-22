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
            version: "neo-node-0.21.0".to_owned(),
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
        ProbeResultValidator.validate_double_replay(&identity, &job, wrong.clone(), wrong, 102,),
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
        ProbeResultValidator.validate_double_replay(&identity, &job, first.clone(), second, 102,),
        Err(ProbeContractError::ReplayMismatch)
    );
    assert_eq!(
        ProbeResultValidator.validate_double_replay(&identity, &job, first.clone(), first, 30_103),
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
