//
// tests.rs - Unit tests for ExecutionEngine
//

use super::*;

#[allow(dead_code)]
mod tests {
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
}
