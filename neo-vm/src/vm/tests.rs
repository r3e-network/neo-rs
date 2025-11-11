use super::executor::*;
use crate::{instruction::Instruction, value::VmValue, VmError};

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

struct AdderInvoker;

impl NativeInvoker for AdderInvoker {
    fn invoke(
        &mut self,
        _contract: &str,
        method: &str,
        args: &[VmValue],
    ) -> Result<VmValue, VmError> {
        match method {
            "sum" => {
                let total = args.iter().try_fold(0i64, |acc, value| {
                    value.as_int().ok_or(VmError::InvalidType).map(|v| acc + v)
                })?;
                Ok(VmValue::Int(total))
            }
            _ => Err(VmError::NativeFailure("unknown method")),
        }
    }
}

#[test]
fn executes_basic_instructions() {
    let instructions = vec![
        Instruction::PushInt(2),
        Instruction::PushInt(3),
        Instruction::Add,
        Instruction::PushInt(4),
        Instruction::Mul,
        Instruction::Return,
    ];
    let mut invoker = NoopInvoker;
    let vm = VirtualMachine::new(&instructions, &mut invoker);
    let result = vm.execute().unwrap();
    assert_eq!(result, VmValue::Int(20));
}

#[test]
fn handles_native_invocation() {
    static CONTRACT: &str = "math";
    static METHOD: &str = "sum";

    let instructions = vec![
        Instruction::PushInt(2),
        Instruction::PushInt(3),
        Instruction::PushInt(5),
        Instruction::PushInt(7),
        Instruction::CallNative {
            contract: CONTRACT,
            method: METHOD,
            arg_count: 4,
        },
        Instruction::Return,
    ];
    let mut invoker = AdderInvoker;
    let vm = VirtualMachine::new(&instructions, &mut invoker);
    let result = vm.execute().unwrap();
    assert_eq!(result, VmValue::Int(17));
}
