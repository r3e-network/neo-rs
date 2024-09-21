use neo_vm::execution_context::ExecutionContext;
use neo_vm::instruction::Instruction;
use neo_vm::vm::{ExecutionContext, Instruction};
use crate::neo_contract::application_engine::ApplicationEngine;

pub trait IDiagnostic {
    fn initialized(&self, engine: &ApplicationEngine);
    fn disposed(&self);
    fn context_loaded(&self, context: &ExecutionContext);
    fn context_unloaded(&self, context: &ExecutionContext);
    fn pre_execute_instruction(&self, instruction: &Instruction);
    fn post_execute_instruction(&self, instruction: &Instruction);
}
