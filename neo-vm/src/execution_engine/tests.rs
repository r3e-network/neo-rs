//
// tests.rs - Unit tests for ExecutionEngine
//

use super::*;

#[allow(dead_code)]
mod execution_engine_tests {
    use super::*;
    use crate::OpCode;

    #[test]
    fn test_execution_engine_creation() {
        let engine = ExecutionEngine::new(None);
        assert_eq!(engine.state(), VMState::BREAK);
        assert!(engine.invocation_stack().is_empty());
        assert!(engine.result_stack().is_empty());
        assert!(engine.uncaught_exception().is_none());
    }

    #[test]
    fn test_load_script() {
        let mut engine = ExecutionEngine::new(None);

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
        let mut engine = ExecutionEngine::new(None);
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
        let mut engine = ExecutionEngine::new(None);

        // Test jump_table getter
        let _jump_table = engine.jump_table();

        // Test jump_table_mut getter
        let _jump_table_mut = engine.jump_table_mut();

        // Test set_jump_table
        let new_jump_table = JumpTable::new();
        engine.set_jump_table(new_jump_table);
    }

    #[test]
    fn test_stack_operations() {
        let mut engine = ExecutionEngine::new(None);

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
        let mut engine = ExecutionEngine::new(None);

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
    fn test_gas_tracking_basic() {
        let mut engine = ExecutionEngine::new(None);

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
        let mut engine = ExecutionEngine::new(None);

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
        let mut engine = ExecutionEngine::new(None);

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
        let mut engine = ExecutionEngine::new(None);

        // Add some gas
        engine.add_gas_consumed(500).expect("Should add gas");
        assert_eq!(engine.gas_consumed(), 500);

        // Reset gas consumed
        engine.reset_gas_consumed();
        assert_eq!(engine.gas_consumed(), 0);
    }

    #[test]
    fn test_gas_tracking_edge_cases() {
        let mut engine = ExecutionEngine::new(None);

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

    #[test]
    fn max_stack_size_is_enforced_after_execution() {
        let mut engine = ExecutionEngine::new(None);
        // 2049 pushes exceed the default MaxStackSize (2048). C# FAULTs after
        // the instruction that crosses the limit.
        let script_bytes = vec![OpCode::PUSH1.byte(); 2049];
        engine
            .load_script(Script::new_relaxed(script_bytes), -1, 0)
            .expect("load script");
        assert_eq!(engine.execute(), VMState::FAULT);
    }

    #[test]
    fn nested_syscall_reaches_the_inner_handler() {
        struct Host {
            outer: u32,
            inner: u32,
            inner_calls: u32,
        }
        impl InteropHost for Host {
            fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
                if hash == self.outer {
                    engine.on_syscall(self.inner)
                } else if hash == self.inner {
                    self.inner_calls = self.inner_calls.saturating_add(1);
                    Ok(())
                } else {
                    Err(VmError::invalid_operation_msg(format!(
                        "unexpected syscall {hash}"
                    )))
                }
            }
        }

        let mut engine = ExecutionEngine::new(None);
        let outer = engine
            .interop_service_mut()
            .expect("interop service")
            .register_host_descriptor("Test.Outer", 0, CallFlags::NONE)
            .expect("register outer");
        let inner = engine
            .interop_service_mut()
            .expect("interop service")
            .register_host_descriptor("Test.Inner", 0, CallFlags::NONE)
            .expect("register inner");
        let mut host = Host {
            outer,
            inner,
            inner_calls: 0,
        };
        let host_ptr = &mut host as *mut Host as *mut dyn InteropHost;
        unsafe {
            engine.set_interop_host(host_ptr);
        }
        let mut script_bytes = vec![OpCode::SYSCALL.byte()];
        script_bytes.extend_from_slice(&outer.to_le_bytes());
        script_bytes.push(OpCode::RET.byte());
        engine
            .load_script(Script::new_relaxed(script_bytes), -1, 0)
            .expect("load script");
        assert_eq!(engine.execute(), VMState::HALT);
        engine.clear_interop_host();
        assert_eq!(host.inner_calls, 1);
    }

    #[test]
    fn uncaught_throw_preserves_the_exception_object() {
        let mut engine = ExecutionEngine::new(None);
        let exception = StackItem::from_byte_string(b"BidToken NEP17 Transfer failed".to_vec());

        let result = engine.execute_throw(Some(exception.clone()));

        assert_eq!(result, Err(VmError::UnhandledException(exception.clone())));
        assert_eq!(engine.state(), VMState::FAULT);
        assert_eq!(engine.uncaught_exception(), Some(&exception));
    }
}
