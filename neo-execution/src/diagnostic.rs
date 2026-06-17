//! Diagnostic trait for ApplicationEngine.

use neo_vm::ExecutionContext;
use neo_vm_rs::Instruction;

/// Diagnostic interface for ApplicationEngine.
///
/// Corresponds to C# `Neo.SmartContract.IDiagnostic`. Implementations
/// receive callbacks at each stage of VM execution for tracing and debugging.
pub trait Diagnostic: std::fmt::Debug + Send {
    /// Called when ApplicationEngine is initialized.
    fn initialized(&mut self, engine: &mut crate::ApplicationEngine);

    /// Called when ApplicationEngine is disposed.
    fn disposed(&mut self);

    /// Called when an ExecutionContext is loaded.
    fn context_loaded(&mut self, context: &ExecutionContext);

    /// Called when an ExecutionContext is unloaded.
    fn context_unloaded(&mut self, context: &ExecutionContext);

    /// Called before executing an instruction.
    fn pre_execute_instruction(&mut self, instruction: &Instruction);

    /// Called after executing an instruction.
    fn post_execute_instruction(&mut self, instruction: &Instruction);
}

/// A simple diagnostic implementation that counts executed instructions.
///
/// This provides a minimal but functional diagnostic that can be used
/// for basic profiling and debugging without a full trace viewer.
#[derive(Debug, Default)]
pub struct InstructionCounter {
    /// Total number of instructions executed.
    pub executed_count: u64,
    /// Number of contexts loaded.
    pub contexts_loaded: u64,
    /// Number of contexts unloaded.
    pub contexts_unloaded: u64,
    /// Whether the engine has been initialized.
    pub initialized: bool,
}

impl InstructionCounter {
    /// Creates a new instruction counter.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Diagnostic for InstructionCounter {
    fn initialized(&mut self, _engine: &mut crate::ApplicationEngine) {
        self.initialized = true;
        self.executed_count = 0;
        self.contexts_loaded = 0;
        self.contexts_unloaded = 0;
    }

    fn disposed(&mut self) {
        self.initialized = false;
    }

    fn context_loaded(&mut self, _context: &ExecutionContext) {
        self.contexts_loaded += 1;
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {
        self.contexts_unloaded += 1;
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {
        self.executed_count += 1;
    }
}
