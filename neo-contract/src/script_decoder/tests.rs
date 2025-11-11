use super::decode::decode_script;
use neo_core::script::{OpCode, ScriptBuilder};
use neo_vm::{Instruction, NativeInvoker, VirtualMachine, VmError, VmValue};

struct NoopInvoker;

impl NativeInvoker for NoopInvoker {
    fn invoke(
        &mut self,
        _contract: &str,
        _method: &str,
        _args: &[VmValue],
    ) -> Result<VmValue, VmError> {
        Ok(VmValue::Null)
    }
}

#[test]
fn decodes_pushes_and_add() {
    let mut builder = ScriptBuilder::new();
    builder
        .push_int(2)
        .push_int(3)
        .push_opcode(OpCode::Add)
        .push_opcode(OpCode::Return);
    let script = builder.into_script();
    let program = decode_script(&script).expect("decode script");
    let mut invoker = NoopInvoker;
    let result = VirtualMachine::new(&program, &mut invoker)
        .execute()
        .expect("exec succeeds");
    assert_eq!(result, VmValue::Int(5));
}

#[test]
fn decodes_syscall() {
    let mut builder = ScriptBuilder::new();
    builder
        .push_data(b"hello")
        .push_syscall("System.Runtime.Log")
        .push_opcode(OpCode::Return);
    let script = builder.into_script();
    let program = decode_script(&script).expect("decode");
    assert!(matches!(
        program[1],
        Instruction::Syscall("System.Runtime.Log")
    ));
}
