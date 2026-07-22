//! # neo-execution execution artifact tests
//!
//! Differential artifact capture and canonicalization regressions.
//!
//! ## Boundary
//!
//! These tests exercise comparison-only artifact behavior with isolated caches
//! and NeoVM values. They do not publish ledger state or test node scheduling.
//!
//! ## Contents
//!
//! - Cyclic and aliased stack graph canonicalization cases.
//! - Resource-bound and execution-result capture cases.
//! - Storage point-read and range-observation regressions.

use super::*;
use crate::application_engine::TEST_MODE_GAS;
use crate::diagnostic::NoDiagnostic;
use crate::host_access_audit::{ContractCallAccess, ContractCallKind, HostContextAccess};
use crate::native_contract_provider::NoNativeContractProvider;
use crate::{ApplicationEngine, ApplicationExecutionContext, ExecutionContextState};
use neo_config::ProtocolSettings;
use neo_primitives::CallFlags;
use neo_primitives::{Hardfork, TriggerType, UInt160};
use neo_storage::{DataCache, EmptyCacheBacking, StorageItem, StorageKey};
use neo_vm::stack_item::{Array, Map, Struct};
use neo_vm::{
    ContractResolutionIdentity, InteropInterface, OpCode, ReferenceCounter, Script, StackItem,
    VmState,
};
use std::sync::Arc;

fn mixed_graph() -> Vec<StackItem> {
    let buffer = StackItem::from_buffer(vec![0x01, 0x02, 0x03]);
    let bls = StackItem::from_interface(InteropInterface::bls12381(vec![0x5a; 48]));

    let array_value =
        Array::new_untracked(vec![buffer.clone(), StackItem::from_i64(-7), bls.clone()]);
    array_value.set_read_only(true);
    let array = StackItem::Array(array_value);

    let struct_value = Struct::new_untracked(vec![array.clone(), buffer.clone()]);
    struct_value.set_read_only(true);
    let structure = StackItem::Struct(struct_value);

    let iterator = StackItem::from_interface(InteropInterface::iterator(23));
    let map_value = Map::new_untracked(vec![
        (
            StackItem::from_byte_string(b"structure".to_vec()),
            structure.clone(),
        ),
        (
            StackItem::from_byte_string(b"iterator".to_vec()),
            iterator.clone(),
        ),
    ]);
    map_value.set_read_only(true);
    let map = StackItem::Map(map_value);

    let script = Arc::new(
        Script::new(vec![OpCode::NOP.byte(), OpCode::RET.byte()], true)
            .expect("valid pointer script"),
    );
    let first_pointer = StackItem::from_pointer(Arc::clone(&script), 0);
    let second_pointer = StackItem::from_pointer(script, 1);

    vec![
        map,
        structure,
        array,
        buffer,
        first_pointer,
        second_pointer,
        iterator.clone(),
        iterator,
        bls,
    ]
}

#[test]
fn independently_allocated_equivalent_graphs_have_identical_canonical_documents() {
    let first_roots = mixed_graph();
    let second_roots = mixed_graph();

    let first = CanonicalStackDocument::capture(&first_roots, ExecutionArtifactLimits::DEFAULT)
        .expect("capture first graph");
    let second = CanonicalStackDocument::capture(&second_roots, ExecutionArtifactLimits::DEFAULT)
        .expect("capture independently allocated graph");

    assert_eq!(first, second);

    let nodes = first.graph().nodes();
    assert!(nodes.iter().any(
        |node| matches!(node, CanonicalStackNode::Buffer(bytes) if bytes == &[0x01, 0x02, 0x03])
    ));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Array {
            read_only: true,
            ..
        }
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Struct {
            read_only: true,
            ..
        }
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Map {
            read_only: true,
            entries
        } if entries.len() == 2
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Script(bytes)
            if bytes == &[OpCode::NOP.byte(), OpCode::RET.byte()]
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Interop(CanonicalInteropInterface::Iterator(23))
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        CanonicalStackNode::Interop(CanonicalInteropInterface::Bls12381(bytes))
            if bytes == &vec![0x5a; 48]
    )));

    let CanonicalStackValue::Pointer {
        script: first_script,
        position: 0,
    } = first.roots()[4]
    else {
        panic!("first pointer root")
    };
    let CanonicalStackValue::Pointer {
        script: second_script,
        position: 1,
    } = first.roots()[5]
    else {
        panic!("second pointer root")
    };
    assert_eq!(first_script, second_script);
    assert_eq!(first.roots()[6], first.roots()[7]);
}

#[test]
fn canonical_documents_distinguish_alias_topology_from_equal_contents() {
    let shared = StackItem::from_buffer(vec![0xaa, 0xbb]);
    let aliased = CanonicalStackDocument::capture(
        &[shared.clone(), shared],
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture aliased roots");
    let split = CanonicalStackDocument::capture(
        &[
            StackItem::from_buffer(vec![0xaa, 0xbb]),
            StackItem::from_buffer(vec![0xaa, 0xbb]),
        ],
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture split roots");

    assert_eq!(aliased.graph().nodes().len(), 1);
    assert_eq!(split.graph().nodes().len(), 2);
    assert_eq!(aliased.roots()[0], aliased.roots()[1]);
    assert_ne!(split.roots()[0], split.roots()[1]);
    assert_ne!(aliased, split);
}

fn capture_self_cycle() -> CanonicalStackDocument {
    let root = StackItem::from_array(vec![StackItem::from_i64(7)]);
    let alias = root.clone();
    let StackItem::Array(array) = &root else {
        unreachable!("test constructs an array")
    };
    array.push(alias).expect("append self-reference");

    let document = CanonicalStackDocument::capture(
        std::slice::from_ref(&root),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture cyclic graph");
    array.clear().expect("break test cycle");
    document
}

#[test]
fn cyclic_graphs_terminate_and_normalize_independently() {
    let first = capture_self_cycle();
    let second = capture_self_cycle();

    assert_eq!(first, second);
    assert_eq!(first.roots(), &[CanonicalStackValue::Reference(0)]);
    assert_eq!(
        first.graph().nodes(),
        &[CanonicalStackNode::Array {
            read_only: false,
            items: vec![
                CanonicalStackValue::Integer(vec![7]),
                CanonicalStackValue::Reference(0),
            ],
        }]
    );
}

fn assert_limit(
    result: Result<CanonicalStackDocument, ExecutionArtifactError>,
    resource: &'static str,
    actual: usize,
    maximum: usize,
) {
    assert_eq!(
        result.expect_err("capture must fail closed at its bound"),
        ExecutionArtifactError::LimitExceeded {
            resource,
            actual,
            maximum,
        }
    );
}

#[test]
fn graph_capture_enforces_root_node_edge_depth_and_byte_bounds() {
    let limits = ExecutionArtifactLimits {
        max_stack_roots: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    assert_limit(
        CanonicalStackDocument::capture(&[StackItem::Null], limits),
        "stack roots",
        1,
        0,
    );

    let limits = ExecutionArtifactLimits {
        max_stack_nodes: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    assert_limit(
        CanonicalStackDocument::capture(&[StackItem::from_buffer(vec![1])], limits),
        "stack graph nodes",
        1,
        0,
    );

    let limits = ExecutionArtifactLimits {
        max_stack_edges: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    assert_limit(
        CanonicalStackDocument::capture(&[StackItem::from_array(vec![StackItem::Null])], limits),
        "stack graph edges",
        1,
        0,
    );

    let limits = ExecutionArtifactLimits {
        max_stack_depth: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };
    assert_limit(
        CanonicalStackDocument::capture(&[StackItem::from_array(vec![StackItem::Null])], limits),
        "stack graph depth",
        1,
        0,
    );

    let limits = ExecutionArtifactLimits {
        max_retained_bytes: 1,
        ..ExecutionArtifactLimits::DEFAULT
    };
    assert_limit(
        CanonicalStackDocument::capture(&[StackItem::from_byte_string(vec![0x11, 0x22])], limits),
        "retained bytes",
        2,
        1,
    );
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
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    engine
}

#[test]
fn complete_engine_artifacts_compare_and_report_the_first_mismatch_component() {
    let ordinary_engine = executed_engine();
    let optimized_engine = executed_engine();
    let ordinary_journal = ExecutionObservationJournal::new();
    let optimized_journal = ExecutionObservationJournal::new();

    let ordinary = CanonicalExecutionArtifact::capture(
        &ordinary_engine,
        &ordinary_journal,
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture ordinary artifact");
    let optimized = CanonicalExecutionArtifact::capture(
        &optimized_engine,
        &optimized_journal,
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture optimized artifact");

    assert_eq!(ordinary.vm_state(), VmState::HALT);
    assert_eq!(
        ordinary.result_stack(),
        &[CanonicalStackValue::Integer(vec![1])]
    );
    assert_eq!(ordinary.compare(&optimized), Ok(()));

    let mut mismatched_journal = ExecutionObservationJournal::new();
    mismatched_journal
        .record_fee_charge(1)
        .expect("bounded fee observation");
    let mismatched = CanonicalExecutionArtifact::capture(
        &optimized_engine,
        &mismatched_journal,
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture mismatched artifact");
    let mismatch = ordinary
        .compare(&mismatched)
        .expect_err("fee-charge sequence must be compared");
    assert_eq!(mismatch.component(), ExecutionArtifactComponent::FeeCharges);
    let detail = mismatch
        .detail()
        .expect("sequence mismatches carry first-divergence detail");
    assert_eq!(detail.ordinary_count, 0);
    assert_eq!(detail.candidate_count, 1);
    assert_eq!(detail.first_diff_index, 0);
    assert_eq!(detail.ordinary_element_hash, 0);
    assert_ne!(detail.candidate_element_hash, 0);
    assert!(mismatch.to_string().contains("first diff at index 0"));
}

#[test]
fn pre_aspidochelone_nonce_mutation_is_part_of_the_environment_artifact() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks = settings
        .hardforks
        .without_activation(Hardfork::HfAspidochelone);
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            settings,
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load context");
    let before_nonce = engine.nonce_data();
    let before = CanonicalExecutionArtifact::capture(
        &engine,
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture before random");

    engine.runtime_get_random().expect("legacy GetRandom");
    engine.pop().expect("discard random result");
    assert_eq!(engine.random_times(), 0);
    assert_ne!(engine.nonce_data(), before_nonce);
    let after = CanonicalExecutionArtifact::capture(
        &engine,
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("capture after random");

    assert_eq!(
        before
            .compare(&after)
            .expect_err("mutated nonce must differ")
            .component(),
        ExecutionArtifactComponent::Environment
    );
}

#[test]
fn engine_capture_bounds_fee_observations_before_retaining_the_artifact() {
    let engine = executed_engine();
    let mut journal = ExecutionObservationJournal::new();
    journal
        .record_fee_charge(1)
        .expect("bounded fee observation");
    let limits = ExecutionArtifactLimits {
        max_fee_charges: 0,
        ..ExecutionArtifactLimits::DEFAULT
    };

    assert_eq!(
        CanonicalExecutionArtifact::capture(&engine, &journal, limits)
            .expect_err("fee observations must be bounded"),
        ExecutionArtifactError::LimitExceeded {
            resource: "fee-charge observations",
            actual: 1,
            maximum: 0,
        }
    );
}

fn active_engine_with_detached_calling_chain(
    calling_has_parent: bool,
) -> ApplicationEngine<NoNativeContractProvider> {
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
    let references = ReferenceCounter::new();
    let detached_parent = ApplicationExecutionContext::new_with_state(
        Script::new_relaxed(vec![OpCode::RET.byte()]),
        0,
        &references,
        ExecutionContextState::<EmptyCacheBacking>::new(),
    );
    let mut calling_state = ExecutionContextState::<EmptyCacheBacking>::new();
    if calling_has_parent {
        calling_state.calling_context = Some(detached_parent);
    }
    let detached_calling = ApplicationExecutionContext::new_with_state(
        Script::new_relaxed(vec![OpCode::RET.byte()]),
        0,
        &references,
        calling_state,
    );
    engine
        .load_script_with_state(
            Script::new_relaxed(vec![OpCode::RET.byte()]),
            -1,
            0,
            |state| state.calling_context = Some(detached_calling),
        )
        .expect("load active context");
    engine
}

#[test]
fn calling_context_parent_presence_is_compared_for_called_by_entry_witnesses() {
    let entry_called = active_engine_with_detached_calling_chain(false);
    let nested_called = active_engine_with_detached_calling_chain(true);
    let entry_artifact = CanonicalExecutionArtifact::capture(
        &entry_called,
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("entry-called artifact");
    let nested_artifact = CanonicalExecutionArtifact::capture(
        &nested_called,
        &ExecutionObservationJournal::new(),
        ExecutionArtifactLimits::DEFAULT,
    )
    .expect("nested-called artifact");

    assert_eq!(
        entry_artifact
            .compare(&nested_artifact)
            .expect_err("CalledByEntry-visible chain must differ")
            .component(),
        ExecutionArtifactComponent::InvocationStack
    );
}

fn observed_call_access() -> ContractCallAccess {
    ContractCallAccess::new(
        ContractCallKind::Dynamic,
        ContractResolutionIdentity::new(UInt160::from([0x42; 20]), 17, 3, 0x1020_3040),
        12,
        "observed",
        CallFlags::ALL,
        1,
        1,
    )
}

fn capture_with_journal(
    engine: &ApplicationEngine<NoNativeContractProvider>,
    journal: &ExecutionObservationJournal,
) -> CanonicalExecutionArtifact {
    CanonicalExecutionArtifact::capture(engine, journal, ExecutionArtifactLimits::DEFAULT)
        .expect("bounded test artifact")
}

#[test]
fn call_observation_snapshots_joint_argument_and_result_aliases_before_mutation() {
    let engine = executed_engine();
    let live = StackItem::from_buffer(vec![0x11, 0x22]);
    let mut observed = ExecutionObservationJournal::new();
    observed
        .record_call(
            observed_call_access(),
            vec![live.clone()],
            CallObservationOutcome::Returned(vec![live.clone()]),
        )
        .expect("record aliased call");

    let StackItem::Buffer(buffer) = &live else {
        unreachable!("test buffer")
    };
    buffer.set(0, 0xff).expect("mutate after observation");

    let expected_value = StackItem::from_buffer(vec![0x11, 0x22]);
    let mut expected = ExecutionObservationJournal::new();
    expected
        .record_call(
            observed_call_access(),
            vec![expected_value.clone()],
            CallObservationOutcome::Returned(vec![expected_value]),
        )
        .expect("record expected call");

    assert_eq!(
        capture_with_journal(&engine, &observed).compare(&capture_with_journal(&engine, &expected)),
        Ok(())
    );

    let mut split = ExecutionObservationJournal::new();
    split
        .record_call(
            observed_call_access(),
            vec![StackItem::from_buffer(vec![0x11, 0x22])],
            CallObservationOutcome::Returned(vec![StackItem::from_buffer(vec![0x11, 0x22])]),
        )
        .expect("record equal but unaliased call");
    assert_eq!(
        capture_with_journal(&engine, &expected)
            .compare(&capture_with_journal(&engine, &split))
            .expect_err("cross-boundary alias topology must be compared")
            .component(),
        ExecutionArtifactComponent::Calls
    );
}

#[test]
fn successive_call_observations_retain_distinct_mutable_versions_and_cycles() {
    let engine = executed_engine();
    let live_buffer = StackItem::from_buffer(vec![1]);
    let live_cycle = StackItem::from_array(vec![live_buffer.clone()]);
    let StackItem::Array(live_array) = &live_cycle else {
        unreachable!("test array")
    };
    live_array
        .push(live_cycle.clone())
        .expect("create self-cycle");

    let mut observed = ExecutionObservationJournal::new();
    observed
        .record_call(
            observed_call_access(),
            vec![live_cycle.clone()],
            CallObservationOutcome::Returned(vec![live_cycle.clone()]),
        )
        .expect("record first version");
    let StackItem::Buffer(buffer) = &live_buffer else {
        unreachable!("test buffer")
    };
    buffer.set(0, 2).expect("mutate version");
    observed
        .record_call(
            observed_call_access(),
            vec![live_cycle.clone()],
            CallObservationOutcome::Returned(vec![live_cycle.clone()]),
        )
        .expect("record second version");
    live_array.clear().expect("break source cycle");

    fn cyclic_version(byte: u8) -> StackItem {
        let root = StackItem::from_array(vec![StackItem::from_buffer(vec![byte])]);
        let StackItem::Array(array) = &root else {
            unreachable!("test array")
        };
        array.push(root.clone()).expect("create expected cycle");
        root
    }

    let first = cyclic_version(1);
    let second = cyclic_version(2);
    let mut expected = ExecutionObservationJournal::new();
    for value in [&first, &second] {
        expected
            .record_call(
                observed_call_access(),
                vec![value.clone()],
                CallObservationOutcome::Returned(vec![value.clone()]),
            )
            .expect("record expected version");
    }
    for value in [first, second] {
        let StackItem::Array(array) = value else {
            unreachable!("test array")
        };
        array.clear().expect("break expected cycle");
    }

    assert_eq!(
        capture_with_journal(&engine, &observed).compare(&capture_with_journal(&engine, &expected)),
        Ok(())
    );
}

#[test]
fn faulted_call_snapshots_joint_argument_exception_aliases_and_cycles() {
    let engine = executed_engine();
    let live = StackItem::from_array(vec![StackItem::from_buffer(vec![0x31])]);
    let StackItem::Array(live_array) = &live else {
        unreachable!("test array")
    };
    live_array.push(live.clone()).expect("create source cycle");
    let mut observed = ExecutionObservationJournal::new();
    observed
        .record_call(
            observed_call_access(),
            vec![live.clone()],
            CallObservationOutcome::Fault {
                message: "fault".to_owned(),
                exception: Some(live.clone()),
            },
        )
        .expect("record faulted call");
    live_array.clear().expect("mutate source after observation");

    let expected_value = StackItem::from_array(vec![StackItem::from_buffer(vec![0x31])]);
    let StackItem::Array(expected_array) = &expected_value else {
        unreachable!("test array")
    };
    expected_array
        .push(expected_value.clone())
        .expect("create expected cycle");
    let mut expected = ExecutionObservationJournal::new();
    expected
        .record_call(
            observed_call_access(),
            vec![expected_value.clone()],
            CallObservationOutcome::Fault {
                message: "fault".to_owned(),
                exception: Some(expected_value.clone()),
            },
        )
        .expect("record expected faulted call");
    expected_array.clear().expect("break expected cycle");

    assert_eq!(
        capture_with_journal(&engine, &observed).compare(&capture_with_journal(&engine, &expected)),
        Ok(())
    );
}

#[test]
fn context_and_diagnostic_observations_snapshot_their_root_aliases() {
    let engine = executed_engine();
    let live = StackItem::from_buffer(vec![0x44]);
    let mut observed = ExecutionObservationJournal::new();
    observed
        .record_context(
            HostContextAccess::Notifications,
            ContextObservationValue::StackItems(vec![live.clone(), live.clone()]),
        )
        .expect("record context stack");
    observed
        .record_diagnostic(
            DiagnosticObservationKind::PostInstruction,
            Some(UInt160::from([0x33; 20])),
            Some(7),
            vec![OpCode::NOP.byte()],
            vec![live.clone(), live.clone()],
        )
        .expect("record diagnostic stack");
    let StackItem::Buffer(buffer) = &live else {
        unreachable!("test buffer")
    };
    buffer.set(0, 0x99).expect("mutate after observations");

    let expected_value = StackItem::from_buffer(vec![0x44]);
    let mut expected = ExecutionObservationJournal::new();
    expected
        .record_context(
            HostContextAccess::Notifications,
            ContextObservationValue::StackItems(vec![
                expected_value.clone(),
                expected_value.clone(),
            ]),
        )
        .expect("record expected context stack");
    expected
        .record_diagnostic(
            DiagnosticObservationKind::PostInstruction,
            Some(UInt160::from([0x33; 20])),
            Some(7),
            vec![OpCode::NOP.byte()],
            vec![expected_value.clone(), expected_value],
        )
        .expect("record expected diagnostic stack");

    assert_eq!(
        capture_with_journal(&engine, &observed).compare(&capture_with_journal(&engine, &expected)),
        Ok(())
    );
}

#[test]
fn journal_rejects_counts_stack_roots_and_witness_fault_bytes_before_retention() {
    let mut no_calls = ExecutionObservationJournal::with_limits(ExecutionArtifactLimits {
        max_calls: 0,
        ..ExecutionArtifactLimits::DEFAULT
    });
    assert_eq!(
        no_calls
            .record_call(
                observed_call_access(),
                vec![StackItem::Null],
                CallObservationOutcome::Returned(vec![]),
            )
            .expect_err("call count must be checked before snapshotting"),
        ExecutionArtifactError::LimitExceeded {
            resource: "calls",
            actual: 1,
            maximum: 0,
        }
    );

    let mut one_root = ExecutionObservationJournal::with_limits(ExecutionArtifactLimits {
        max_stack_roots: 1,
        ..ExecutionArtifactLimits::DEFAULT
    });
    assert_eq!(
        one_root
            .record_context(
                HostContextAccess::Notifications,
                ContextObservationValue::StackItems(vec![StackItem::Null, StackItem::Null]),
            )
            .expect_err("aggregate roots must be bounded at observation time"),
        ExecutionArtifactError::LimitExceeded {
            resource: "stack roots",
            actual: 2,
            maximum: 1,
        }
    );
    one_root
        .record_context(
            HostContextAccess::Notifications,
            ContextObservationValue::StackItems(vec![StackItem::Null]),
        )
        .expect("failed insertion must not consume the root budget");

    let mut one_node = ExecutionObservationJournal::with_limits(ExecutionArtifactLimits {
        max_stack_nodes: 1,
        ..ExecutionArtifactLimits::DEFAULT
    });
    one_node
        .record_context(
            HostContextAccess::Notifications,
            ContextObservationValue::StackItems(vec![StackItem::from_buffer(vec![1])]),
        )
        .expect("first embedded node");
    assert!(matches!(
        one_node.record_diagnostic(
            DiagnosticObservationKind::PostInstruction,
            None,
            None,
            vec![],
            vec![StackItem::from_buffer(vec![2])],
        ),
        Err(ExecutionArtifactError::LimitExceeded {
            resource: "stack graph nodes",
            actual: 2,
            maximum: 1,
        })
    ));
    one_node
        .record_diagnostic(
            DiagnosticObservationKind::PostInstruction,
            None,
            None,
            vec![],
            vec![StackItem::Null],
        )
        .expect("failed insertion must not consume the node budget");

    let mut short_faults = ExecutionObservationJournal::with_limits(ExecutionArtifactLimits {
        max_retained_bytes: 3,
        ..ExecutionArtifactLimits::DEFAULT
    });
    assert_eq!(
        short_faults
            .record_witness(
                UInt160::zero(),
                WitnessObservationOutcome::Fault("four".to_owned()),
            )
            .expect_err("witness fault text must be byte bounded"),
        ExecutionArtifactError::LimitExceeded {
            resource: "retained bytes",
            actual: 4,
            maximum: 3,
        }
    );
    short_faults
        .record_witness(
            UInt160::zero(),
            WitnessObservationOutcome::Fault("ok".to_owned()),
        )
        .expect("failed insertion must not consume the byte budget");
}

#[test]
fn final_capture_rechecks_witness_pending_call_and_iterator_bounds_before_snapshots() {
    let mut witness_journal = ExecutionObservationJournal::new();
    witness_journal
        .record_witness(
            UInt160::zero(),
            WitnessObservationOutcome::Fault("w".repeat(1024 * 1024)),
        )
        .expect("default journal accepts bounded fault");
    let witness_error = CanonicalExecutionArtifact::capture(
        &executed_engine(),
        &witness_journal,
        ExecutionArtifactLimits {
            max_retained_bytes: 512 * 1024,
            ..ExecutionArtifactLimits::DEFAULT
        },
    )
    .expect_err("final artifact must independently charge witness text");
    assert!(matches!(
        witness_error,
        ExecutionArtifactError::LimitExceeded {
            resource: "retained bytes",
            maximum: 524_288,
            ..
        }
    ));

    let mut pending = executed_engine();
    pending.queue_contract_call_from_native(
        UInt160::from([1; 20]),
        UInt160::from([2; 20]),
        "pending",
        vec![StackItem::from_buffer(vec![0; 1024])],
    );
    assert_eq!(
        CanonicalExecutionArtifact::capture(
            &pending,
            &ExecutionObservationJournal::new(),
            ExecutionArtifactLimits {
                max_calls: 0,
                ..ExecutionArtifactLimits::DEFAULT
            },
        )
        .expect_err("pending calls must be counted before payload cloning"),
        ExecutionArtifactError::LimitExceeded {
            resource: "calls",
            actual: 1,
            maximum: 0,
        }
    );

    let mut iterating = executed_engine();
    iterating
        .create_storage_iterator(vec![(
            StorageKey::new(17, b"key".to_vec()),
            StorageItem::from_bytes(vec![0; 1024]),
        )])
        .expect("create test iterator");
    assert_eq!(
        CanonicalExecutionArtifact::capture(
            &iterating,
            &ExecutionObservationJournal::new(),
            ExecutionArtifactLimits {
                max_iterator_rows: 0,
                ..ExecutionArtifactLimits::DEFAULT
            },
        )
        .expect_err("iterator rows must be counted before payload cloning"),
        ExecutionArtifactError::LimitExceeded {
            resource: "iterator rows",
            actual: 1,
            maximum: 0,
        }
    );
}

#[path = "storage_ranges.rs"]
mod storage_range_tests;
