use alloc::{format, string::String, vec::Vec};

use crate::{error::VmError, instruction::Instruction, syscall::SyscallDispatcher, value::VmValue};
use neo_base::Bytes;
use neo_contract::runtime::ExecutionContext;

pub trait NativeInvoker {
    fn invoke(
        &mut self,
        contract: &str,
        method: &str,
        args: &[VmValue],
    ) -> Result<VmValue, VmError>;
}

pub struct VirtualMachine<'a> {
    instructions: &'a [Instruction],
    ip: usize,
    stack: Vec<VmValue>,
    locals: Vec<VmValue>,
    invoker: &'a mut dyn NativeInvoker,
    syscalls: Option<SyscallDispatcher<'a>>,
}

impl<'a> VirtualMachine<'a> {
    pub fn new(instructions: &'a [Instruction], invoker: &'a mut dyn NativeInvoker) -> Self {
        Self {
            instructions,
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            invoker,
            syscalls: None,
        }
    }

    pub fn with_context(
        instructions: &'a [Instruction],
        invoker: &'a mut dyn NativeInvoker,
        context: &'a mut ExecutionContext<'a>,
    ) -> Self {
        Self {
            instructions,
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            invoker,
            syscalls: Some(SyscallDispatcher::new(context)),
        }
    }

    pub fn execute(mut self) -> Result<VmValue, VmError> {
        loop {
            let instruction = self
                .instructions
                .get(self.ip)
                .cloned()
                .ok_or(VmError::Fault)?;
            self.ip += 1;
            match instruction {
                Instruction::PushInt(value) => self.stack.push(VmValue::Int(value)),
                Instruction::PushBool(value) => self.stack.push(VmValue::Bool(value)),
                Instruction::PushBytes(bytes) => self.stack.push(VmValue::Bytes(bytes.into())),
                Instruction::Add => {
                    let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
                    self.stack.push(VmValue::Int(lhs + rhs));
                }
                Instruction::Sub => {
                    let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
                    self.stack.push(VmValue::Int(lhs - rhs));
                }
                Instruction::Mul => {
                    let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
                    self.stack.push(VmValue::Int(lhs * rhs));
                }
                Instruction::Div => {
                    let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
                    if rhs == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.stack.push(VmValue::Int(lhs / rhs));
                }
                Instruction::And => {
                    let (rhs, lhs) = (self.pop_bool()?, self.pop_bool()?);
                    self.stack.push(VmValue::Bool(lhs && rhs));
                }
                Instruction::Or => {
                    let (rhs, lhs) = (self.pop_bool()?, self.pop_bool()?);
                    self.stack.push(VmValue::Bool(lhs || rhs));
                }
                Instruction::Not => {
                    let value = self.pop_value()?;
                    match value {
                        VmValue::Bool(v) => self.stack.push(VmValue::Bool(!v)),
                        VmValue::Int(v) => self.stack.push(VmValue::Int(!v)),
                        _ => return Err(VmError::InvalidType),
                    }
                }
                Instruction::Store(index) => {
                    let value = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    if self.locals.len() <= index {
                        self.locals.resize(index + 1, VmValue::Null);
                    }
                    self.locals[index] = value;
                }
                Instruction::Load(index) => {
                    let value = self.locals.get(index).cloned().unwrap_or(VmValue::Null);
                    self.stack.push(value);
                }
                Instruction::Dup(depth) => {
                    let len = self.stack.len();
                    if depth >= len {
                        return Err(VmError::StackUnderflow);
                    }
                    let value = self.stack[len - 1 - depth].clone();
                    self.stack.push(value);
                }
                Instruction::Swap(depth) => {
                    let len = self.stack.len();
                    if depth >= len {
                        return Err(VmError::StackUnderflow);
                    }
                    let top_index = len - 1;
                    let other_index = len - 1 - depth;
                    self.stack.swap(top_index, other_index);
                }
                Instruction::Drop => {
                    self.pop_value()?;
                }
                Instruction::Over => {
                    let value = self.peek_value(1)?;
                    self.stack.push(value);
                }
                Instruction::Pick(depth) => {
                    let value = self.peek_value(depth)?;
                    self.stack.push(value);
                }
                Instruction::Roll(depth) => {
                    let len = self.stack.len();
                    if depth >= len {
                        return Err(VmError::StackUnderflow);
                    }
                    let index = len - 1 - depth;
                    let value = self.stack.remove(index);
                    self.stack.push(value);
                }
                Instruction::Mod => {
                    let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
                    if rhs == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.stack.push(VmValue::Int(lhs % rhs));
                }
                Instruction::Equal => {
                    let rhs = self.pop_value()?;
                    let lhs = self.pop_value()?;
                    let result = Self::equals(lhs, rhs)?;
                    self.stack.push(VmValue::Bool(result));
                }
                Instruction::Greater => {
                    let rhs = self.pop_int()?;
                    let lhs = self.pop_int()?;
                    self.stack.push(VmValue::Bool(lhs > rhs));
                }
                Instruction::Less => {
                    let rhs = self.pop_int()?;
                    let lhs = self.pop_int()?;
                    self.stack.push(VmValue::Bool(lhs < rhs));
                }
                Instruction::GreaterOrEqual => {
                    let rhs = self.pop_int()?;
                    let lhs = self.pop_int()?;
                    self.stack.push(VmValue::Bool(lhs >= rhs));
                }
                Instruction::LessOrEqual => {
                    let rhs = self.pop_int()?;
                    let lhs = self.pop_int()?;
                    self.stack.push(VmValue::Bool(lhs <= rhs));
                }
                Instruction::NotEqual => {
                    let rhs = self.pop_value()?;
                    let lhs = self.pop_value()?;
                    let result = Self::equals(lhs, rhs)?;
                    self.stack.push(VmValue::Bool(!result));
                }
                Instruction::Xor => {
                    let rhs = self.pop_value()?;
                    let lhs = self.pop_value()?;
                    match (lhs, rhs) {
                        (VmValue::Bool(a), VmValue::Bool(b)) => {
                            self.stack.push(VmValue::Bool(a ^ b));
                        }
                        (VmValue::Int(a), VmValue::Int(b)) => {
                            self.stack.push(VmValue::Int(a ^ b));
                        }
                        _ => return Err(VmError::InvalidType),
                    }
                }
                Instruction::Shl => {
                    let shift = self.pop_int()?;
                    let value = self.pop_int()?;
                    if shift < 0 {
                        return Err(VmError::InvalidType);
                    }
                    self.stack.push(VmValue::Int(
                        value.checked_shl(shift as u32).ok_or(VmError::Fault)?,
                    ));
                }
                Instruction::Shr => {
                    let shift = self.pop_int()?;
                    let value = self.pop_int()?;
                    if shift < 0 {
                        return Err(VmError::InvalidType);
                    }
                    self.stack.push(VmValue::Int(
                        value.checked_shr(shift as u32).ok_or(VmError::Fault)?,
                    ));
                }
                Instruction::ToBool => {
                    let value = self.pop_value()?;
                    self.stack.push(VmValue::Bool(Self::to_bool(value)?));
                }
                Instruction::ToInt => {
                    let value = self.pop_value()?;
                    self.stack.push(VmValue::Int(Self::to_int(value)?));
                }
                Instruction::ToBytes => {
                    let value = self.pop_value()?;
                    self.stack.push(VmValue::Bytes(Self::to_bytes(value)?));
                }
                Instruction::ToString => {
                    let value = self.pop_value()?;
                    self.stack.push(VmValue::String(Self::to_string(value)?));
                }
                Instruction::Syscall(name) => {
                    let args = self.collect_syscall_args()?;
                    let dispatcher = self.syscalls.as_mut().ok_or(VmError::UnsupportedSyscall)?;
                    let result = dispatcher.invoke(name, &args)?;
                    self.stack.push(result);
                }
                Instruction::Negate => {
                    let value = self.pop_int()?;
                    self.stack.push(VmValue::Int(-value));
                }
                Instruction::Inc => {
                    let value = self.pop_int()?;
                    self.stack.push(VmValue::Int(value + 1));
                }
                Instruction::Dec => {
                    let value = self.pop_int()?;
                    self.stack.push(VmValue::Int(value - 1));
                }
                Instruction::Sign => {
                    let value = self.pop_int()?;
                    let sign = if value > 0 {
                        1
                    } else if value < 0 {
                        -1
                    } else {
                        0
                    };
                    self.stack.push(VmValue::Int(sign));
                }
                Instruction::Abs => {
                    let value = self.pop_int()?;
                    let abs = value.checked_abs().ok_or(VmError::Fault)?;
                    self.stack.push(VmValue::Int(abs));
                }
                Instruction::Jump(target) => {
                    self.jump_to(target)?;
                }
                Instruction::JumpIfFalse(target) => {
                    let cond = self.pop_bool()?;
                    if !cond {
                        self.jump_to(target)?;
                    }
                }
                Instruction::CallNative {
                    contract,
                    method,
                    arg_count,
                } => {
                    if self.stack.len() < arg_count {
                        return Err(VmError::StackUnderflow);
                    }
                    let start = self.stack.len() - arg_count;
                    let args = self.stack.split_off(start);
                    let result = self
                        .invoker
                        .invoke(contract, method, &args)
                        .map_err(|_| VmError::NativeFailure("invoke failed"))?;
                    self.stack.push(result);
                }
                Instruction::Return => break,
            }
        }

        Ok(self.stack.pop().unwrap_or(VmValue::Null))
    }

    fn pop_int(&mut self) -> Result<i64, VmError> {
        self.stack
            .pop()
            .and_then(|v| v.as_int())
            .ok_or(VmError::InvalidType)
    }

    fn pop_bool(&mut self) -> Result<bool, VmError> {
        self.stack
            .pop()
            .and_then(|v| v.as_bool())
            .ok_or(VmError::InvalidType)
    }

    fn pop_value(&mut self) -> Result<VmValue, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    fn jump_to(&mut self, target: usize) -> Result<(), VmError> {
        if target >= self.instructions.len() {
            return Err(VmError::Fault);
        }
        self.ip = target;
        Ok(())
    }

    fn peek_value(&self, depth: usize) -> Result<VmValue, VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow);
        }
        Ok(self.stack[len - 1 - depth].clone())
    }

    fn equals(lhs: VmValue, rhs: VmValue) -> Result<bool, VmError> {
        match (lhs, rhs) {
            (VmValue::Null, VmValue::Null) => Ok(true),
            (VmValue::Bool(a), VmValue::Bool(b)) => Ok(a == b),
            (VmValue::Int(a), VmValue::Int(b)) => Ok(a == b),
            (VmValue::Bytes(a), VmValue::Bytes(b)) => Ok(a == b),
            (VmValue::String(a), VmValue::String(b)) => Ok(a == b),
            _ => Err(VmError::InvalidType),
        }
    }

    fn collect_syscall_args(&mut self) -> Result<Vec<VmValue>, VmError> {
        let count = self.pop_int().map_err(|_| VmError::InvalidType)?;
        if count < 0 {
            return Err(VmError::InvalidType);
        }
        let count = count as usize;
        if self.stack.len() < count {
            return Err(VmError::StackUnderflow);
        }
        let start = self.stack.len() - count;
        Ok(self.stack.split_off(start))
    }

    fn to_bool(value: VmValue) -> Result<bool, VmError> {
        Ok(match value {
            VmValue::Bool(v) => v,
            VmValue::Int(v) => v != 0,
            VmValue::Bytes(bytes) => !bytes.is_empty(),
            VmValue::String(s) => !s.is_empty(),
            VmValue::Null => false,
        })
    }

    fn to_int(value: VmValue) -> Result<i64, VmError> {
        match value {
            VmValue::Int(v) => Ok(v),
            VmValue::Bool(v) => Ok(if v { 1 } else { 0 }),
            VmValue::Bytes(bytes) => {
                let data = bytes.as_slice();
                if data.len() > 8 {
                    return Err(VmError::InvalidType);
                }
                let mut buf = [0u8; 8];
                buf[..data.len()].copy_from_slice(data);
                Ok(i64::from_le_bytes(buf))
            }
            VmValue::String(s) => s.parse::<i64>().map_err(|_| VmError::InvalidType),
            VmValue::Null => Ok(0),
        }
    }

    fn to_bytes(value: VmValue) -> Result<Bytes, VmError> {
        Ok(match value {
            VmValue::Bytes(bytes) => bytes,
            VmValue::Bool(v) => Bytes::from(vec![if v { 1 } else { 0 }]),
            VmValue::Int(v) => Bytes::from(v.to_le_bytes().to_vec()),
            VmValue::String(s) => Bytes::from(s.into_bytes()),
            VmValue::Null => Bytes::default(),
        })
    }

    fn to_string(value: VmValue) -> Result<String, VmError> {
        Ok(match value {
            VmValue::String(s) => s,
            VmValue::Bool(v) => v.to_string(),
            VmValue::Int(v) => v.to_string(),
            VmValue::Bytes(bytes) => bytes
                .as_slice()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(""),
            VmValue::Null => String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::{hash::Hash160, Bytes};
    use neo_contract::{
        error::ContractError,
        manifest::{
            ContractManifest, ContractMethod, ContractParameter, ParameterKind, Permission,
            PermissionKind,
        },
        native::{NativeContract, NativeRegistry},
        runtime::{ExecutionContext, InvocationResult, StorageContext, Value},
    };
    use neo_store::{ColumnId, MemoryStore, Store};

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

    struct RegistryAdapter<'a> {
        registry: &'a NativeRegistry,
        ctx: ExecutionContext<'a>,
    }

    impl<'a> NativeInvoker for RegistryAdapter<'a> {
        fn invoke(
            &mut self,
            contract: &str,
            method: &str,
            args: &[VmValue],
        ) -> Result<VmValue, VmError> {
            let values: Vec<Value> = args
                .iter()
                .cloned()
                .map(|value| match value {
                    VmValue::Int(v) => Value::Int(v),
                    VmValue::Bool(v) => Value::Bool(v),
                    VmValue::Bytes(bytes) => Value::Bytes(bytes),
                    VmValue::String(s) => Value::String(s),
                    VmValue::Null => Value::Null,
                })
                .collect();
            let result = self
                .registry
                .invoke(contract, method, &mut self.ctx, &values)
                .map_err(|_| VmError::NativeFailure("registry error"))?;
            Ok(match result.value {
                Value::Int(v) => VmValue::Int(v),
                Value::Bool(v) => VmValue::Bool(v),
                Value::Bytes(bytes) => VmValue::Bytes(bytes),
                Value::String(s) => VmValue::String(s),
                Value::Null => VmValue::Null,
            })
        }
    }

    struct AdderContract {
        manifest: ContractManifest,
    }

    impl NativeContract for AdderContract {
        fn name(&self) -> &'static str {
            "Adder"
        }

        fn manifest(&self) -> &ContractManifest {
            &self.manifest
        }

        fn invoke(
            &self,
            _ctx: &mut ExecutionContext<'_>,
            method: &ContractMethod,
            params: &[Value],
        ) -> Result<InvocationResult, ContractError> {
            match method.name.as_str() {
                "sum" => {
                    let total = params
                        .iter()
                        .map(|value| match value {
                            Value::Int(v) => Ok(*v),
                            _ => Err(ContractError::InvalidParameters),
                        })
                        .sum::<Result<i64, _>>()?;
                    Ok(InvocationResult {
                        value: Value::Int(total),
                        gas_used: 0,
                    })
                }
                _ => Err(ContractError::MethodNotFound {
                    method: method.name.clone(),
                }),
            }
        }
    }

    #[test]
    fn arithmetic_and_native_call() {
        let manifest = ContractManifest {
            name: "Adder".into(),
            groups: vec![],
            methods: vec![ContractMethod {
                name: "sum".into(),
                parameters: vec![
                    ContractParameter {
                        name: "a".into(),
                        kind: ParameterKind::Integer,
                    },
                    ContractParameter {
                        name: "b".into(),
                        kind: ParameterKind::Integer,
                    },
                ],
                return_type: ParameterKind::Integer,
                safe: true,
            }],
            permissions: vec![Permission {
                kind: PermissionKind::Call,
                contract: None,
            }],
        };

        let registry = NativeRegistry::new();
        registry.register(AdderContract { manifest });
        let mut store = MemoryStore::new();
        let ctx = ExecutionContext::new(&mut store, 10_000, None);
        let mut adapter = RegistryAdapter {
            registry: &registry,
            ctx,
        };

        let script = vec![
            Instruction::PushInt(1),
            Instruction::PushInt(2),
            Instruction::CallNative {
                contract: "Adder",
                method: "sum",
                arg_count: 2,
            },
            Instruction::PushInt(3),
            Instruction::Add,
            Instruction::Return,
        ];
        let result = VirtualMachine::new(&script, &mut adapter)
            .execute()
            .expect("vm executes");
        assert_eq!(result, VmValue::Int(6));
    }

    #[test]
    fn dup_and_mul() {
        let script = vec![
            Instruction::PushInt(7),
            Instruction::Dup(0),
            Instruction::Mul,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Int(49));
    }

    #[test]
    fn conditional_branching() {
        let script = vec![
            Instruction::PushInt(5),
            Instruction::PushInt(3),
            Instruction::Greater,
            Instruction::JumpIfFalse(7),
            Instruction::PushInt(42),
            Instruction::Jump(8),
            Instruction::PushInt(0),
            Instruction::Return,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Int(42));
    }

    #[test]
    fn stack_manipulation_ops() {
        let script = vec![
            Instruction::PushInt(1),
            Instruction::PushInt(2),
            Instruction::Over,
            Instruction::Add,
            Instruction::Swap(1),
            Instruction::Drop,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Int(3));
    }

    #[test]
    fn modulo_and_xor_ops() {
        let script = vec![
            Instruction::PushInt(10),
            Instruction::PushInt(3),
            Instruction::Mod,
            Instruction::PushInt(0b1010),
            Instruction::PushInt(0b0110),
            Instruction::Xor,
            Instruction::Add,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Int(1 + (0b1010 ^ 0b0110)));
    }

    #[test]
    fn comparison_extensions() {
        let script = vec![
            Instruction::PushInt(5),
            Instruction::PushInt(5),
            Instruction::GreaterOrEqual,
            Instruction::Dup(0),
            Instruction::NotEqual,
            Instruction::PushInt(9),
            Instruction::PushInt(4),
            Instruction::LessOrEqual,
            Instruction::Xor,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Bool(false));
    }

    #[test]
    fn unary_numeric_ops() {
        let script = vec![
            Instruction::PushInt(3),
            Instruction::Negate,
            Instruction::Dup(0),
            Instruction::Abs,
            Instruction::Swap(1),
            Instruction::Sign,
            Instruction::PushInt(0),
            Instruction::Inc,
            Instruction::PushInt(2),
            Instruction::Dec,
            Instruction::Add,
            Instruction::PushInt(1),
            Instruction::Shl,
            Instruction::PushInt(1),
            Instruction::Shr,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Int(2));
    }

    #[test]
    fn conversion_ops() {
        let script = vec![
            Instruction::PushInt(42),
            Instruction::ToString,
            Instruction::ToInt,
            Instruction::PushBool(true),
            Instruction::ToInt,
            Instruction::Add,
            Instruction::ToBytes,
            Instruction::ToBool,
            Instruction::Return,
        ];
        let mut invoker = NoopInvoker;
        let result = VirtualMachine::new(&script, &mut invoker)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Bool(true));
    }

    #[test]
    fn syscall_log_and_storage() {
        let mut store = MemoryStore::new();
        store.create_column(ColumnId::new("contract"));

        let script = vec![
            Instruction::PushBytes(b"contract".to_vec()),
            Instruction::PushBytes(b"key".to_vec()),
            Instruction::PushBytes(b"value".to_vec()),
            Instruction::PushInt(3),
            Instruction::Syscall("System.Storage.Put"),
            Instruction::Drop,
            Instruction::PushBytes(b"contract".to_vec()),
            Instruction::PushBytes(b"key".to_vec()),
            Instruction::PushInt(2),
            Instruction::Syscall("System.Storage.Get"),
            Instruction::Dup(0),
            Instruction::PushInt(1),
            Instruction::Syscall("System.Runtime.Log"),
            Instruction::Drop,
            Instruction::PushBytes(b"event".to_vec()),
            Instruction::PushBytes(b"payload".to_vec()),
            Instruction::PushInt(2),
            Instruction::Syscall("System.Runtime.Notify"),
            Instruction::Drop,
            Instruction::PushBytes(b"contract".to_vec()),
            Instruction::PushBytes(b"key".to_vec()),
            Instruction::PushInt(2),
            Instruction::Syscall("System.Storage.Delete"),
            Instruction::Drop,
            Instruction::PushBytes(b"contract".to_vec()),
            Instruction::PushBytes(b"key".to_vec()),
            Instruction::PushInt(2),
            Instruction::Syscall("System.Storage.Get"),
            Instruction::Return,
        ];

        {
            let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
            let mut invoker = NoopInvoker;
            let result = {
                let vm = VirtualMachine::with_context(&script, &mut invoker, &mut ctx);
                vm.execute().expect("exec succeeds")
            };
            assert_eq!(result, VmValue::Null);
        }

        let stored = store
            .get(ColumnId::new("contract"), b"key")
            .expect("storage access");
        assert!(stored.is_none());
    }

    #[test]
    fn syscall_check_witness_and_time() {
        let mut store = MemoryStore::new();
        store.create_column(ColumnId::new("contract"));

        let signer = Hash160::new([0x42; 20]);
        let mut ctx = ExecutionContext::with_timestamp(&mut store, 1_000, Some(signer), 1_234);
        ctx.set_invocation_counter(3);
        ctx.set_storage_context(StorageContext::new(ColumnId::new("contract")));
        ctx.set_script(Bytes::from(vec![0xAA, 0xBB]));

        let script = vec![
            Instruction::PushBool(true),
            Instruction::Store(0),
            Instruction::PushBytes(signer.as_ref().to_vec()),
            Instruction::PushInt(1),
            Instruction::Syscall("System.Runtime.CheckWitness"),
            Instruction::PushBool(true),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.Platform"),
            Instruction::ToBytes,
            Instruction::PushBytes(b"NEO".to_vec()),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.Trigger"),
            Instruction::ToBytes,
            Instruction::PushBytes(b"Application".to_vec()),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.GetInvocationCounter"),
            Instruction::PushInt(3),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.Time"),
            Instruction::PushInt(1_234),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.ScriptHash"),
            Instruction::ToBytes,
            Instruction::PushBytes(vec![0xAA, 0xBB]),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Runtime.Script"),
            Instruction::ToBytes,
            Instruction::PushBytes(vec![0xAA, 0xBB]),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::PushInt(0),
            Instruction::Syscall("System.Storage.GetContext"),
            Instruction::PushBytes(b"contract".to_vec()),
            Instruction::Equal,
            Instruction::Load(0),
            Instruction::And,
            Instruction::Store(0),
            Instruction::Load(0),
            Instruction::Return,
        ];

        let mut invoker = NoopInvoker;
        let result = VirtualMachine::with_context(&script, &mut invoker, &mut ctx)
            .execute()
            .expect("exec succeeds");
        assert_eq!(result, VmValue::Bool(true));
    }
}
