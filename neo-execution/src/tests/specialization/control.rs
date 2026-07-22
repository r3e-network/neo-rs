use super::*;
use crate::application_engine::TEST_MODE_GAS;
use crate::diagnostic::NoDiagnostic;
use crate::native_contract_provider::NoNativeContractProvider;
use crate::{
    ApplicationEngine, CanonicalExecutionArtifact, ExecutionArtifactLimits,
    ExecutionObservationJournal,
};
use neo_config::ProtocolSettings;
use neo_primitives::CallFlags;
use neo_primitives::{TriggerType, UInt160, UInt256};
use neo_storage::DataCache;
use neo_vm::{CandidateId, CandidateVersion, OpCode, SpecializationMode};
use std::sync::Arc;

const CANDIDATE: CandidateId = CandidateId::new(7);
const VERSION: CandidateVersion = CandidateVersion::new(3);

fn config(strict_replay: bool, limits: SpecializationControlLimits) -> SpecializationControlConfig {
    SpecializationControlConfig::try_enabled(
        strict_replay,
        limits,
        [
            CandidateRouteConfig::new(CANDIDATE, VERSION, SpecializationMode::Shadow),
            CandidateRouteConfig::new(
                CandidateId::new(9),
                CandidateVersion::new(1),
                SpecializationMode::Authoritative,
            ),
        ],
    )
    .expect("valid control config")
}

fn executed_engine() -> ApplicationEngine<NoNativeContractProvider> {
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(
            vec![OpCode::PUSH1.byte(), OpCode::RET.byte()],
            CallFlags::ALL,
            None,
        )
        .expect("load script");
    engine.execute().expect("execute script");
    engine
}

fn artifact_mismatch() -> crate::ExecutionArtifactMismatch {
    let engine = executed_engine();
    let ordinary = CanonicalExecutionArtifact::capture(
        &engine,
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("ordinary artifact");
    let mut journal = ExecutionObservationJournal::new();
    journal
        .record_fee_charge(1)
        .expect("bounded fee observation");
    let optimized =
        CanonicalExecutionArtifact::capture(&engine, &journal, ExecutionArtifactLimits::DEFAULT)
            .expect("optimized artifact");
    ordinary
        .compare(&optimized)
        .expect_err("fee observation differs")
}

fn mismatch_input<'a>(payload: &'a [u8]) -> SpecializationMismatchInput<'a> {
    SpecializationMismatchInput {
        candidate_id: CANDIDATE,
        candidate_version: VERSION,
        mismatch: artifact_mismatch(),
        block_index: Some(1_887_001),
        transaction_hash: Some(UInt256::from([0x44; 32])),
        script_hash: UInt160::from([0x33; 20]),
        entry_ip: 391,
        ordinary_artifact_digest: [0x11; 32],
        optimized_artifact_digest: [0x22; 32],
        payload,
    }
}

#[test]
fn routing_is_disabled_by_default_and_exact_versioned_when_enabled() {
    let disabled = SpecializationControl::default();
    assert_eq!(
        disabled.route(CANDIDATE, VERSION),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::GloballyDisabled,
        }
    );

    let control = SpecializationControl::new(config(false, SpecializationControlLimits::DEFAULT));
    assert_eq!(
        control.route(CANDIDATE, VERSION),
        SpecializationRouteDecision::Shadow
    );
    assert_eq!(
        control.route(CANDIDATE, CandidateVersion::new(4)),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::CandidateVersionMismatch,
        }
    );
    assert_eq!(
        control.route(CandidateId::new(9), CandidateVersion::new(1)),
        SpecializationRouteDecision::Authoritative
    );
    assert_eq!(
        control.route(CandidateId::new(10), CandidateVersion::new(1)),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::CandidateNotConfigured,
        }
    );
}

#[test]
fn global_and_candidate_kill_switches_are_shared_and_irreversible() {
    let control = SpecializationControl::new(config(false, SpecializationControlLimits::DEFAULT));
    let shared = control.clone();
    assert!(!control.kill_candidate(CANDIDATE, CandidateVersion::new(99)));
    assert!(control.kill_candidate(CANDIDATE, VERSION));
    assert_eq!(
        shared.route(CANDIDATE, VERSION),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::CandidateKillSwitch,
        }
    );

    shared.kill_global();
    assert_eq!(
        control.route(CandidateId::new(9), CandidateVersion::new(1)),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::GlobalKillSwitch,
        }
    );
    assert!(control.snapshot().global_killed);
}

#[test]
fn first_mismatch_latches_candidate_and_retains_one_bounded_reproducer() {
    let limits = SpecializationControlLimits {
        max_reproducer_bytes: 3,
        ..SpecializationControlLimits::DEFAULT
    };
    let control = SpecializationControl::new(config(false, limits));
    let payload = [1, 2, 3, 4, 5];
    assert_eq!(
        control
            .record_mismatch(mismatch_input(&payload))
            .expect("non-strict mismatch"),
        MismatchRecordOutcome::FirstMismatchLatched
    );
    assert_eq!(
        control
            .record_mismatch(mismatch_input(b"later"))
            .expect("later mismatch"),
        MismatchRecordOutcome::AlreadyLatched
    );
    assert_eq!(
        control.route(CANDIDATE, VERSION),
        SpecializationRouteDecision::Ordinary {
            reason: SpecializationDisableReason::MismatchLatched,
        }
    );

    let snapshot = control.snapshot();
    assert_eq!(snapshot.reproducers.len(), 1);
    assert_eq!(snapshot.retained_reproducer_bytes, 3);
    let evidence = &snapshot.reproducers[0];
    assert_eq!(evidence.payload_prefix, [1, 2, 3]);
    assert_eq!(evidence.original_payload_bytes, 5);
    assert!(evidence.payload_truncated);
    assert_ne!(evidence.payload_digest, [0; 32]);
    let detail = evidence
        .detail
        .expect("sequence mismatch detail is retained");
    assert_eq!(detail.ordinary_count, 0);
    assert_eq!(detail.candidate_count, 1);
    assert_eq!(detail.first_diff_index, 0);
    assert_eq!(snapshot.candidates[0].mismatches, 2);
}

#[test]
fn strict_replay_errors_after_latching_and_match_counters_are_exact() {
    let control = SpecializationControl::new(config(true, SpecializationControlLimits::DEFAULT));
    assert!(control.record_match(CANDIDATE, VERSION));
    assert!(!control.record_match(CANDIDATE, CandidateVersion::new(9)));
    let error = control
        .record_mismatch(mismatch_input(b"strict"))
        .expect_err("strict replay fails");
    assert!(matches!(
        error,
        SpecializationControlError::StrictReplayMismatch {
            candidate_id: CANDIDATE,
            candidate_version: VERSION,
            component: crate::ExecutionArtifactComponent::FeeCharges,
            detail: Some(detail),
        }
            if detail.ordinary_count == 0
                && detail.candidate_count == 1
                && detail.first_diff_index == 0
    ));
    let snapshot = control.snapshot();
    assert_eq!(snapshot.candidates[0].matches, 1);
    assert_eq!(snapshot.candidates[0].mismatches, 1);
    assert!(snapshot.candidates[0].mismatch_latched);
}

#[test]
fn configuration_rejects_zero_bounds_duplicates_disabled_and_capacity() {
    let zero = SpecializationControlLimits {
        max_candidates: 0,
        ..SpecializationControlLimits::DEFAULT
    };
    assert!(matches!(
        SpecializationControlConfig::try_enabled(false, zero, []),
        Err(SpecializationControlConfigError::ZeroLimit {
            limit: "max_candidates"
        })
    ));
    assert!(matches!(
        SpecializationControlConfig::try_enabled(
            false,
            SpecializationControlLimits::DEFAULT,
            [CandidateRouteConfig::new(
                CANDIDATE,
                VERSION,
                SpecializationMode::Disabled,
            )],
        ),
        Err(SpecializationControlConfigError::DisabledCandidateMode)
    ));
    assert!(matches!(
        SpecializationControlConfig::try_enabled(
            false,
            SpecializationControlLimits::DEFAULT,
            [
                CandidateRouteConfig::new(CANDIDATE, VERSION, SpecializationMode::Shadow),
                CandidateRouteConfig::new(
                    CANDIDATE,
                    CandidateVersion::new(4),
                    SpecializationMode::Shadow,
                ),
            ],
        ),
        Err(SpecializationControlConfigError::DuplicateCandidateId {
            candidate_id: CANDIDATE
        })
    ));
    let one = SpecializationControlLimits {
        max_candidates: 1,
        ..SpecializationControlLimits::DEFAULT
    };
    assert!(matches!(
        SpecializationControlConfig::try_enabled(
            false,
            one,
            [
                CandidateRouteConfig::new(CANDIDATE, VERSION, SpecializationMode::Shadow),
                CandidateRouteConfig::new(
                    CandidateId::new(9),
                    CandidateVersion::new(1),
                    SpecializationMode::Shadow,
                ),
            ],
        ),
        Err(SpecializationControlConfigError::CandidateCapacity {
            actual: 2,
            maximum: 1
        })
    ));
}
