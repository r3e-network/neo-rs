//! IDiagnostic - matches C# Neo.SmartContract.IDiagnostic exactly

use neo_vm::execution_context::ExecutionContext;
use neo_vm::instruction::Instruction;

/// Diagnostic interface for ApplicationEngine (matches C# IDiagnostic)
pub trait IDiagnostic: std::fmt::Debug + Send {
    /// Called when ApplicationEngine is initialized
    fn initialized(&mut self, engine: &mut crate::smart_contract::ApplicationEngine);

    /// Called when ApplicationEngine is disposed
    fn disposed(&mut self);

    /// Called when an ExecutionContext is loaded
    fn context_loaded(&mut self, context: &ExecutionContext);

    /// Called when an ExecutionContext is unloaded
    fn context_unloaded(&mut self, context: &ExecutionContext);

    /// Called before executing an instruction
    fn pre_execute_instruction(&mut self, instruction: &Instruction);

    /// Called after executing an instruction
    fn post_execute_instruction(&mut self, instruction: &Instruction);
}
