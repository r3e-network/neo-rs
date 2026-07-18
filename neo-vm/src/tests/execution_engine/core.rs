//
// tests.rs - Unit tests for ExecutionEngine
//

use super::*;

#[allow(dead_code)]
mod execution_engine_tests {
    use super::*;
    use crate::OpCode;
    use std::hint::black_box;
    use std::sync::Arc;
    use std::time::Instant;

    // The opt-in collector may consume at most 2.5x unprofiled wall time on
    // dispatch-only and stack-heavy stress scripts (150% profiler overhead).
    const PROFILER_TOTAL_TIME_BUDGET_BPS: u128 = 25_000;
    const OVERHEAD_SAMPLES: usize = 17;
    const OVERHEAD_BATCH_EXECUTIONS: usize = 16;

    fn run_profiled_script(script: &Arc<Script>) -> (VMState, crate::VmExecutionProfile) {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine.enable_execution_profiling();
        let context = engine
            .create_context_from_script_arc(Arc::clone(script), -1, 0)
            .expect("create profiled context");
        engine.load_context(context).expect("load profiled context");
        let state = engine.execute();
        let profile = engine.execution_profile().expect("execution profile");
        (state, profile)
    }

    fn execute_overhead_batch(script: &Arc<Script>, profiled: bool) -> u128 {
        let started = Instant::now();
        for _ in 0..OVERHEAD_BATCH_EXECUTIONS {
            let mut engine = ExecutionEngine::<()>::new(None);
            if profiled {
                engine.enable_execution_profiling();
            }
            let context = engine
                .create_context_from_script_arc(Arc::clone(script), -1, 0)
                .expect("create overhead context");
            engine.load_context(context).expect("load overhead context");
            black_box(engine.execute());
            if profiled {
                black_box(engine.execution_profile().expect("execution profile"));
            }
        }
        started.elapsed().as_nanos()
    }

    fn median(mut samples: Vec<u128>) -> u128 {
        samples.sort_unstable();
        samples[samples.len() / 2]
    }

    fn measure_profiler_ratio(script: &Arc<Script>) -> (u128, u128) {
        for _ in 0..3 {
            black_box(execute_overhead_batch(script, false));
            black_box(execute_overhead_batch(script, true));
        }

        let mut baseline = Vec::with_capacity(OVERHEAD_SAMPLES);
        let mut profiled = Vec::with_capacity(OVERHEAD_SAMPLES);
        for sample in 0..OVERHEAD_SAMPLES {
            if sample % 2 == 0 {
                baseline.push(execute_overhead_batch(script, false));
                profiled.push(execute_overhead_batch(script, true));
            } else {
                profiled.push(execute_overhead_batch(script, true));
                baseline.push(execute_overhead_batch(script, false));
            }
        }
        (median(baseline), median(profiled))
    }

    #[test]
    fn test_execution_engine_creation() {
        let engine = ExecutionEngine::<()>::new(None);
        assert_eq!(engine.state(), VMState::BREAK);
        assert!(engine.invocation_stack().is_empty());
        assert!(engine.result_stack().is_empty());
        assert!(engine.uncaught_exception().is_none());
    }

    #[test]
    fn test_load_script() {
        let mut engine = ExecutionEngine::<()>::new(None);

        let script_bytes = vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::ADD.byte(),
            OpCode::RET.byte(),
        ];
        let script = Script::new_relaxed(script_bytes);

        {
            let context = engine
                .load_script(script, -1, 0)
                .expect("VM operation should succeed");

            assert_eq!(context.instruction_pointer(), 0);
            assert_eq!(context.rvcount(), -1);
        }

        assert_eq!(engine.invocation_stack().len(), 1);
    }

    #[test]
    fn test_set_state() {
        let mut engine = ExecutionEngine::<()>::new(None);
        assert_eq!(engine.state(), VMState::BREAK);

        engine.set_state(VMState::NONE);
        assert_eq!(engine.state(), VMState::NONE);

        engine.set_state(VMState::HALT);
        assert_eq!(engine.state(), VMState::HALT);

        engine.set_state(VMState::FAULT);
        assert_eq!(engine.state(), VMState::FAULT);
    }

    #[test]
    fn test_jump_table_methods() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Test jump_table getter
        let _jump_table = engine.jump_table();

        // Test jump_table_mut getter
        let _jump_table_mut = engine.jump_table_mut();

        // Test set_jump_table
        let new_jump_table = JumpTable::<()>::new();
        engine.set_jump_table(new_jump_table);
    }

    #[test]
    fn test_stack_operations() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Create a script with a few instructions
        let script_bytes = vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::ADD.byte(),
            OpCode::RET.byte(),
        ];
        let script = Script::new_relaxed(script_bytes);

        // Load the script
        engine
            .load_script(script, -1, 0)
            .expect("VM operation should succeed");

        // Push some items onto the stack
        engine
            .push(StackItem::from_int(1))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(2))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(3))
            .expect("VM operation should succeed");

        // Peek at the items
        assert_eq!(
            engine
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(3)
        );
        assert_eq!(
            engine
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            engine
                .peek(2)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(1)
        );

        // Pop an item
        let item = engine.pop().unwrap();
        assert_eq!(
            item.as_int().expect("Operation failed"),
            num_bigint::BigInt::from(3)
        );

        // Peek again
        assert_eq!(
            engine
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("Operation failed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            engine
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("Operation failed"),
            num_bigint::BigInt::from(1)
        );
    }

    #[test]
    fn test_unload_context() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Create a script with a few instructions
        let script_bytes = vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::ADD.byte(),
            OpCode::RET.byte(),
        ];
        let script = Script::new_relaxed(script_bytes);

        // Load the script
        let _context = engine
            .load_script(script, -1, 0)
            .expect("VM operation should succeed");

        // Push some items onto the stack
        engine
            .push(StackItem::from_int(1))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(2))
            .expect("VM operation should succeed");

        // Remove the context
        let _context = engine
            .remove_context(0)
            .expect("VM operation should succeed");

        // Check that the invocation stack is empty
        assert!(engine.invocation_stack().is_empty());

        // Check that the VM state is HALT
        assert_eq!(engine.state(), VMState::HALT);
    }

    #[test]
    fn reset_execution_session_releases_context_slot_references() {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");

        {
            let context = engine
                .current_context_mut()
                .expect("context should be loaded");
            context
                .init_slot(1, 1)
                .expect("initialize local/argument slots");
            context
                .local_variables_mut()
                .expect("local slot should exist")
                .set(
                    0,
                    StackItem::from_array(vec![StackItem::from_i64(1), StackItem::from_i64(2)]),
                )
                .expect("store compound local");
        }

        assert!(
            engine.reference_counter().count() > 0,
            "slot initialization and compound storage must be reference-counted"
        );

        engine.reset_execution_session();

        assert!(engine.invocation_stack().is_empty());
        assert_eq!(engine.reference_counter().count(), 0);
    }

    #[test]
    fn execution_profile_attributes_repeated_entry_offsets_to_immutable_script() {
        let mut engine = ExecutionEngine::<()>::new(None);
        let script = Script::new_relaxed(vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::RET.byte(),
        ]);
        let script_hash = script.script_hash();

        engine.enable_execution_profiling();
        engine
            .load_script(script.clone(), -1, 0)
            .expect("load first profiled context");
        engine
            .load_script(script, -1, 1)
            .expect("load repeated profiled context");

        assert_eq!(engine.execute(), VMState::HALT);
        let profile = engine.execution_profile().expect("execution profile");
        assert_eq!(profile.total_instructions(), 5);
        assert_eq!(profile.other_script_instructions(), 0);
        assert_eq!(profile.other_script_context_loads(), 0);
        assert_eq!(profile.scripts().len(), 1);

        let script_profile = &profile.scripts()[0];
        assert_eq!(*script_profile.script_hash(), script_hash);
        assert_eq!(script_profile.script_len(), 3);
        assert_eq!(script_profile.instructions(), 5);
        assert_eq!(script_profile.context_loads(), 2);
        assert_eq!(script_profile.other_entry_context_loads(), 0);
        assert_eq!(script_profile.entry_points().len(), 2);
        assert_eq!(script_profile.entry_points()[0].entry_offset(), 0);
        assert_eq!(script_profile.entry_points()[0].context_loads(), 1);
        assert_eq!(script_profile.entry_points()[1].entry_offset(), 1);
        assert_eq!(script_profile.entry_points()[1].context_loads(), 1);
    }

    #[test]
    fn enabling_execution_profile_records_already_loaded_context() {
        let mut engine = ExecutionEngine::<()>::new(None);
        let script = Script::new_relaxed(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()]);
        engine
            .load_script(script, -1, 1)
            .expect("load context before profiling");

        engine.enable_execution_profiling();
        assert_eq!(engine.execute(), VMState::HALT);

        let profile = engine.execution_profile().expect("execution profile");
        assert_eq!(profile.scripts().len(), 1);
        assert_eq!(profile.scripts()[0].context_loads(), 1);
        assert_eq!(profile.scripts()[0].instructions(), 1);
        assert_eq!(profile.scripts()[0].entry_points()[0].entry_offset(), 1);
    }

    #[test]
    fn execution_profile_bounds_distinct_script_fingerprints() {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine.enable_execution_profiling();

        for discriminator in 0..65u8 {
            engine
                .load_script(
                    Script::new_relaxed(vec![OpCode::PUSHDATA1.byte(), 1, discriminator]),
                    -1,
                    0,
                )
                .expect("load bounded profile context");
        }

        let profile = engine.execution_profile().expect("execution profile");
        assert_eq!(profile.scripts().len(), 64);
        assert_eq!(profile.other_script_context_loads(), 1);
        assert_eq!(profile.other_script_instructions(), 0);
    }

    #[test]
    fn execution_profile_is_disabled_by_default_and_detached_on_reset() {
        let mut engine = ExecutionEngine::<()>::new(None);
        assert!(engine.execution_profile().is_none());
        assert!(!engine.result_stack().has_profile());

        engine
            .load_script(Script::new_relaxed(vec![OpCode::PUSH1.byte()]), -1, 0)
            .expect("load unprofiled context");
        assert!(
            !engine
                .current_context()
                .expect("unprofiled context")
                .evaluation_stack()
                .has_profile()
        );
        assert_eq!(engine.execute(), VMState::HALT);

        engine.reset_execution_session();
        engine.enable_execution_profiling();
        assert!(engine.execution_profile().is_some());
        assert!(engine.result_stack().has_profile());
        engine
            .load_script(Script::new_relaxed(vec![OpCode::PUSH2.byte()]), -1, 0)
            .expect("load profiled context");
        assert!(
            engine
                .current_context()
                .expect("profiled context")
                .evaluation_stack()
                .has_profile()
        );
        assert_eq!(engine.execute(), VMState::HALT);

        engine.reset_execution_session();
        assert!(engine.execution_profile().is_none());
        assert!(!engine.result_stack().has_profile());
        engine
            .load_script(Script::new_relaxed(vec![OpCode::PUSH3.byte()]), -1, 0)
            .expect("load next unprofiled context");
        assert!(
            !engine
                .current_context()
                .expect("next unprofiled context")
                .evaluation_stack()
                .has_profile()
        );
        assert_eq!(engine.execute(), VMState::HALT);
    }

    #[test]
    fn concurrent_profiled_engines_produce_identical_results_and_profiles() {
        const WORKERS: usize = 8;
        let mut script_bytes = Vec::with_capacity(2_001);
        for _ in 0..1_000 {
            script_bytes.push(OpCode::PUSH1.byte());
            script_bytes.push(OpCode::DROP.byte());
        }
        script_bytes.push(OpCode::RET.byte());
        let script = Arc::new(Script::new_relaxed(script_bytes));
        let expected = run_profiled_script(&script);

        std::thread::scope(|scope| {
            let workers = (0..WORKERS)
                .map(|_| {
                    let script = Arc::clone(&script);
                    scope.spawn(move || run_profiled_script(&script))
                })
                .collect::<Vec<_>>();
            for worker in workers {
                assert_eq!(worker.join().expect("profile worker"), expected);
            }
        });
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        ignore = "run with cargo test --release -p neo-vm execution_profiler_overhead_stays_within_budget -- --nocapture"
    )]
    fn execution_profiler_overhead_stays_within_budget() {
        assert!(
            !cfg!(debug_assertions),
            "the profiler overhead budget must be measured with optimized code"
        );

        let mut nop_bytes = vec![OpCode::NOP.byte(); 4_096];
        nop_bytes.push(OpCode::RET.byte());
        let nop_script = Arc::new(Script::new_relaxed(nop_bytes));

        let mut stack_bytes = Vec::with_capacity(4_097);
        for _ in 0..2_048 {
            stack_bytes.push(OpCode::PUSH1.byte());
            stack_bytes.push(OpCode::DROP.byte());
        }
        stack_bytes.push(OpCode::RET.byte());
        let stack_script = Arc::new(Script::new_relaxed(stack_bytes));

        for (workload, script) in [("nop", nop_script), ("stack", stack_script)] {
            let (baseline_ns, profiled_ns) = measure_profiler_ratio(&script);
            let ratio_bps = profiled_ns.saturating_mul(10_000) / baseline_ns.max(1);
            eprintln!(
                "VM profiler overhead: workload={workload} baseline_batch_ns={baseline_ns} profiled_batch_ns={profiled_ns} total_ratio={:.3}x budget={:.3}x",
                ratio_bps as f64 / 10_000.0,
                PROFILER_TOTAL_TIME_BUDGET_BPS as f64 / 10_000.0,
            );
            assert!(
                ratio_bps <= PROFILER_TOTAL_TIME_BUDGET_BPS,
                "{workload} profiler ratio {ratio_bps} bps exceeded {PROFILER_TOTAL_TIME_BUDGET_BPS} bps"
            );
        }
    }

    #[test]
    fn pickitem_struct_out_of_range_is_catchable() {
        let mut engine = ExecutionEngine::<()>::new(None);
        let script_bytes = vec![
            OpCode::TRY.byte(),
            9,
            0,
            OpCode::PUSH11.byte(),
            OpCode::NEWSTRUCT.byte(),
            OpCode::PUSH11.byte(),
            OpCode::PICKITEM.byte(),
            OpCode::ENDTRY.byte(),
            5,
            OpCode::DROP.byte(),
            OpCode::PUSH1.byte(),
            OpCode::RET.byte(),
            OpCode::PUSH2.byte(),
            OpCode::RET.byte(),
        ];

        engine
            .load_script(Script::new_relaxed(script_bytes), -1, 0)
            .expect("load test script");

        assert_eq!(engine.execute(), VMState::HALT);
        assert_eq!(engine.result_stack().len(), 1);
        assert_eq!(
            engine
                .result_stack()
                .peek(0)
                .expect("catch result")
                .as_int()
                .expect("integer result"),
            num_bigint::BigInt::from(1)
        );
    }

    #[test]
    fn test_gas_tracking_basic() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Initial gas consumed should be 0
        assert_eq!(engine.gas_consumed(), 0);

        // Default gas limit should be 20 GAS
        assert_eq!(engine.gas_limit(), DEFAULT_GAS_LIMIT);
        assert_eq!(engine.gas_limit(), 20_0000_0000);

        // Add some gas
        engine.add_gas_consumed(100).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 100);

        // Add more gas
        engine.add_gas_consumed(200).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 300);

        // Check gas remaining
        assert_eq!(engine.gas_remaining(), DEFAULT_GAS_LIMIT - 300);

        // Check not exhausted
        assert!(!engine.is_gas_exhausted());
    }

    #[test]
    fn test_gas_tracking_limit_exceeded() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Set a low gas limit for testing
        engine.set_gas_limit(1000);
        assert_eq!(engine.gas_limit(), 1000);

        // Add gas within limit
        engine.add_gas_consumed(500).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 500);

        // Add more gas to reach limit
        engine.add_gas_consumed(500).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 1000);
        assert!(engine.is_gas_exhausted());

        // Adding more gas should fail
        let result = engine.add_gas_consumed(1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::GasExhausted { .. }));
    }

    #[test]
    fn test_gas_tracking_refund() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Add some gas
        engine.add_gas_consumed(500).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 500);

        // Refund (negative) gas
        engine.add_gas_consumed(-200).expect("Should refund gas");
        assert_eq!(engine.gas_consumed(), 300);

        // Refund more than consumed - should clamp to 0
        engine.add_gas_consumed(-1000).expect("Should clamp to 0");
        assert_eq!(engine.gas_consumed(), 0);
    }

    #[test]
    fn test_gas_tracking_reset() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Add some gas
        engine.add_gas_consumed(500).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 500);

        // Reset gas consumed
        engine.reset_gas_consumed();
        assert_eq!(engine.gas_consumed(), 0);
    }

    #[test]
    fn test_gas_tracking_edge_cases() {
        let mut engine = ExecutionEngine::<()>::new(None);

        // Test adding 0 gas
        engine.add_gas_consumed(0).expect("Should handle 0");
        assert_eq!(engine.gas_consumed(), 0);

        // Test gas remaining when no gas consumed
        assert_eq!(engine.gas_remaining(), engine.gas_limit());

        // Test with exactly at limit
        engine.set_gas_limit(100);
        let result = engine.add_gas_consumed(100);
        assert!(result.is_ok());
        assert_eq!(engine.gas_consumed(), 100);
        assert!(engine.is_gas_exhausted());

        // Test gas remaining at 0
        assert_eq!(engine.gas_remaining(), 0);
    }
}
