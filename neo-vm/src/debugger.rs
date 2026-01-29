//! Debugger implementation.
//!
//! This module provides the Debugger functionality exactly matching C# Neo.VM.Debugger.

// Matches C# using directives exactly:
// using System.Collections.Generic;

use crate::execution_engine::ExecutionEngine;
use crate::script::Script;
use crate::vm_state::VMState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// namespace Neo.VM -> public class Debugger
/// A simple debugger for `ExecutionEngine`.
pub struct Debugger {
    // private readonly ExecutionEngine _engine;
    engine: ExecutionEngine,

    // private readonly Dictionary<Script, HashSet<uint>> _breakPoints = new();
    break_points: HashMap<Arc<Script>, HashSet<u32>>,
}

impl Debugger {
    /// Create a debugger on the specified `ExecutionEngine`.
    /// public Debugger(ExecutionEngine engine)
    #[must_use]
    pub fn new(engine: ExecutionEngine) -> Self {
        Self {
            engine,
            break_points: HashMap::new(),
        }
    }

    /// Returns an immutable reference to the attached engine.
    #[must_use]
    pub const fn engine(&self) -> &ExecutionEngine {
        &self.engine
    }

    /// Returns a mutable reference to the attached engine.
    pub fn engine_mut(&mut self) -> &mut ExecutionEngine {
        &mut self.engine
    }

    /// Add a breakpoint at the specified position of the specified script.
    /// The VM will break the execution when it reaches the breakpoint.
    /// public void AddBreakPoint(Script script, uint position)
    pub fn add_break_point(&mut self, script: Arc<Script>, position: u32) {
        let hashset = self.break_points.entry(script).or_default();
        hashset.insert(position);
    }

    /// Start or continue execution of the VM.
    /// public `VMState` `Execute()`
    pub fn execute(&mut self) -> VMState {
        if self.engine.state() == VMState::BREAK {
            self.engine.set_state(VMState::NONE);
        }

        while self.engine.state() == VMState::NONE {
            self.execute_and_check_break_points();
        }

        self.engine.state()
    }

    /// private void `ExecuteAndCheckBreakPoints()`
    fn execute_and_check_break_points(&mut self) {
        let _ = self.engine.execute_next();

        if self.engine.state() == VMState::NONE
            && !self.engine.invocation_stack().is_empty()
            && !self.break_points.is_empty()
        {
            if let Some(context) = self.engine.current_context() {
                let script = context.script_arc();
                if let Some(hashset) = self.break_points.get(&script) {
                    if hashset.contains(&(context.instruction_pointer() as u32)) {
                        self.engine.set_state(VMState::BREAK);
                    }
                }
            }
        }
    }

    /// Removes the breakpoint at the specified position in the specified script.
    /// public bool RemoveBreakPoint(Script script, uint position)
    pub fn remove_break_point(&mut self, script: &Arc<Script>, position: u32) -> bool {
        if let Some(hashset) = self.break_points.get_mut(script) {
            if !hashset.remove(&position) {
                return false;
            }

            if hashset.is_empty() {
                self.break_points.remove(script);
            }

            true
        } else {
            false
        }
    }

    /// Returns true if a breakpoint exists for the given script/position.
    #[must_use]
    pub fn has_break_point(&self, script: &Arc<Script>, position: u32) -> bool {
        self.break_points
            .get(script)
            .is_some_and(|set| set.contains(&position))
    }

    /// Returns the total number of registered breakpoints.
    #[must_use]
    pub fn break_point_count(&self) -> usize {
        self.break_points
            .values()
            .map(std::collections::HashSet::len)
            .sum()
    }

    /// Execute the next instruction.
    /// If the instruction involves a call to a method,
    /// it steps into the method and breaks the execution on the first instruction of that method.
    /// public `VMState` `StepInto()`
    pub fn step_into(&mut self) -> VMState {
        if self.engine.state() == VMState::HALT || self.engine.state() == VMState::FAULT {
            return self.engine.state();
        }

        let _ = self.engine.execute_next();

        if self.engine.state() == VMState::NONE {
            self.engine.set_state(VMState::BREAK);
        }

        self.engine.state()
    }

    /// Execute until the currently executed method is returned.
    /// public `VMState` `StepOut()`
    pub fn step_out(&mut self) -> VMState {
        if self.engine.state() == VMState::BREAK {
            self.engine.set_state(VMState::NONE);
        }

        let initial_depth = self.engine.invocation_stack().len();

        while self.engine.state() == VMState::NONE
            && self.engine.invocation_stack().len() >= initial_depth
        {
            self.execute_and_check_break_points();
        }

        if self.engine.state() == VMState::NONE {
            self.engine.set_state(VMState::BREAK);
        }

        self.engine.state()
    }

    /// Execute the next instruction.
    /// If the instruction involves a call to a method, it does not step into the method (it steps over it instead).
    /// public `VMState` `StepOver()`
    pub fn step_over(&mut self) -> VMState {
        if self.engine.state() == VMState::HALT || self.engine.state() == VMState::FAULT {
            return self.engine.state();
        }

        self.engine.set_state(VMState::NONE);
        let initial_depth = self.engine.invocation_stack().len();

        loop {
            self.execute_and_check_break_points();
            if !(self.engine.state() == VMState::NONE
                && self.engine.invocation_stack().len() > initial_depth)
            {
                break;
            }
        }

        if self.engine.state() == VMState::NONE {
            self.engine.set_state(VMState::BREAK);
        }

        self.engine.state()
    }

    /// Execute a single instruction, stepping into calls (alias for `StepInto` in C#)
    /// public `VMState` `Step()`
    pub fn step(&mut self) -> VMState {
        self.step_into()
    }
}
