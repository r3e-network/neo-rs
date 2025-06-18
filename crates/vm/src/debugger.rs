//! Debugger for the Neo Virtual Machine.
//!
//! This module provides debugging functionality for the Neo VM.

use crate::execution_context::ExecutionContext;
use crate::execution_engine::{ExecutionEngine, VMState};
use crate::instruction::Instruction;
use crate::Error;
use crate::Result;
use std::collections::HashMap;

/// Represents a breakpoint in the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// The script hash.
    script_hash: Vec<u8>,

    /// The instruction pointer.
    instruction_pointer: usize,
}

impl Breakpoint {
    /// Creates a new breakpoint.
    pub fn new(script_hash: Vec<u8>, instruction_pointer: usize) -> Self {
        Self {
            script_hash,
            instruction_pointer,
        }
    }

    /// Gets the script hash.
    pub fn script_hash(&self) -> &[u8] {
        &self.script_hash
    }

    /// Gets the instruction pointer.
    pub fn instruction_pointer(&self) -> usize {
        self.instruction_pointer
    }
}

/// Represents a debugger for the VM.
pub struct Debugger {
    /// The execution engine.
    engine: ExecutionEngine,

    /// The breakpoints.
    breakpoints: HashMap<Vec<u8>, Vec<usize>>,
}

impl Debugger {
    /// Creates a new debugger with the specified execution engine.
    pub fn new(engine: ExecutionEngine) -> Self {
        Self {
            engine,
            breakpoints: HashMap::new(),
        }
    }

    /// Gets the execution engine.
    pub fn engine(&self) -> &ExecutionEngine {
        &self.engine
    }

    /// Gets a mutable reference to the execution engine.
    pub fn engine_mut(&mut self) -> &mut ExecutionEngine {
        &mut self.engine
    }

    /// Adds a breakpoint.
    pub fn add_breakpoint(&mut self, breakpoint: Breakpoint) {
        let script_hash = breakpoint.script_hash().to_vec();
        let instruction_pointer = breakpoint.instruction_pointer();

        self.breakpoints
            .entry(script_hash)
            .or_insert_with(Vec::new)
            .push(instruction_pointer);
    }

    /// Removes a breakpoint.
    pub fn remove_breakpoint(&mut self, breakpoint: &Breakpoint) {
        if let Some(breakpoints) = self.breakpoints.get_mut(breakpoint.script_hash()) {
            if let Some(index) = breakpoints.iter().position(|&ip| ip == breakpoint.instruction_pointer()) {
                breakpoints.remove(index);
            }
        }
    }

    /// Clears all breakpoints.
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Gets all breakpoints.
    pub fn breakpoints(&self) -> Vec<Breakpoint> {
        let mut result = Vec::new();

        for (script_hash, instruction_pointers) in &self.breakpoints {
            for &instruction_pointer in instruction_pointers {
                result.push(Breakpoint::new(script_hash.clone(), instruction_pointer));
            }
        }

        result
    }

    /// Checks if a breakpoint exists at the specified location.
    pub fn has_breakpoint(&self, script_hash: &[u8], instruction_pointer: usize) -> bool {
        if let Some(breakpoints) = self.breakpoints.get(script_hash) {
            breakpoints.contains(&instruction_pointer)
        } else {
            false
        }
    }

    /// Executes the VM until a breakpoint is hit or the VM halts.
    pub fn execute(&mut self) -> Result<VMState> {
        while self.engine.state() != VMState::HALT && self.engine.state() != VMState::FAULT {
            // Check if we're at a breakpoint
            if let Some(context) = self.engine.current_context() {
                let script_hash = context.script().hash();
                let instruction_pointer = context.instruction_pointer();

                if self.has_breakpoint(&script_hash, instruction_pointer) {
                    self.engine.set_state(VMState::BREAK);
                    return Ok(VMState::BREAK);
                }
            }

            // Execute the next instruction
            if let Err(err) = self.engine.execute_next() {
                return Err(err);
            }
        }

        Ok(self.engine.state())
    }

    /// Executes a single instruction.
    pub fn step(&mut self) -> Result<VMState> {
        if self.engine.state() == VMState::BREAK {
            self.engine.set_state(VMState::NONE);
        }

        if let Err(err) = self.engine.execute_next() {
            return Err(err);
        }

        self.engine.set_state(VMState::BREAK);
        Ok(VMState::BREAK)
    }

    /// Executes until the current context returns.
    pub fn step_out(&mut self) -> Result<VMState> {
        if self.engine.state() == VMState::BREAK {
            self.engine.set_state(VMState::NONE);
        }

        let current_context = self.engine.current_context().cloned();

        while self.engine.state() != VMState::HALT && self.engine.state() != VMState::FAULT {
            // Execute the next instruction
            if let Err(err) = self.engine.execute_next() {
                return Err(err);
            }

            // Check if we've stepped out of the current context
            if let Some(context) = &current_context {
                if self.engine.current_context().is_none() || self.engine.current_context().unwrap().script().hash() != context.script().hash() {
                    self.engine.set_state(VMState::BREAK);
                    return Ok(VMState::BREAK);
                }
            }
        }

        Ok(self.engine.state())
    }

    /// Executes until the next line in the current context.
    pub fn step_over(&mut self) -> Result<VMState> {
        if self.engine.state() == VMState::BREAK {
            self.engine.set_state(VMState::NONE);
        }

        let current_context = self.engine.current_context().cloned();
        let current_instruction_pointer = current_context.as_ref().map(|c| c.instruction_pointer());

        // Execute the next instruction
        if let Err(err) = self.engine.execute_next() {
            return Err(err);
        }

        // If the instruction was a call, execute until we return to the current context
        if let Some(context) = &current_context {
            if let Some(current_context) = self.engine.current_context() {
                if current_context.script().hash() != context.script().hash() {
                    // We've entered a new context, execute until we return
                    return self.step_out();
                }
            }
        }

        self.engine.set_state(VMState::BREAK);
        Ok(VMState::BREAK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op_code::OpCode;
    use crate::script::Script;

    #[test]
    fn test_debugger_creation() {
        let engine = ExecutionEngine::new(None);
        let debugger = Debugger::new(engine);

        assert_eq!(debugger.breakpoints().len(), 0);
    }

    #[test]
    fn test_debugger_breakpoints() {
        let engine = ExecutionEngine::new(None);
        let mut debugger = Debugger::new(engine);

        let script_hash = vec![1, 2, 3];
        let breakpoint = Breakpoint::new(script_hash.clone(), 10);

        debugger.add_breakpoint(breakpoint.clone());

        assert_eq!(debugger.breakpoints().len(), 1);
        assert_eq!(debugger.breakpoints()[0].script_hash(), &script_hash);
        assert_eq!(debugger.breakpoints()[0].instruction_pointer(), 10);

        assert!(debugger.has_breakpoint(&script_hash, 10));
        assert!(!debugger.has_breakpoint(&script_hash, 20));

        debugger.remove_breakpoint(&breakpoint);

        assert_eq!(debugger.breakpoints().len(), 0);
        assert!(!debugger.has_breakpoint(&script_hash, 10));
    }
}
