use alloc::{string::String, vec::Vec};

use neo_base::{hash::Hash160, Bytes};

use crate::{
    error::VmError,
    runtime::{RuntimeHost, Trigger},
    value::VmValue,
};

use super::SyscallDispatcher;

pub(super) fn register_runtime(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register("System.Runtime.Log", runtime_log);
    dispatcher.register("System.Runtime.Notify", runtime_notify);
    dispatcher.register("System.Runtime.Platform", runtime_platform);
    dispatcher.register("System.Runtime.Trigger", runtime_trigger);
    dispatcher.register(
        "System.Runtime.GetInvocationCounter",
        runtime_invocation_counter,
    );
    dispatcher.register("System.Runtime.CheckWitness", runtime_check_witness);
    dispatcher.register("System.Runtime.Time", runtime_time);
    dispatcher.register("System.Runtime.ScriptHash", runtime_script_hash);
    dispatcher.register("System.Runtime.Script", runtime_script);
}

fn runtime_log(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
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
    host.log(message);
    Ok(VmValue::Null)
}

fn runtime_notify(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
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
        payload.push(value.clone());
    }

    host.notify(event_name, payload)
        .map_err(|_| VmError::NativeFailure("runtime notify"))?;
    Ok(VmValue::Null)
}

fn runtime_platform(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::String(host.platform().to_string()))
}

fn runtime_trigger(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    let trigger = match host.trigger() {
        Trigger::Application => "Application",
        Trigger::Verification => "Verification",
    };
    Ok(VmValue::String(trigger.to_string()))
}

fn runtime_invocation_counter(
    host: &mut dyn RuntimeHost,
    _args: &[VmValue],
) -> Result<VmValue, VmError> {
    Ok(VmValue::Int(host.invocation_counter() as i64))
}

fn runtime_check_witness(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let witness = args
        .first()
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    let hash = Hash160::from_slice(witness.as_slice()).map_err(|_| VmError::InvalidType)?;
    Ok(VmValue::Bool(host.check_witness(&hash)))
}

fn runtime_time(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Int(host.timestamp()))
}

fn runtime_script_hash(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    match host.script_hash() {
        Some(hash) => Ok(VmValue::Bytes(Bytes::from(hash.to_vec()))),
        None => Ok(VmValue::Null),
    }
}

fn runtime_script(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Bytes(host.script()))
}
