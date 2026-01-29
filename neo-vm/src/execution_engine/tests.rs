//
// tests.rs - Unit tests for ExecutionEngine
//

use super::*;

#[allow(dead_code)]
mod execution_engine_tests {
    use super::*;
    use crate::op_code::OpCode;

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
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
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
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
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
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
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
}
