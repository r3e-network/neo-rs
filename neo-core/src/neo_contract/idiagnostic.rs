use neo::vm::{ApplicationEngine, ExecutionContext, Instruction};

pub trait IDiagnostic {
    fn initialized(&self, engine: &ApplicationEngine);
    fn disposed(&self);
    fn context_loaded(&self, context: &ExecutionContext);
    fn context_unloaded(&self, context: &ExecutionContext);
    fn pre_execute_instruction(&self, instruction: &Instruction);
    fn post_execute_instruction(&self, instruction: &Instruction);
}
