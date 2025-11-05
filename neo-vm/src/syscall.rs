use alloc::{string::String, vec::Vec};

use neo_base::{hash::Hash160, Bytes};
use neo_contract::runtime::ExecutionContext;
use neo_store::ColumnId;

use crate::{error::VmError, value::VmValue};

#[derive(Clone, Copy, Debug)]
pub enum Syscall {
    RuntimeLog,
    RuntimeNotify,
    RuntimePlatform,
    RuntimeTrigger,
    RuntimeInvocationCounter,
    StorageGet,
    StoragePut,
    StorageDelete,
    StorageGetContext,
    RuntimeCheckWitness,
    RuntimeTime,
    RuntimeScriptHash,
    RuntimeScript,
}

impl Syscall {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "System.Runtime.Log" => Some(Syscall::RuntimeLog),
            "System.Runtime.Notify" => Some(Syscall::RuntimeNotify),
            "System.Runtime.Platform" => Some(Syscall::RuntimePlatform),
            "System.Runtime.Trigger" => Some(Syscall::RuntimeTrigger),
            "System.Runtime.GetInvocationCounter" => Some(Syscall::RuntimeInvocationCounter),
            "System.Runtime.CheckWitness" => Some(Syscall::RuntimeCheckWitness),
            "System.Runtime.Time" => Some(Syscall::RuntimeTime),
            "System.Runtime.ScriptHash" => Some(Syscall::RuntimeScriptHash),
            "System.Runtime.Script" => Some(Syscall::RuntimeScript),
            "System.Storage.Get" => Some(Syscall::StorageGet),
            "System.Storage.Put" => Some(Syscall::StoragePut),
            "System.Storage.Delete" => Some(Syscall::StorageDelete),
            "System.Storage.GetContext" => Some(Syscall::StorageGetContext),
            _ => None,
        }
    }
}

pub struct SyscallDispatcher<'a> {
    ctx: &'a mut ExecutionContext<'a>,
}

impl<'a> SyscallDispatcher<'a> {
    pub fn new(ctx: &'a mut ExecutionContext<'a>) -> Self {
        Self { ctx }
    }

    pub fn invoke(&mut self, name: &str, args: &[VmValue]) -> Result<VmValue, VmError> {
        match Syscall::from_name(name).ok_or(VmError::UnsupportedSyscall)? {
            Syscall::RuntimeLog => self.runtime_log(args),
            Syscall::RuntimeNotify => self.runtime_notify(args),
            Syscall::RuntimePlatform => self.runtime_platform(args),
            Syscall::RuntimeTrigger => self.runtime_trigger(args),
            Syscall::RuntimeInvocationCounter => self.runtime_invocation_counter(args),
            Syscall::RuntimeCheckWitness => self.runtime_check_witness(args),
            Syscall::RuntimeTime => self.runtime_time(args),
            Syscall::StorageGet => self.storage_get(args),
            Syscall::StoragePut => self.storage_put(args),
            Syscall::StorageDelete => self.storage_delete(args),
            Syscall::StorageGetContext => self.storage_get_context(),
            Syscall::RuntimeScriptHash => self.runtime_script_hash(),
            Syscall::RuntimeScript => self.runtime_script(),
        }
    }

    fn runtime_log(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        let message = match args.first() {
            Some(VmValue::String(s)) => s.clone(),
            Some(VmValue::Bytes(bytes)) => {
                String::from_utf8(bytes.as_slice().to_vec()).map_err(|_| VmError::InvalidType)?
            }
            Some(VmValue::Int(value)) => value.to_string(),
            Some(VmValue::Bool(value)) => value.to_string(),
            Some(_) => return Err(VmError::InvalidType),
            None => String::new(),
        };
        self.ctx.push_log(message);
        Ok(VmValue::Null)
    }

    fn storage_get(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        let column = self.parse_context(args.get(0))?;
        let key = args
            .get(1)
            .and_then(|value| value.as_bytes())
            .ok_or(VmError::InvalidType)?;
        match self
            .ctx
            .load(column, key.as_slice())
            .map_err(|_| VmError::NativeFailure("storage get"))?
        {
            Some(bytes) => Ok(VmValue::Bytes(Bytes::from(bytes))),
            None => Ok(VmValue::Null),
        }
    }

    fn storage_put(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        let column = self.parse_context(args.get(0))?;
        let key = args
            .get(1)
            .and_then(|value| value.as_bytes())
            .ok_or(VmError::InvalidType)?;
        let value = args
            .get(2)
            .and_then(|value| value.as_bytes())
            .ok_or(VmError::InvalidType)?;
        self.ctx
            .put(column, key.as_slice().to_vec(), value.as_slice().to_vec())
            .map_err(|_| VmError::NativeFailure("storage put"))?;
        Ok(VmValue::Null)
    }

    fn runtime_notify(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        if args.is_empty() {
            return Err(VmError::InvalidType);
        }

        let event_name = match &args[0] {
            VmValue::String(s) => s.clone(),
            VmValue::Bytes(bytes) => {
                String::from_utf8(bytes.as_slice().to_vec()).map_err(|_| VmError::InvalidType)?
            }
            _ => return Err(VmError::InvalidType),
        };

        let mut payload = Vec::new();
        for value in &args[1..] {
            payload.push(vm_to_runtime(value.clone())?);
        }

        self.ctx.push_notification(event_name, payload);
        Ok(VmValue::Null)
    }

    fn runtime_platform(&mut self, _args: &[VmValue]) -> Result<VmValue, VmError> {
        Ok(VmValue::String(String::from("NEO")))
    }

    fn runtime_trigger(&mut self, _args: &[VmValue]) -> Result<VmValue, VmError> {
        Ok(VmValue::String(String::from("Application")))
    }

    fn runtime_invocation_counter(&mut self, _args: &[VmValue]) -> Result<VmValue, VmError> {
        Ok(VmValue::Int(self.ctx.invocation_counter() as i64))
    }

    fn runtime_check_witness(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        let witness = args
            .first()
            .and_then(|value| value.as_bytes())
            .ok_or(VmError::InvalidType)?;
        let signer = self
            .ctx
            .signer()
            .ok_or(VmError::NativeFailure("missing signer"))?;
        if witness.len() != 20 {
            return Err(VmError::InvalidType);
        }
        let mut buf = [0u8; 20];
        buf.copy_from_slice(witness.as_slice());
        Ok(VmValue::Bool(Hash160::new(buf) == signer))
    }

    fn runtime_time(&mut self, _args: &[VmValue]) -> Result<VmValue, VmError> {
        let now = self.ctx.timestamp();
        Ok(VmValue::Int(now))
    }

    fn storage_delete(&mut self, args: &[VmValue]) -> Result<VmValue, VmError> {
        let column = self.parse_context(args.get(0))?;
        let key = args
            .get(1)
            .and_then(|value| value.as_bytes())
            .ok_or(VmError::InvalidType)?;
        self.ctx
            .delete(column, key.as_slice())
            .map_err(|_| VmError::NativeFailure("storage delete"))?;
        Ok(VmValue::Null)
    }

    fn storage_get_context(&mut self) -> Result<VmValue, VmError> {
        let ctx = self.ctx.storage_context();
        Ok(VmValue::Bytes(ctx.to_bytes()))
    }

    fn runtime_script_hash(&mut self) -> Result<VmValue, VmError> {
        Ok(VmValue::Bytes(self.ctx.script().clone()))
    }

    fn runtime_script(&mut self) -> Result<VmValue, VmError> {
        Ok(VmValue::Bytes(self.ctx.script().clone()))
    }

    fn parse_context(&self, value: Option<&VmValue>) -> Result<ColumnId, VmError> {
        let bytes = value
            .and_then(|val| val.as_bytes())
            .ok_or(VmError::InvalidType)?;
        if bytes.as_slice().starts_with(b"ctx:") {
            match &bytes.as_slice()[4..] {
                b"contract" => Ok(ColumnId::new("contract")),
                b"storage" => Ok(ColumnId::new("storage")),
                _ => Err(VmError::InvalidType),
            }
        } else {
            match bytes.as_slice() {
                b"contract" => Ok(ColumnId::new("contract")),
                b"storage" => Ok(ColumnId::new("storage")),
                _ => Err(VmError::InvalidType),
            }
        }
    }
}

fn vm_to_runtime(value: VmValue) -> Result<neo_contract::runtime::Value, VmError> {
    use neo_contract::runtime::Value;
    Ok(match value {
        VmValue::Null => Value::Null,
        VmValue::Bool(v) => Value::Bool(v),
        VmValue::Int(v) => Value::Int(v),
        VmValue::Bytes(bytes) => Value::Bytes(bytes),
        VmValue::String(s) => Value::String(s),
    })
}
