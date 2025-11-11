use neo_base::Bytes;
use neo_store::ColumnId;

use crate::{error::VmError, runtime::RuntimeHost, value::VmValue};

use super::SyscallDispatcher;

pub(super) fn register_storage(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register("System.Storage.GetContext", storage_get_context);
    dispatcher.register("System.Storage.Get", storage_get);
    dispatcher.register("System.Storage.Put", storage_put);
    dispatcher.register("System.Storage.Delete", storage_delete);
}

fn storage_get_context(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Bytes(host.storage_context_bytes()))
}

fn storage_get(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    match host
        .load_storage(column, key.as_slice())
        .map_err(|_| VmError::NativeFailure("storage get"))?
    {
        Some(bytes) => Ok(VmValue::Bytes(Bytes::from(bytes))),
        None => Ok(VmValue::Null),
    }
}

fn storage_put(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    let value = args
        .get(2)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    host.put_storage(column, key.as_slice(), value.as_slice())
        .map_err(|_| VmError::NativeFailure("storage put"))?;
    Ok(VmValue::Null)
}

fn storage_delete(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    host.delete_storage(column, key.as_slice())
        .map_err(|_| VmError::NativeFailure("storage delete"))?;
    Ok(VmValue::Null)
}

fn parse_context(value: Option<&VmValue>) -> Result<ColumnId, VmError> {
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
