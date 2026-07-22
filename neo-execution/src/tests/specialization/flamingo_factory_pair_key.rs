use super::*;
use crate::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use crate::diagnostic::NoDiagnostic;
use crate::native_contract_provider::NoNativeContractProvider;
use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_payloads::Block;
use neo_primitives::Hardfork;
use neo_storage::DataCache;
use neo_vm::{
    CandidateAuthority, ExecutionEngine, ExecutionEngineLimits, HardforkPlanState,
    InstructionCount, OpCode, ReferenceCounter, Script, Slot, StackItemType, VmState,
};

const PROFILE_HEIGHT: u32 = 1_887_001;

fn mainnet_hardforks_at(settings: &ProtocolSettings, height: u32) -> HardforkTableIdentity {
    Hardfork::ALL
        .into_iter()
        .fold(HardforkTableIdentity::unconfigured(), |table, hardfork| {
            let state = match settings.hardforks.activation_height(hardfork) {
                None => HardforkPlanState::Unconfigured,
                Some(activation_height) if height >= activation_height => {
                    HardforkPlanState::Active { activation_height }
                }
                Some(activation_height) => HardforkPlanState::Pending { activation_height },
            };
            table.with_state(hardfork, state)
        })
}

fn ordinary_pair_key_with_gas_limit(
    token_a: &[u8],
    token_b: &[u8],
    gas_limit: i64,
) -> (VmState, u64, i64, Option<StackItem>) {
    let settings = ProtocolSettings::default();
    assert_eq!(settings.network, MAINNET_MAGIC);
    let mut block = Block::new();
    block.header.set_index(PROFILE_HEIGHT);
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            Some(block),
            settings,
            gas_limit,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    let mut context = engine
        .load_script_with_state(
            Script::new_relaxed(SCRIPT_BYTES.to_vec()),
            -1,
            FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize,
            |_| {},
        )
        .expect("exact helper script loads");
    let reference_counter = context.reference_counter().clone();
    context.set_static_fields(Some(Slot::new(2, reference_counter)));
    context
        .store_static_field(1, StackItem::from_buffer(vec![0xFF]))
        .expect("static prefix initializes");
    context
        .push(StackItem::from_byte_string(token_b.to_vec()))
        .expect("tokenB push");
    context
        .push(StackItem::from_byte_string(token_a.to_vec()))
        .expect("tokenA push");

    let state = engine.execute_allow_fault();
    let result = engine.result_stack().peek(0).ok().cloned();
    (
        state,
        engine.instructions_executed(),
        engine.gas_consumed(),
        result,
    )
}

fn ordinary_pair_key(token_a: &[u8], token_b: &[u8]) -> (VmState, u64, i64, StackItem) {
    let (state, instructions, gas, result) =
        ordinary_pair_key_with_gas_limit(token_a, token_b, TEST_MODE_GAS);
    (
        state,
        instructions,
        gas,
        result.expect("HALT helper result"),
    )
}

fn assert_matches_ordinary(token_a: [u8; 20], token_b: [u8; 20]) {
    let artifact = try_flamingo_pair_key(
        &[
            StackItem::from_byte_string(token_a.to_vec()),
            StackItem::from_byte_string(token_b.to_vec()),
        ],
        &StackItem::from_buffer(vec![0xFF]),
        false,
        false,
    )
    .expect("exact invocation is eligible");
    let (state, instructions, gas_datoshi, ordinary) = ordinary_pair_key(&token_a, &token_b);

    assert_eq!(state, VmState::Halt);
    assert_eq!(instructions, artifact.instructions());
    assert_eq!(gas_datoshi, artifact.gas_units() as i64 * 30);
    assert_eq!(ordinary.stack_item_type(), StackItemType::Buffer);
    assert_eq!(artifact.result().stack_item_type(), StackItemType::Buffer);
    assert_eq!(
        ordinary.as_bytes().expect("ordinary Buffer bytes"),
        artifact
            .result()
            .as_bytes()
            .expect("specialized Buffer bytes")
    );
}

#[test]
fn exact_contract_is_shadow_only_and_declares_complete_pure_accounting() {
    let settings = ProtocolSettings::default();
    assert_eq!(settings.network, MAINNET_MAGIC);
    let hardforks = mainnet_hardforks_at(&settings, PROFILE_HEIGHT);
    let candidate = flamingo_pair_key_candidate(hardforks).expect("candidate contract");
    let execution = candidate.identity().execution();

    assert_eq!(candidate.authority(), CandidateAuthority::ShadowOnly);
    assert_eq!(
        candidate.identity().candidate_id(),
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID
    );
    assert_eq!(execution.entry_ip(), FLAMINGO_FACTORY_PAIR_KEY_ENTRY);
    assert_eq!(execution.script_len(), 1_281);
    assert_eq!(execution.hardforks(), hardforks);
    assert_eq!(
        UInt160::from_bytes(execution.script_hash()).expect("script Hash160"),
        UInt160::from_str("0x0993009a4e794f2e2071fb9489eef5aff390ea97").expect("known script hash")
    );
    assert_eq!(
        Crypto::hash160(execution.script_bytes()),
        *execution.script_hash()
    );
    assert_eq!(
        candidate.instruction_count(),
        InstructionCount::Decision {
            decision: 0,
            when_true: TRUE_BRANCH_INSTRUCTIONS,
            when_false: FALSE_BRANCH_INSTRUCTIONS,
        }
    );
    assert_eq!(candidate.gas_steps().len(), 1);
    assert_eq!(candidate.effects().stack().peak_reference_count_delta(), 3);
    assert!(
        candidate
            .eligibility()
            .context()
            .contains(&ContextDependency::FeeWhitelist { expected: false })
    );
    assert!(
        candidate
            .eligibility()
            .context()
            .contains(&ContextDependency::InternalCallFrame)
    );
    assert!(candidate.state().point_reads().is_empty());
    assert!(candidate.state().range_reads().is_empty());
    assert!(candidate.state().native_reads().is_empty());
    assert!(candidate.effects().host().is_empty());
}

#[test]
fn both_orderings_and_equal_values_match_the_ordinary_vm() {
    assert_matches_ordinary([0x11; 20], [0x22; 20]);
    assert_matches_ordinary([0xF0; 20], [0x01; 20]);
    assert_matches_ordinary([0x44; 20], [0x44; 20]);

    let mut low = [0u8; 20];
    let mut high = [0u8; 20];
    low[0] = 0xFF;
    high[19] = 1;
    assert_matches_ordinary(low, high);
}

#[test]
fn deterministic_randomized_values_match_the_ordinary_vm() {
    let mut state = 0xA5A5_1F2E_8877_6655u64;
    for _ in 0..512 {
        let mut token_a = [0u8; 20];
        let mut token_b = [0u8; 20];
        for byte in token_a.iter_mut().chain(token_b.iter_mut()) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        assert_matches_ordinary(token_a, token_b);
    }
}

fn internal_call_engine(limits: ExecutionEngineLimits) -> (ExecutionEngine<()>, StackItem, usize) {
    let mut engine = ExecutionEngine::<()>::new_with_limits(None, ReferenceCounter::new(), limits);
    engine
        .load_script(Script::new_relaxed(SCRIPT_BYTES.to_vec()), -1, 0)
        .expect("factory caller loads");

    let mut caller = engine.current_context().expect("caller context").clone();
    let reference_counter = caller.reference_counter().clone();
    caller.set_static_fields(Some(Slot::new(2, reference_counter)));
    caller
        .store_static_field(1, StackItem::from_buffer(vec![0xFF]))
        .expect("static prefix initializes");
    let sentinel = StackItem::from_i64(0x1234);
    caller.push(sentinel.clone()).expect("sentinel push");
    caller
        .push(StackItem::from_byte_string(vec![0x22; 20]))
        .expect("tokenB push");
    caller
        .push(StackItem::from_byte_string(vec![0x11; 20]))
        .expect("tokenA push");
    let references_before = engine.reference_counter().count();

    let callee = caller
        .clone_with_position(FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize)
        .expect("internal CALL clone");
    assert!(caller.shares_evaluation_stack_with(&callee));
    assert!(caller.shares_static_fields_with(&callee));
    assert_eq!(callee.rvcount(), 0);
    assert!(callee.local_variables().is_none());
    assert!(callee.arguments().is_none());
    assert!(callee.try_stack().is_none());
    engine.load_context(callee).expect("callee loads");

    (engine, sentinel, references_before)
}

#[test]
fn ordinary_internal_call_shares_frame_state_and_peaks_three_references() {
    let (mut engine, sentinel, references_before) =
        internal_call_engine(ExecutionEngineLimits::DEFAULT);
    engine.enable_execution_profiling();

    while engine.invocation_stack().len() > 1 {
        engine.execute_next().expect("helper instruction succeeds");
    }

    let profile = engine.execution_profile().expect("profile enabled");
    assert_eq!(profile.max_reference_count(), references_before as u64 + 3);
    assert_eq!(profile.opcode_count(OpCode::INITSLOT), 1);
    let caller = engine.current_context().expect("caller remains");
    assert_eq!(caller.evaluation_stack().len(), 2);
    assert_eq!(caller.peek(1).expect("sentinel remains"), sentinel);
    assert_eq!(
        caller
            .peek(0)
            .expect("pair key remains")
            .as_bytes()
            .expect("pair key bytes"),
        [vec![0xFF], vec![0x11; 20], vec![0x22; 20]].concat()
    );
    assert_eq!(
        caller
            .load_static_field(1)
            .expect("static prefix remains")
            .as_bytes()
            .expect("prefix bytes"),
        [0xFF]
    );
}

#[test]
fn transient_reference_limit_boundary_requires_ordinary_fallback() {
    let (_, _, references_before) = internal_call_engine(ExecutionEngineLimits::DEFAULT);
    let mut permitted_limits = ExecutionEngineLimits::DEFAULT;
    permitted_limits.max_stack_size =
        u32::try_from(references_before + 3).expect("small test peak");
    let (mut permitted, _, permitted_baseline) = internal_call_engine(permitted_limits);
    assert_eq!(permitted_baseline, references_before);
    while permitted.invocation_stack().len() > 1 {
        permitted
            .execute_next()
            .expect("exact peak at MaxStackSize is permitted");
    }

    let mut rejected_limits = ExecutionEngineLimits::DEFAULT;
    rejected_limits.max_stack_size = u32::try_from(references_before + 2).expect("small test peak");
    let (mut rejected, _, rejected_baseline) = internal_call_engine(rejected_limits);
    assert_eq!(rejected_baseline, references_before);
    let error = loop {
        match rejected.execute_next() {
            Ok(()) => assert!(
                rejected.invocation_stack().len() > 1,
                "ordinary helper must fault before returning"
            ),
            Err(error) => break error,
        }
    };
    assert!(error.to_string().contains("MaxStackSize exceed"));
}

#[test]
fn insufficient_aggregate_gas_requires_ordinary_per_opcode_fault() {
    for (token_a, token_b) in [([0x11; 20], [0x22; 20]), ([0xF0; 20], [0x01; 20])] {
        let artifact = try_flamingo_pair_key(
            &[
                StackItem::from_byte_string(token_a.to_vec()),
                StackItem::from_byte_string(token_b.to_vec()),
            ],
            &StackItem::from_buffer(vec![0xFF]),
            false,
            false,
        )
        .expect("eligible branch");
        let exact_gas = artifact.gas_units() as i64 * 30;
        let (exact_state, _, exact_consumed, exact_result) =
            ordinary_pair_key_with_gas_limit(&token_a, &token_b, exact_gas);
        assert_eq!(exact_state, VmState::Halt);
        assert_eq!(exact_consumed, exact_gas);
        assert!(exact_result.is_some());

        let (fault_state, fault_instructions, fault_consumed, _) =
            ordinary_pair_key_with_gas_limit(&token_a, &token_b, exact_gas - 1);
        assert_eq!(fault_state, VmState::Fault);
        assert!(fault_instructions <= artifact.instructions());
        assert!(fault_consumed > exact_gas - 1);
    }
}

#[test]
fn artifact_uses_one_fresh_buffer_and_exact_branch_charges() {
    let a = StackItem::from_byte_string(vec![0x11; 20]);
    let b = StackItem::from_byte_string(vec![0x22; 20]);
    let first = try_flamingo_pair_key(
        &[a.clone(), b.clone()],
        &StackItem::from_buffer(vec![0xFF]),
        false,
        false,
    )
    .expect("ascending invocation");
    let second = try_flamingo_pair_key(&[b, a], &StackItem::from_buffer(vec![0xFF]), false, false)
        .expect("descending invocation");

    assert!(first.lower_first());
    assert_eq!(first.instructions(), TRUE_BRANCH_INSTRUCTIONS);
    assert_eq!(first.gas_units(), TRUE_BRANCH_GAS_UNITS);
    assert!(!second.lower_first());
    assert_eq!(second.instructions(), FALSE_BRANCH_INSTRUCTIONS);
    assert_eq!(second.gas_units(), FALSE_BRANCH_GAS_UNITS);
    let (StackItem::Buffer(first_buffer), StackItem::Buffer(second_buffer)) =
        (first.result(), second.result())
    else {
        panic!("candidate results must be Buffers");
    };
    assert_ne!(first_buffer.id(), second_buffer.id());
}

#[test]
fn unsupported_types_lengths_static_state_and_diagnostics_fail_closed() {
    let bytes = StackItem::from_byte_string(vec![0x11; 20]);
    let prefix = StackItem::from_buffer(vec![0xFF]);
    let unsupported = vec![
        StackItem::Null,
        StackItem::from_bool(true),
        StackItem::from_i64(1),
        StackItem::from_buffer(vec![0x11; 20]),
        StackItem::from_array(Vec::new()),
        StackItem::from_struct(Vec::new()),
    ];
    for item in unsupported {
        assert_eq!(
            try_flamingo_pair_key(&[item, bytes.clone()], &prefix, false, false),
            Err(FlamingoPairKeyEligibilityError::Argument { index: 0 })
        );
    }
    for length in [0, 19, 21, 31, 32, 33] {
        assert_eq!(
            try_flamingo_pair_key(
                &[StackItem::from_byte_string(vec![0; length]), bytes.clone(),],
                &prefix,
                false,
                false,
            ),
            Err(FlamingoPairKeyEligibilityError::Argument { index: 0 })
        );
    }
    assert_eq!(
        try_flamingo_pair_key(std::slice::from_ref(&bytes), &prefix, false, false),
        Err(FlamingoPairKeyEligibilityError::Arity { actual: 1 })
    );
    assert_eq!(
        try_flamingo_pair_key(
            &[bytes.clone(), bytes.clone()],
            &StackItem::from_byte_string(vec![0xFF]),
            false,
            false,
        ),
        Err(FlamingoPairKeyEligibilityError::StaticPrefix)
    );
    assert_eq!(
        try_flamingo_pair_key(
            &[bytes.clone(), bytes.clone()],
            &StackItem::from_buffer(vec![0xFE]),
            false,
            false,
        ),
        Err(FlamingoPairKeyEligibilityError::StaticPrefix)
    );
    assert_eq!(
        try_flamingo_pair_key(&[bytes.clone(), bytes.clone()], &prefix, true, false),
        Err(FlamingoPairKeyEligibilityError::DiagnosticsEnabled)
    );
    assert_eq!(
        try_flamingo_pair_key(&[bytes.clone(), bytes], &prefix, false, true),
        Err(FlamingoPairKeyEligibilityError::FeeWhitelisted)
    );
}
