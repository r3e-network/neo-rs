//! IDiagnostic - matches C# Neo.SmartContract.IDiagnostic exactly

/// Diagnostic interface for ApplicationEngine (matches C# IDiagnostic)
pub trait IDiagnostic: std::fmt::Debug {
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

/// Placeholder for ExecutionContext from VM
#[derive(Debug)]
pub struct ExecutionContext {
    pub instruction_pointer: usize,
    pub script: Vec<u8>,
}

/// Placeholder for Instruction from VM
#[derive(Debug)]
pub struct Instruction {
    pub op_code: u8,
    pub operand: Vec<u8>,
}
