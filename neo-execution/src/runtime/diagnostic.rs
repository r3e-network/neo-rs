//! Diagnostic trait for ApplicationEngine.

use crate::ApplicationExecutionContext;
use neo_storage::CacheRead;
use neo_vm_rs::Instruction;

/// Diagnostic interface for ApplicationEngine.
///
/// Corresponds to C# `Neo.SmartContract.IDiagnostic`. Implementations
/// receive callbacks at each stage of VM execution for tracing and debugging.
pub trait Diagnostic: std::fmt::Debug + Send {
    /// Returns whether this sink wants diagnostic callbacks.
    fn enabled(&self) -> bool {
        true
    }

    /// Called when ApplicationEngine is initialized.
    fn initialized(&mut self);

    /// Called when ApplicationEngine is disposed.
    fn disposed(&mut self);

    /// Called when an ExecutionContext is loaded.
    fn context_loaded<B: CacheRead>(&mut self, context: &ApplicationExecutionContext<B>);

    /// Called when an ExecutionContext is unloaded.
    fn context_unloaded<B: CacheRead>(&mut self, context: &ApplicationExecutionContext<B>);

    /// Called before executing an instruction.
    fn pre_execute_instruction(&mut self, instruction: &Instruction);

    /// Called after executing an instruction.
    fn post_execute_instruction(&mut self, instruction: &Instruction);
}

/// No-op diagnostic sink used by the hot execution path.
///
/// Keeping this as a concrete type lets [`crate::ApplicationEngine`] compile
/// without dynamic diagnostic dispatch when tracing is disabled.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoDiagnostic;

impl Diagnostic for NoDiagnostic {
    fn enabled(&self) -> bool {
        false
    }

    fn initialized(&mut self) {}

    fn disposed(&mut self) {}

    fn context_loaded<B: CacheRead>(&mut self, _context: &ApplicationExecutionContext<B>) {}

    fn context_unloaded<B: CacheRead>(&mut self, _context: &ApplicationExecutionContext<B>) {}

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}

impl<D> Diagnostic for Option<D>
where
    D: Diagnostic,
{
    fn enabled(&self) -> bool {
        self.as_ref().is_some_and(|diagnostic| diagnostic.enabled())
    }

    fn initialized(&mut self) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.initialized();
        }
    }

    fn disposed(&mut self) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.disposed();
        }
    }

    fn context_loaded<B: CacheRead>(&mut self, context: &ApplicationExecutionContext<B>) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.context_loaded(context);
        }
    }

    fn context_unloaded<B: CacheRead>(&mut self, context: &ApplicationExecutionContext<B>) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.context_unloaded(context);
        }
    }

    fn pre_execute_instruction(&mut self, instruction: &Instruction) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.pre_execute_instruction(instruction);
        }
    }

    fn post_execute_instruction(&mut self, instruction: &Instruction) {
        if let Some(diagnostic) = self.as_mut() {
            diagnostic.post_execute_instruction(instruction);
        }
    }
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
    fn initialized(&mut self) {
        self.initialized = true;
        self.executed_count = 0;
        self.contexts_loaded = 0;
        self.contexts_unloaded = 0;
    }

    fn disposed(&mut self) {
        self.initialized = false;
    }

    fn context_loaded<B: CacheRead>(&mut self, _context: &ApplicationExecutionContext<B>) {
        self.contexts_loaded += 1;
    }

    fn context_unloaded<B: CacheRead>(&mut self, _context: &ApplicationExecutionContext<B>) {
        self.contexts_unloaded += 1;
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {
        self.executed_count += 1;
    }
}
